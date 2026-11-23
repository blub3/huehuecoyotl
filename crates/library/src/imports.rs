//! Add to Library (3.0, issue #154, SPEC 7): what an import copies and where it goes. Each album,
//! movie, show or book is one job of files; the copying itself is in the core. A job waits in
//! `review` instead of guessing a match or writing over anything.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::{Value, json};

use crate::naming::{self, Album, Book, Episode, Movie, Track, VideoTech};
use crate::{DbError, Result, versions};

pub const SCHEMA: &str = r#"
CREATE TABLE import_job(id INTEGER PRIMARY KEY,
                        item_id INTEGER REFERENCES item(id) ON DELETE SET NULL,
                        type TEXT NOT NULL,
                        title TEXT NOT NULL,
                        state TEXT NOT NULL CHECK (state IN ('queued','paused','review','done','failed','skipped')),
                        review TEXT,
                        note TEXT NOT NULL DEFAULT '',
                        automatic INTEGER NOT NULL DEFAULT 0,
                        folder TEXT NOT NULL DEFAULT '',
                        version_id INTEGER,
                        created_at INTEGER NOT NULL,
                        updated_at INTEGER NOT NULL);
CREATE INDEX import_job_state ON import_job(state, id);
CREATE TABLE import_file(id INTEGER PRIMARY KEY,
                         job_id INTEGER NOT NULL REFERENCES import_job(id) ON DELETE CASCADE,
                         file_id INTEGER REFERENCES file(id) ON DELETE SET NULL,
                         item_id INTEGER,
                         dest TEXT NOT NULL,
                         size INTEGER NOT NULL DEFAULT 0,
                         state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','done','skipped')),
                         UNIQUE (job_id, dest));
CREATE INDEX import_file_job ON import_file(job_id, state);
"#;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanFile {
    pub file: i64,
    pub item: i64,
    /// Inside the type's folder of the library.
    pub dest: String,
    pub size: i64,
}

/// One album, movie, show or book to import.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub item: i64,
    /// movies, shows, music or audiobooks.
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub folder: String,
    pub files: Vec<PlanFile>,
    pub already: usize,
    /// Why it waits: `match` (not surely matched) or `clash` (the library has another copy).
    pub review: Option<String>,
    pub version: i64,
}

fn after_type(path: &str) -> String {
    path.split_once('/').map_or(path, |(_, r)| r).to_string()
}

fn ext(path: &str) -> String {
    path.rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .filter(|e| e.len() <= 6)
        .unwrap_or_default()
}

fn is_managed_file(c: &Connection, item: i64) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS (SELECT 1 FROM file f JOIN source s ON s.id = f.source_id
           WHERE f.item_id = ?1 AND (s.managed IS NOT NULL OR s.moving_to IS NOT NULL))",
        [item],
        |r| r.get(0),
    )?)
}

fn source_is_managed(c: &Connection, id: i64) -> Result<bool> {
    crate::managed::is_library_source(c, id)
}

/// Taken: the path holds library files of something else than `owner` (or its children).
fn folder_taken(c: &Connection, kind: &str, rel: &str, owner: i64) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS (SELECT 1 FROM file f JOIN source s ON s.id = f.source_id
           LEFT JOIN item i ON i.id = f.item_id
           WHERE s.managed = ?1 AND substr(f.path, 1, length(?2) + 1) = ?2 || '/'
             AND coalesce(i.parent_id, 0) != ?3 AND coalesce(f.item_id, 0) != ?3)",
        params![kind, rel, owner],
        |r| r.get(0),
    )?)
}

fn first_credit(c: &Connection, item: i64, role: &str) -> Result<String> {
    Ok(c.query_row(
        "SELECT coalesce(a.title, '') FROM credit cr JOIN item a ON a.id = cr.artist_id
         WHERE cr.item_id = ?1 AND cr.role = ?2 ORDER BY cr.position LIMIT 1",
        params![item, role],
        |r| r.get(0),
    )
    .optional()?
    .unwrap_or_default())
}

fn year_of(s: &str) -> Option<i32> {
    s.get(..4).and_then(|y| y.parse().ok())
}

fn path_of(c: &Connection, file: i64) -> Result<String> {
    Ok(c.query_row("SELECT path FROM file WHERE id = ?1", [file], |r| r.get(0))?)
}

/// `{codec}-{rate}-{depth}` of a version's files, to tell another quality from the same one.
fn quality(c: &Connection, version: i64) -> Result<String> {
    Ok(c.query_row(
        "SELECT coalesce(fa.codec, '') || '-' || coalesce(fa.sample_rate, 0) || '-' || coalesce(fa.bit_depth, 0)
         FROM file f LEFT JOIN file_audio fa ON fa.file_id = f.id WHERE f.version_id = ?1 LIMIT 1",
        [version],
        |r| r.get(0),
    )
    .optional()?
    .unwrap_or_default())
}

fn plan_album(c: &Connection, album: i64, only: Option<i64>) -> Result<Option<Plan>> {
    let (title, year, state, parent, date, compilation, disc_total): (
        String, Option<i32>, Option<String>, Option<i64>, String, bool, i64,
    ) = c.query_row(
        "SELECT coalesce(a.title, ''), a.year, a.match_state, a.parent_id, coalesce(m.release_date, ''),
                coalesce(m.compilation, 0), coalesce(m.disc_total, 0)
         FROM item a LEFT JOIN music m ON m.item_id = a.id WHERE a.id = ?1",
        [album],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get::<_, i64>(5)? != 0, r.get(6)?)),
    )?;
    let release: Option<String> = c
        .query_row(
            "SELECT value FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = 'release' LIMIT 1",
            [album],
            |r| r.get(0),
        )
        .optional()?;
    let vs = versions::versions(c, album)?;
    let mut version = None;
    let mut library_version = None;
    for v in &vs {
        if source_is_managed(c, v.source_id)? {
            library_version.get_or_insert(v.id);
        } else {
            version.get_or_insert(v.id);
        }
    }
    let artist_name = {
        let a = first_credit(c, album, "album_artist")?;
        if a.is_empty() {
            match parent {
                Some(p) => c.query_row(
                    "SELECT coalesce(title, '') FROM item WHERE id = ?1",
                    [p],
                    |r| r.get(0),
                )?,
                None => String::new(),
            }
        } else {
            a
        }
    };
    let subtitle = artist_name.clone();
    let Some(version) = version else {
        return Ok(library_version.map(|v| Plan {
            item: album,
            kind: "music".into(),
            title: title.clone(),
            subtitle: subtitle.clone(),
            folder: String::new(),
            files: vec![],
            already: 1,
            review: None,
            version: v,
        }));
    };
    let tracks: Vec<(i64, i64)> = versions::version_tracks(c, version, "quality")?
        .into_iter()
        .filter(|(t, _)| only.is_none_or(|o| o == *t))
        .collect();
    let edition = vs
        .iter()
        .find(|v| v.id == version)
        .map(|v| v.edition.clone())
        .unwrap_or_default();
    let a = Album {
        album_artist: artist_name,
        title: title.clone(),
        year: year_of(&date).or(year),
        disambiguation: String::new(),
        edition,
        release_id: release.clone().unwrap_or_default(),
    };
    let taken_err = std::cell::Cell::new(None::<DbError>);
    let folder = naming::album_folder(
        &a,
        &|p| match folder_taken(c, "music", &after_type(p), album) {
            Ok(t) => t,
            Err(e) => {
                taken_err.set(Some(e));
                false
            }
        },
    );
    if let Some(e) = taken_err.take() {
        return Err(e);
    }
    let folder_rel = after_type(&folder);
    let discs = {
        let set: BTreeSet<i64> = {
            let mut st = c.prepare("SELECT DISTINCT coalesce(season_no, 1) FROM item WHERE parent_id = ?1 AND kind = 'track'")?;
            st.query_map([album], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        (set.len() as i64).max(disc_total)
    };
    let mut files = Vec::new();
    let mut held = Vec::new();
    let mut already = 0;
    for (t, file) in &tracks {
        let in_library = is_managed_file(c, *t)?;
        already += usize::from(in_library);
        let (name, disc, no): (String, Option<u32>, Option<u32>) = c.query_row(
            "SELECT coalesce(title, ''), season_no, episode_no FROM item WHERE id = ?1",
            [t],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let p = path_of(c, *file)?;
        let size: i64 = c.query_row(
            "SELECT coalesce(size, 0) FROM file WHERE id = ?1",
            [file],
            |r| r.get(0),
        )?;
        let tf = naming::track_file(&Track {
            disc: disc.unwrap_or(1),
            discs: discs.max(1) as u32,
            number: no.filter(|n| *n > 0),
            title: name,
            artist: first_credit(c, *t, "artist")?,
            compilation,
            ext: ext(&p),
        });
        let f = PlanFile {
            file: *file,
            item: *t,
            dest: format!("{folder_rel}/{tf}"),
            size,
        };
        if in_library {
            held.push(f);
        } else {
            files.push(f);
        }
    }
    // Every track is there: the same quality is done, another one is the user's call.
    let mut review = None;
    if files.is_empty() && already > 0 {
        if let Some(l) = library_version
            && quality(c, l)? != quality(c, version)?
        {
            review = Some("clash".to_string());
            files = held;
            already = 0;
        }
    } else if state.as_deref() != Some("matched") && release.is_none() {
        review = Some("match".to_string());
    }
    Ok(Some(Plan {
        item: album,
        kind: "music".into(),
        title,
        subtitle,
        folder: folder_rel,
        files,
        already,
        review,
        version,
    }))
}

/// (file, path, size, probe)
type VideoFile = (i64, String, i64, Option<String>);

/// The file to copy of a video: the largest outside the library.
fn best_video(c: &Connection, item: i64) -> Result<Option<VideoFile>> {
    Ok(c.query_row(
        "SELECT f.id, f.path, coalesce(f.size, 0), f.probe_json FROM file f JOIN source s ON s.id = f.source_id
         WHERE f.item_id = ?1 AND s.managed IS NULL AND s.moving_to IS NULL ORDER BY f.size DESC, f.id LIMIT 1",
        [item],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .optional()?)
}

/// What the name of a video says about it, from its probe.
pub fn video_tech(probe: Option<&str>) -> VideoTech {
    let Some(v) = probe.and_then(|p| serde_json::from_str::<Value>(p).ok()) else {
        return VideoTech::default();
    };
    let video = &v["video"][0];
    let audio = &v["audio"][0];
    let height = video["height"].as_u64().unwrap_or(0);
    let width = video["width"].as_u64().unwrap_or(0);
    let resolution = match (width, height) {
        (w, h) if w >= 3000 || h >= 1800 => "2160p",
        (w, h) if w >= 1800 || h >= 1000 => "1080p",
        (w, h) if w >= 1200 || h >= 700 => "720p",
        (_, h) if h >= 560 => "576p",
        (_, h) if h > 0 => "480p",
        _ => "",
    };
    let up = |s: &str| match s {
        "hevc" => "HEVC".to_string(),
        "h264" => "H264".to_string(),
        "av1" => "AV1".to_string(),
        "eac3" => "EAC3".to_string(),
        "ac3" => "AC3".to_string(),
        "truehd" => "TrueHD".to_string(),
        "dts" => "DTS".to_string(),
        "aac" => "AAC".to_string(),
        "flac" => "FLAC".to_string(),
        o => o.to_uppercase(),
    };
    let depth = video["bit_depth"]
        .as_u64()
        .filter(|d| *d > 8)
        .map(|d| format!("{d}bit"))
        .unwrap_or_default();
    let profile = audio["profile"].as_str().unwrap_or("");
    let channels = audio["layout"]
        .as_str()
        .map(|l| {
            if l == "stereo" {
                "2.0"
            } else if l == "mono" {
                "1.0"
            } else {
                l
            }
        })
        .unwrap_or("");
    VideoTech {
        resolution: resolution.into(),
        hdr: video["hdr"].as_str().unwrap_or("").to_string(),
        vcodec: video["codec"].as_str().map(up).unwrap_or_default(),
        bitdepth: depth,
        acodec: audio["codec"].as_str().map(up).unwrap_or_default(),
        atmos: profile.contains("Atmos"),
        channels: channels.to_string(),
    }
}

fn plan_movie(c: &Connection, item: i64) -> Result<Option<Plan>> {
    let (title, year, tmdb, imdb, state): (
        String,
        Option<i32>,
        Option<i64>,
        Option<String>,
        Option<String>,
    ) = c.query_row(
        "SELECT coalesce(title, ''), year, tmdb_id, imdb_id, match_state FROM item WHERE id = ?1",
        [item],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let has = is_managed_file(c, item)?;
    let best = best_video(c, item)?;
    // Issue #155: the film is there, in another quality: the user decides.
    let other_quality = has
        && match &best {
            Some((_, _, _, probe)) => {
                let lib: Option<String> = c
                    .query_row(
                        "SELECT f.probe_json FROM file f JOIN source s ON s.id = f.source_id
                         WHERE f.item_id = ?1 AND s.managed IS NOT NULL LIMIT 1",
                        [item],
                        |r| r.get(0),
                    )
                    .optional()?
                    .flatten();
                let (a, b) = (video_tech(probe.as_deref()), video_tech(lib.as_deref()));
                (a.resolution, a.vcodec) != (b.resolution, b.vcodec)
            }
            None => false,
        };
    let best = if has && !other_quality { None } else { best };
    let mut files = Vec::new();
    let mut folder = String::new();
    if let Some((file, path, size, probe)) = best {
        let full = naming::movie(&Movie {
            title: title.clone(),
            year,
            imdb: imdb.unwrap_or_default(),
            tmdb,
            tech: video_tech(probe.as_deref()),
            ext: ext(&path),
            ..Default::default()
        });
        let dest = after_type(&full);
        folder = dest.rsplit_once('/').map_or("", |(d, _)| d).to_string();
        files.push(PlanFile {
            file,
            item,
            dest,
            size,
        });
    }
    let review = if other_quality {
        Some("clash".to_string())
    } else {
        (!files.is_empty() && state.as_deref() != Some("matched")).then(|| "match".to_string())
    };
    Ok(Some(Plan {
        item,
        kind: "movies".into(),
        subtitle: year.map(|y| y.to_string()).unwrap_or_default(),
        title,
        folder,
        already: usize::from(has && !other_quality),
        files,
        review,
        version: 0,
    }))
}

fn plan_episodes(c: &Connection, show: i64, episodes: &[i64]) -> Result<Option<Plan>> {
    let (title, year, tmdb, imdb, state): (
        String,
        Option<i32>,
        Option<i64>,
        Option<String>,
        Option<String>,
    ) = c.query_row(
        "SELECT coalesce(title, ''), year, tmdb_id, imdb_id, match_state FROM item WHERE id = ?1",
        [show],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let mut files = Vec::new();
    let mut already = 0;
    let mut folder = String::new();
    for e in episodes {
        if is_managed_file(c, *e)? {
            already += 1;
            continue;
        }
        let Some((file, path, size, probe)) = best_video(c, *e)? else {
            continue;
        };
        let (name, season, number, etmdb, eimdb): (String, Option<u32>, Option<u32>, Option<i64>, Option<String>) = c.query_row(
            "SELECT coalesce(title, ''), season_no, episode_no, tmdb_id, imdb_id FROM item WHERE id = ?1",
            [e],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
        let full = naming::episode(&Episode {
            show: title.clone(),
            first_air_year: year,
            show_imdb: imdb.clone().unwrap_or_default(),
            show_tmdb: tmdb,
            season: season.unwrap_or(0),
            episode: number.unwrap_or(0),
            last_episode: None,
            title: name,
            imdb: eimdb.unwrap_or_default(),
            tmdb: etmdb,
            tech: video_tech(probe.as_deref()),
            ext: ext(&path),
        });
        let dest = after_type(&full);
        if folder.is_empty() {
            folder = dest.split('/').next().unwrap_or("").to_string();
        }
        files.push(PlanFile {
            file,
            item: *e,
            dest,
            size,
        });
    }
    let review =
        (!files.is_empty() && state.as_deref() != Some("matched")).then(|| "match".to_string());
    Ok(Some(Plan {
        item: show,
        kind: "shows".into(),
        subtitle: year.map(|y| y.to_string()).unwrap_or_default(),
        title,
        folder,
        files,
        already,
        review,
        version: 0,
    }))
}

fn plan_book(c: &Connection, book: i64) -> Result<Option<Plan>> {
    let Some(t) = crate::copy_meta::book_tags_unlocked(c, book)? else {
        return Ok(None);
    };
    let state: Option<String> =
        c.query_row("SELECT match_state FROM item WHERE id = ?1", [book], |r| {
            r.get(0)
        })?;
    let b = Book {
        author: t.artists.first().cloned().unwrap_or_default(),
        title: t.title.clone(),
        year: year_of(&t.date),
        series: t.series.clone(),
        series_number: t.series_part.clone(),
        narrator: t.narrators.first().cloned().unwrap_or_default(),
        edition: String::new(),
    };
    let taken_err = std::cell::Cell::new(None::<DbError>);
    let folder =
        naming::book_folder(
            &b,
            &|p| match folder_taken(c, "audiobooks", &after_type(p), book) {
                Ok(t) => t,
                Err(e) => {
                    taken_err.set(Some(e));
                    false
                }
            },
        );
    if let Some(e) = taken_err.take() {
        return Err(e);
    }
    let folder = after_type(&folder);
    let rows: Vec<(i64, i64, String, i64, bool, Option<String>)> = {
        let mut st = c.prepare(
            "SELECT bf.file_id, bf.idx, f.path, coalesce(f.size, 0), s.managed IS NOT NULL OR s.moving_to IS NOT NULL,
                    (SELECT ch.title FROM chapter ch WHERE ch.book_id = bf.book_id AND ch.file_id = bf.file_id ORDER BY ch.idx LIMIT 1)
             FROM book_file bf JOIN file f ON f.id = bf.file_id JOIN source s ON s.id = f.source_id
             WHERE bf.book_id = ?1 ORDER BY bf.idx",
        )?;
        st.query_map([book], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    let already = rows.iter().filter(|r| r.4).count();
    let many = rows.len() > 1;
    let mut files = Vec::new();
    if already == 0 {
        for (file, idx, path, size, _, chapter) in &rows {
            let fallback = path
                .rsplit('/')
                .next()
                .unwrap_or("")
                .rsplit_once('.')
                .map_or("", |(s, _)| s)
                .to_string();
            let chapter_title = chapter
                .clone()
                .filter(|t| !t.is_empty())
                .unwrap_or(fallback);
            let name = naming::book_file(
                &b,
                many.then_some((*idx as u32 + 1, chapter_title.as_str())),
                &ext(path),
            );
            files.push(PlanFile {
                file: *file,
                item: book,
                dest: format!("{folder}/{name}"),
                size: *size,
            });
        }
    }
    let review =
        (!files.is_empty() && state.as_deref() != Some("matched")).then(|| "match".to_string());
    let subtitle = b.author.clone();
    Ok(Some(Plan {
        item: book,
        kind: "audiobooks".into(),
        title: t.title,
        subtitle,
        folder,
        files,
        already: usize::from(already > 0),
        review,
        version: 0,
    }))
}

/// What Add to Library would do for an item: one plan per album, movie, show or book.
pub fn plan(c: &Connection, item: i64) -> Result<Vec<Plan>> {
    let (kind, parent): (String, Option<i64>) = c.query_row(
        "SELECT kind, parent_id FROM item WHERE id = ?1",
        [item],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let children = |p: i64, k: &str| -> Result<Vec<i64>> {
        let mut st = c.prepare("SELECT id FROM item WHERE parent_id = ?1 AND kind = ?2 ORDER BY coalesce(season_no, 0), coalesce(episode_no, 0), id")?;
        let rows = st.query_map(params![p, k], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    };
    let mut out = Vec::new();
    match (kind.as_str(), parent) {
        ("track", Some(album)) => out.extend(plan_album(c, album, Some(item))?),
        ("album", _) => out.extend(plan_album(c, item, None)?),
        ("artist", _) => {
            for a in children(item, "album")? {
                out.extend(plan_album(c, a, None)?);
            }
        }
        ("movie" | "video", _) => out.extend(plan_movie(c, item)?),
        ("show", _) => {
            let mut eps = Vec::new();
            for s in children(item, "season")? {
                eps.extend(children(s, "episode")?);
            }
            out.extend(plan_episodes(c, item, &eps)?);
        }
        ("season", Some(show)) => out.extend(plan_episodes(c, show, &children(item, "episode")?)?),
        ("episode", Some(season)) => {
            let show: Option<i64> =
                c.query_row("SELECT parent_id FROM item WHERE id = ?1", [season], |r| {
                    r.get(0)
                })?;
            if let Some(show) = show {
                out.extend(plan_episodes(c, show, &[item])?);
            }
        }
        ("audiobook", _) => out.extend(plan_book(c, item)?),
        _ => {}
    }
    Ok(out)
}

/// Everything of a source that is not in the library yet, as items to plan.
pub fn items_of_source(c: &Connection, source: i64) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT DISTINCT CASE
             WHEN i.kind = 'track' THEN i.parent_id
             WHEN i.kind = 'episode' THEN (SELECT p.parent_id FROM item p WHERE p.id = i.parent_id)
             ELSE i.id END
         FROM file f JOIN item i ON i.id = f.item_id WHERE f.source_id = ?1",
    )?;
    let rows = st.query_map([source], |r| r.get::<_, Option<i64>>(0))?;
    let v: Vec<Option<i64>> = rows.collect::<rusqlite::Result<_>>()?;
    Ok(v.into_iter().flatten().collect())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Start the plans: one job each. A job already open for the item stays; nothing to copy is
/// recorded as "already in your library". Returns the jobs that have work.
pub fn start(c: &mut Connection, plans: &[Plan], automatic: bool) -> Result<Vec<i64>> {
    let tx = c.transaction()?;
    let t = now();
    let mut ids = Vec::new();
    for p in plans {
        let open: Option<i64> = tx
            .query_row(
                "SELECT id FROM import_job WHERE item_id = ?1 AND state IN ('queued', 'paused', 'review')",
                [p.item],
                |r| r.get(0),
            )
            .optional()?;
        if open.is_some() {
            continue;
        }
        let state = if p.files.is_empty() {
            "done"
        } else if p.review.is_some() {
            "review"
        } else {
            "queued"
        };
        let note = if p.files.is_empty() { "already" } else { "" };
        tx.execute(
            "INSERT INTO import_job(item_id, type, title, state, review, note, automatic, folder, version_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![p.item, p.kind, p.title, state, p.review, note, automatic, p.folder, p.version, t],
        )?;
        let job = tx.last_insert_rowid();
        for f in &p.files {
            tx.execute(
                "INSERT OR IGNORE INTO import_file(job_id, file_id, item_id, dest, size) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![job, f.file, f.item, f.dest, f.size],
            )?;
        }
        if state == "queued" {
            ids.push(job);
        }
    }
    tx.commit()?;
    Ok(ids)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Work {
    pub job: i64,
    pub row: i64,
    pub file: i64,
    pub item: i64,
    pub dest: String,
    pub kind: String,
    pub version: i64,
    pub album: Option<i64>,
}

/// The next file to copy.
pub fn next(c: &Connection) -> Result<Option<Work>> {
    Ok(c.query_row(
        "SELECT j.id, f.id, f.file_id, f.item_id, f.dest, j.type, coalesce(j.version_id, 0), j.item_id
         FROM import_job j JOIN import_file f ON f.job_id = j.id
         WHERE j.state = 'queued' AND f.state = 'pending' ORDER BY j.id, f.id LIMIT 1",
        [],
        |r| {
            Ok(Work {
                job: r.get(0)?,
                row: r.get(1)?,
                file: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                item: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                dest: r.get(4)?,
                kind: r.get(5)?,
                version: r.get(6)?,
                album: r.get(7)?,
            })
        },
    )
    .optional()?)
}

/// Queued jobs whose files are all handled become done.
pub fn settle(c: &Connection) -> Result<()> {
    c.execute(
        "UPDATE import_job SET state = 'done', updated_at = ?1
         WHERE state = 'queued' AND NOT EXISTS
           (SELECT 1 FROM import_file f WHERE f.job_id = import_job.id AND f.state = 'pending')",
        [now()],
    )?;
    Ok(())
}

pub fn file_skipped(c: &Connection, row: i64) -> Result<()> {
    c.execute(
        "UPDATE import_file SET state = 'skipped' WHERE id = ?1",
        [row],
    )?;
    Ok(())
}

pub fn set_state(
    c: &Connection,
    job: i64,
    state: &str,
    review: Option<&str>,
    note: &str,
) -> Result<()> {
    c.execute(
        "UPDATE import_job SET state = ?2, review = ?3, note = ?4, updated_at = ?5 WHERE id = ?1",
        params![job, state, review, note, now()],
    )?;
    Ok(())
}

/// The checked copy joins the item: same track, movie or book, one more file. For music it is a
/// version of the album; a book's timeline moves to the copy, because a book has one set of files.
pub fn add_copy(c: &mut Connection, w: &Work, source: i64, size: i64, mtime: i64) -> Result<i64> {
    let tx = c.transaction()?;
    let (fingerprint, probe): (Option<Vec<u8>>, Option<String>) = tx.query_row(
        "SELECT fingerprint, probe_json FROM file WHERE id = ?1",
        [w.file],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    // Tags change the file, so the old fingerprint and sizes only fit an unchanged copy.
    let same: bool = tx.query_row(
        "SELECT coalesce(size, -1) = ?2 FROM file WHERE id = ?1",
        params![w.file, size],
        |r| r.get(0),
    )?;
    let version = if w.kind == "music" {
        let key = format!("v:{source}:import:{}", w.job);
        let found: Option<i64> = tx
            .query_row("SELECT id FROM album_version WHERE key = ?1", [&key], |r| {
                r.get(0)
            })
            .optional()?;
        Some(match found {
            Some(v) => v,
            None => {
                tx.execute(
                    "INSERT INTO album_version(album_id, key, source_id, title, year, release_id, release_group)
                     SELECT album_id, ?2, ?3, title, year, release_id, release_group FROM album_version WHERE id = ?1",
                    params![w.version, key, source],
                )?;
                tx.last_insert_rowid()
            }
        })
    } else {
        None
    };
    let rel = w.dest.clone();
    tx.execute(
        "INSERT INTO file(source_id, path, size, mtime, fingerprint, probe_json, item_id, version_id, last_seen_scan)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(source_id, path) DO UPDATE SET size = excluded.size, mtime = excluded.mtime,
            item_id = excluded.item_id, version_id = excluded.version_id",
        params![source, rel, size, mtime, same.then_some(fingerprint).flatten(), probe, w.item, version, now() * 1000],
    )?;
    let id: i64 = tx.query_row(
        "SELECT id FROM file WHERE source_id = ?1 AND path = ?2",
        params![source, rel],
        |r| r.get(0),
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO file_audio(file_id, codec, sample_rate, bit_depth, channels, lossless, bitrate, duration_ms)
         SELECT ?1, codec, sample_rate, bit_depth, channels, lossless, bitrate, duration_ms FROM file_audio WHERE file_id = ?2",
        params![id, w.file],
    )?;
    if w.kind == "audiobooks" {
        tx.execute(
            "UPDATE book_file SET file_id = ?1 WHERE file_id = ?2",
            params![id, w.file],
        )?;
        tx.execute(
            "UPDATE chapter SET file_id = ?1 WHERE file_id = ?2",
            params![id, w.file],
        )?;
        let key: Option<String> = tx
            .query_row("SELECT key FROM book WHERE item_id = ?1", [w.item], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(k) = key {
            crate::copies::set_setting(
                &tx,
                &format!("book_imported:{k}"),
                Some(&w.item.to_string()),
            )?;
        }
    }
    tx.execute(
        "UPDATE import_file SET state = 'done' WHERE id = ?1",
        [w.row],
    )?;
    tx.commit()?;
    Ok(id)
}

/// A book whose files the library already holds is not read again from its old place.
pub fn book_is_imported(c: &Connection, key: &str) -> Result<bool> {
    Ok(crate::copies::setting(c, &format!("book_imported:{key}"))?.is_some())
}

/// True when these paths of a source already are a book's copies under another key.
pub fn files_belong_to_book(c: &Connection, source: i64, paths: &[String]) -> Result<bool> {
    for p in paths {
        let hit: bool = c.query_row(
            "SELECT EXISTS (SELECT 1 FROM file f JOIN book_file b ON b.file_id = f.id
               JOIN book k ON k.item_id = b.book_id
               WHERE f.source_id = ?1 AND f.path = ?2 AND k.key NOT LIKE 'b:' || ?1 || ':%')",
            params![source, p],
            |r| r.get(0),
        )?;
        if hit {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The Imports list, newest first within its group, and how many wait for the user.
pub fn list(c: &Connection) -> Result<Value> {
    let mut st = c.prepare(
        "SELECT j.id, j.item_id, j.type, j.title, j.state, j.review, j.note, j.automatic, j.folder,
                (SELECT count(*) FROM import_file f WHERE f.job_id = j.id),
                (SELECT count(*) FROM import_file f WHERE f.job_id = j.id AND f.state = 'done'),
                (SELECT coalesce(sum(f.size), 0) FROM import_file f WHERE f.job_id = j.id),
                (SELECT coalesce(sum(f.size), 0) FROM import_file f WHERE f.job_id = j.id AND f.state = 'done')
         FROM import_job j ORDER BY j.updated_at DESC, j.id DESC LIMIT 200",
    )?;
    let jobs: Vec<Value> = st
        .query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "item": r.get::<_, Option<i64>>(1)?,
                "type": r.get::<_, String>(2)?,
                "title": r.get::<_, String>(3)?,
                "state": r.get::<_, String>(4)?,
                "review": r.get::<_, Option<String>>(5)?,
                "note": r.get::<_, String>(6)?,
                "automatic": r.get::<_, bool>(7)?,
                "folder": r.get::<_, String>(8)?,
                "files": r.get::<_, i64>(9)?,
                "filesDone": r.get::<_, i64>(10)?,
                "size": r.get::<_, i64>(11)?,
                "sizeDone": r.get::<_, i64>(12)?,
            }))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let waiting = jobs.iter().filter(|j| j["state"] == "review").count();
    let active = jobs.iter().filter(|j| j["state"] == "queued").count();
    Ok(json!({ "jobs": jobs, "waiting": waiting, "active": active }))
}

// ---- review (issue #155) --------------------------------------------------------------------

fn format_of(c: &Connection, file: i64) -> Result<String> {
    let audio: Option<(Option<String>, Option<i64>, Option<i64>)> = c
        .query_row(
            "SELECT codec, sample_rate, bit_depth FROM file_audio WHERE file_id = ?1",
            [file],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    if let Some((Some(codec), rate, depth)) = audio {
        let khz = rate.filter(|r| *r > 0).map(|r| {
            if r % 1000 == 0 {
                (r / 1000).to_string()
            } else {
                format!("{:.1}", r as f64 / 1000.0)
            }
        });
        return Ok(match (depth.filter(|d| *d > 0), khz) {
            (Some(d), Some(k)) => format!("{} {d}/{k}", codec.to_uppercase()),
            (_, Some(k)) => format!("{} {k} kHz", codec.to_uppercase()),
            _ => codec.to_uppercase(),
        });
    }
    let probe: Option<String> = c
        .query_row("SELECT probe_json FROM file WHERE id = ?1", [file], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    let t = video_tech(probe.as_deref());
    Ok([t.resolution.as_str(), t.hdr.as_str(), t.vcodec.as_str()]
        .iter()
        .filter(|x| !x.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" "))
}

/// The label of a second copy's folder: `24-96`, `mp3`, `1080p`.
fn quality_label(c: &Connection, file: i64) -> Result<String> {
    let f = format_of(c, file)?;
    let (codec, rest) = f.split_once(' ').unwrap_or((f.as_str(), ""));
    Ok(if let Some((d, k)) = rest.split_once('/') {
        format!("{d}-{k}")
    } else if rest.ends_with("kHz") {
        codec.to_lowercase()
    } else {
        f.split(' ').next().unwrap_or("").to_string()
    })
}

/// (row, source file, item, destination, size)
type JobFile = (i64, Option<i64>, i64, String, i64);

fn job_files(c: &Connection, job: i64) -> Result<Vec<JobFile>> {
    let mut st = c.prepare(
        "SELECT id, file_id, coalesce(item_id, 0), dest, size FROM import_file
         WHERE job_id = ?1 AND state = 'pending' ORDER BY id",
    )?;
    let rows = st.query_map([job], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The library's copies of a job's items: (file, source, path).
pub fn library_copies(c: &Connection, job: i64) -> Result<Vec<(i64, i64, String)>> {
    let mut st = c.prepare(
        "SELECT DISTINCT f.id, f.source_id, f.path FROM file f JOIN source s ON s.id = f.source_id
         WHERE s.managed IS NOT NULL AND f.item_id IN (SELECT item_id FROM import_file WHERE job_id = ?1)
         ORDER BY f.path",
    )?;
    let rows = st.query_map([job], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// What the review screens show of a waiting import.
pub fn review_info(c: &Connection, job: i64) -> Result<Value> {
    let (item, kind, title, review, folder): (Option<i64>, String, String, Option<String>, String) =
        c.query_row(
            "SELECT item_id, type, title, review, folder FROM import_job WHERE id = ?1",
            [job],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
    let files = job_files(c, job)?;
    let size: i64 = files.iter().map(|f| f.4).sum();
    let first = files.iter().find_map(|f| f.1);
    let (format, from) = match first {
        Some(f) => {
            let p: String = c.query_row(
                "SELECT s.name || ' › ' || f.path FROM file f JOIN source s ON s.id = f.source_id WHERE f.id = ?1",
                [f],
                |r| r.get(0),
            )?;
            (
                format_of(c, f)?,
                p.rsplit_once('/').map_or(p.clone(), |(d, _)| d.to_string()),
            )
        }
        None => (String::new(), String::new()),
    };
    let held = library_copies(c, job)?;
    let library = match held.first() {
        Some((f, _, path)) => json!({
            "files": held.len(),
            "format": format_of(c, *f)?,
            "folder": path.rsplit_once('/').map_or("", |(d, _)| d),
        }),
        None => Value::Null,
    };
    let label = match first {
        Some(f) => quality_label(c, f)?,
        None => String::new(),
    };
    let (item_kind, year, artist): (Option<String>, Option<i64>, String) = match item {
        Some(i) => {
            let (k, y) = c
                .query_row("SELECT kind, year FROM item WHERE id = ?1", [i], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()?
                .unwrap_or((String::new(), None));
            (Some(k), y, first_credit(c, i, "album_artist")?)
        }
        None => (None, None, String::new()),
    };
    Ok(json!({
        "job": job, "item": item, "itemKind": item_kind, "type": kind, "title": title, "year": year,
        "artist": artist, "review": review, "folder": folder, "files": files.len(), "size": size,
        "format": format, "from": from, "library": library, "label": label,
    }))
}

/// A waiting job's plan again, after a match changed: its files, or what the match now says.
pub fn replan(c: &mut Connection, job: i64) -> Result<()> {
    let (item, state): (Option<i64>, String) = c.query_row(
        "SELECT item_id, state FROM import_job WHERE id = ?1",
        [job],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let Some(item) = item else { return Ok(()) };
    if state != "review" {
        return Ok(());
    }
    let wanted: BTreeSet<i64> = job_files(c, job)?.into_iter().map(|f| f.2).collect();
    let Some(mut p) = plan(c, item)?.into_iter().find(|p| p.item == item) else {
        return Ok(());
    };
    p.files.retain(|f| wanted.contains(&f.item));
    let tx = c.transaction()?;
    tx.execute(
        "DELETE FROM import_file WHERE job_id = ?1 AND state = 'pending'",
        [job],
    )?;
    for f in &p.files {
        tx.execute(
            "INSERT OR IGNORE INTO import_file(job_id, file_id, item_id, dest, size) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![job, f.file, f.item, f.dest, f.size],
        )?;
    }
    let (st, note) = if p.files.is_empty() {
        ("done", "already")
    } else if p.review.is_some() {
        ("review", "")
    } else {
        ("queued", "")
    };
    tx.execute(
        "UPDATE import_job SET state = ?2, review = ?3, note = ?4, title = ?5, folder = ?6, version_id = ?7, updated_at = ?8 WHERE id = ?1",
        params![job, st, p.review.as_deref().filter(|_| st == "review"), note, p.title, p.folder, p.version, now()],
    )?;
    tx.commit()?;
    Ok(())
}

fn any_in_folder(c: &Connection, kind: &str, rel: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS (SELECT 1 FROM file f JOIN source s ON s.id = f.source_id
           WHERE s.managed = ?1 AND substr(f.path, 1, length(?2) + 1) = ?2 || '/')",
        params![kind, rel],
        |r| r.get(0),
    )?)
}

/// Move a job's pending files into `{folder} [label]`, numbered when that is taken too.
fn relabel(c: &Connection, job: i64, label: &str) -> Result<()> {
    let (kind, folder): (String, String) = c.query_row(
        "SELECT type, folder FROM import_job WHERE id = ?1",
        [job],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let clean = naming::clean(label);
    let mut target = format!("{folder} [{clean}]");
    let mut n = 2;
    while any_in_folder(c, &kind, &target)? {
        target = format!("{folder} [{clean} {n}]");
        n += 1;
    }
    for (row, _, _, dest, _) in job_files(c, job)? {
        let rest = dest
            .strip_prefix(&format!("{folder}/"))
            .unwrap_or(&dest)
            .to_string();
        c.execute(
            "UPDATE import_file SET dest = ?2 WHERE id = ?1",
            params![row, format!("{target}/{rest}")],
        )?;
    }
    c.execute(
        "UPDATE import_job SET folder = ?2 WHERE id = ?1",
        params![job, target],
    )?;
    Ok(())
}

/// The user's answer to a waiting import, without the file work of `replace` (done by the core
/// before it calls this). `choice`: asIs, version, rename (with `label`), replace or keep.
pub fn resolve(c: &Connection, job: i64, choice: &str, label: &str) -> Result<()> {
    match choice {
        "asIs" | "replace" => set_state(c, job, "queued", None, ""),
        "keep" => {
            c.execute(
                "UPDATE import_file SET state = 'skipped' WHERE job_id = ?1 AND state = 'pending'",
                [job],
            )?;
            set_state(c, job, "done", None, "kept")
        }
        "version" | "rename" => {
            let wanted = if label.trim().is_empty() {
                match job_files(c, job)?.into_iter().find_map(|f| f.1) {
                    Some(f) => quality_label(c, f)?,
                    None => String::new(),
                }
            } else {
                label.trim().to_string()
            };
            let wanted = if wanted.is_empty() {
                "2".to_string()
            } else {
                wanted
            };
            relabel(c, job, &wanted)?;
            set_state(c, job, "queued", None, "")
        }
        _ => Ok(()),
    }
}

/// Other clashes waiting, for "Do the same for the next clashes of this import".
pub fn other_clashes(c: &Connection, job: i64) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT id FROM import_job WHERE state = 'review' AND review = 'clash' AND id != ?1 ORDER BY id",
    )?;
    let rows = st.query_map([job], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// After `replace` moved a copy away: its row goes, and an album version left empty with it.
/// The item, its play counts and playlists stay.
pub fn drop_copy(c: &Connection, file: i64) -> Result<()> {
    let version: Option<i64> = c
        .query_row("SELECT version_id FROM file WHERE id = ?1", [file], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    c.execute("DELETE FROM file WHERE id = ?1", [file])?;
    if let Some(v) = version {
        c.execute(
            "DELETE FROM album_version WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM file f WHERE f.version_id = ?1)",
            [v],
        )?;
    }
    Ok(())
}
