//! Library queries for the UI (SPEC 8, rule 3: paged view models sized for one screen) and the
//! writes the metadata jobs make.

use rusqlite::{Connection, OptionalExtension, params};

use crate::queries::{DEFAULT_PROFILE, PlayState};
use crate::{LOCAL_SOURCE_ID, Result};

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// SQL for a file's absolute path: library sources store paths relative to their root.
pub const ABS_PATH_SQL: &str =
    "CASE WHEN s.root IS NULL THEN f.path ELSE s.root || '/' || f.path END";

// ---- library folders ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct LibraryFolder {
    pub id: i64,
    /// `local`, or a network protocol (`smb`, `nfs`, ...).
    pub kind: String,
    pub name: String,
    /// Local: the absolute folder. Network: the folder inside the share ("" = its root).
    pub root: String,
    /// Network sources: how to reach them, without the password.
    pub config_json: String,
    pub files: i64,
    pub last_scan: Option<i64>,
    /// `video` or `music` (2.0, SPEC 6 "Content type").
    pub content: String,
}

pub fn libraries(c: &Connection) -> Result<Vec<LibraryFolder>> {
    let mut st = c.prepare(
        "SELECT s.id, s.name, s.root, (SELECT count(*) FROM file f WHERE f.source_id = s.id), s.last_scan,
                s.kind, s.config_json, s.content
         FROM source s WHERE s.root IS NOT NULL ORDER BY s.name",
    )?;
    let rows = st.query_map([], |r| {
        Ok(LibraryFolder {
            id: r.get(0)?,
            name: r.get(1)?,
            root: r.get(2)?,
            files: r.get(3)?,
            last_scan: r.get(4)?,
            kind: r.get(5)?,
            config_json: r.get(6)?,
            content: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Add a library folder with its content type (`video` or `music`). Returns the existing id if
/// the folder is already a library (its content type stays).
pub fn add_library(c: &Connection, name: &str, root: &str, content: &str) -> Result<i64> {
    let root = root.trim_end_matches('/');
    if let Some(id) = c
        .query_row(
            "SELECT id FROM source WHERE kind = 'local' AND root = ?1",
            [root],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    c.execute(
        "INSERT INTO source(kind, name, root, content) VALUES ('local', ?1, ?2, ?3)",
        params![name, root, content],
    )?;
    Ok(c.last_insert_rowid())
}

/// Add a network library: `kind` is the protocol, `root` the folder inside the share.
/// Returns the existing id if the same place is already a library.
pub fn add_network_library(
    c: &Connection,
    kind: &str,
    name: &str,
    config_json: &str,
    root: &str,
    content: &str,
) -> Result<i64> {
    let root = root.trim_end_matches('/');
    if let Some(id) = c
        .query_row(
            "SELECT id FROM source WHERE kind = ?1 AND config_json = ?2 AND root = ?3",
            params![kind, config_json, root],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    c.execute(
        "INSERT INTO source(kind, name, config_json, root, content) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![kind, name, config_json, root, content],
    )?;
    let id = c.last_insert_rowid();
    c.execute(
        "UPDATE source SET credential_ref = ?2 WHERE id = ?1",
        params![id, format!("source:{id}")],
    )?;
    Ok(id)
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceRow {
    pub id: i64,
    pub kind: String,
    pub config_json: String,
    pub credential_ref: Option<String>,
    pub root: Option<String>,
    /// `video` or `music` (2.0).
    pub content: String,
}

pub fn source(c: &Connection, id: i64) -> Result<Option<SourceRow>> {
    Ok(c.query_row(
        "SELECT id, kind, config_json, credential_ref, root, content FROM source WHERE id = ?1",
        [id],
        |r| {
            Ok(SourceRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                config_json: r.get(2)?,
                credential_ref: r.get(3)?,
                root: r.get(4)?,
                content: r.get(5)?,
            })
        },
    )
    .optional()?)
}

/// Store a network source's updated configuration (an SFTP host key seen for the first time).
pub fn set_source_config(c: &Connection, id: i64, config_json: &str) -> Result<()> {
    c.execute(
        "UPDATE source SET config_json = ?2 WHERE id = ?1",
        params![id, config_json],
    )?;
    Ok(())
}

/// The item of a file given by its path inside a source.
pub fn item_for_source_path(c: &Connection, source_id: i64, abs_path: &str) -> Result<Option<i64>> {
    let root: Option<String> = c
        .query_row("SELECT root FROM source WHERE id = ?1", [source_id], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    let rel = match &root {
        Some(r) => match abs_path.strip_prefix(&format!("{r}/")) {
            Some(rel) => rel,
            None => return Ok(None),
        },
        None => abs_path,
    };
    Ok(c.query_row(
        "SELECT item_id FROM file WHERE source_id = ?1 AND path = ?2",
        params![source_id, rel],
        |r| r.get(0),
    )
    .optional()?
    .flatten())
}

/// Remove a library folder and everything only it provided. Play state of items that disappear
/// goes with them.
pub fn remove_library(c: &mut Connection, id: i64) -> Result<()> {
    let tx = c.transaction()?;
    // Queued scans of this folder must not run against a later folder that reuses the id.
    tx.execute(
        "DELETE FROM job WHERE kind = 'scan' AND json_extract(payload_json, '$.source_id') = ?1",
        [id],
    )?;
    tx.execute("DELETE FROM file WHERE source_id = ?1", [id])?;
    tx.execute(
        "DELETE FROM source WHERE id = ?1 AND id != ?2",
        params![id, LOCAL_SOURCE_ID],
    )?;
    tx.execute_batch(
        "DELETE FROM item WHERE kind IN ('movie','episode','video') AND NOT EXISTS (SELECT 1 FROM file f WHERE f.item_id = item.id);
         DELETE FROM item WHERE kind = 'season' AND NOT EXISTS (SELECT 1 FROM item e WHERE e.parent_id = item.id);
         DELETE FROM item WHERE kind = 'show' AND NOT EXISTS (SELECT 1 FROM item s WHERE s.parent_id = item.id);
         DELETE FROM item WHERE kind = 'track' AND NOT EXISTS (SELECT 1 FROM file f WHERE f.item_id = item.id);
         DELETE FROM item WHERE kind = 'album' AND NOT EXISTS (SELECT 1 FROM item t WHERE t.parent_id = item.id);
         DELETE FROM item WHERE kind = 'artist' AND NOT EXISTS (SELECT 1 FROM item a WHERE a.parent_id = item.id)
           AND NOT EXISTS (SELECT 1 FROM credit c WHERE c.artist_id = item.id);
         DELETE FROM item_fts WHERE rowid NOT IN (SELECT id FROM item);",
    )?;
    tx.commit()?;
    Ok(())
}

/// If `abs_path` lies inside a library folder, the file's item (so the folder browser and the
/// library share one play state).
pub fn library_item_for_path(c: &Connection, abs_path: &str) -> Result<Option<i64>> {
    let roots: Vec<(i64, String)> = {
        let mut st =
            c.prepare("SELECT id, root FROM source WHERE kind = 'local' AND root IS NOT NULL")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (id, root) in roots {
        if let Some(rel) = abs_path.strip_prefix(&format!("{root}/")) {
            let item = c
                .query_row(
                    "SELECT item_id FROM file WHERE source_id = ?1 AND path = ?2",
                    params![id, rel],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            if item.is_some() {
                return Ok(item);
            }
        }
    }
    Ok(None)
}

// ---- view models -----------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub year: Option<i64>,
    pub season_no: Option<i64>,
    pub episode_no: Option<i64>,
    pub poster: String,
    /// The 540 px grid tier (`poster_grid_l`); empty until it exists, then QML may prefer it.
    pub poster_large: String,
    pub progress: f32,
    pub watched: bool,
    /// Shows and seasons: episodes not yet watched.
    pub unwatched: i64,
    pub overview: String,
    /// Episodes: a file is in the library. Seasons: at least one episode is. Episodes and
    /// seasons only TMDB knows are shown dimmed and cannot be played.
    pub on_disk: bool,
    pub runtime_min: Option<u32>,
    /// Issue #4: a complete saved copy of it (or of one of its episodes) is on this computer.
    pub saved: bool,
    /// Music (2.0): an album's artist, a track's album; "" otherwise.
    pub subtitle: String,
    pub parent_id: Option<i64>,
    /// Issue #63: how many versions an album has (0 for everything else).
    pub versions: i64,
    /// Issue #66: a book cover's height / width (0 when unknown or not a book).
    pub ratio: f64,
    /// Issue #78: albums and tracks: the best version's format ("FLAC 24/96"), "" otherwise.
    pub format: String,
    pub hires: bool,
    /// Issue #79: artists: their albums, and how many of them are Hi-Res.
    pub albums: i64,
    pub hires_albums: i64,
    /// Issue #80: tracks: the artists' names, the length of the best file, the play count.
    pub artist: String,
    pub duration_ms: i64,
    pub plays: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Title,
    Added,
    Year,
    /// Issue #8: music played most recently first; only what was played (albums by their
    /// tracks' plays).
    Played,
    /// Issue #78: music by play count, most played first (albums by their tracks' plays).
    Plays,
    /// Issue #80: songs by artist, by album, by length, by quality (best first).
    Artist,
    Album,
    Duration,
    Quality,
}

/// Issue #78: the Quality lens and the music filters. Lenses combine (all must hold); an
/// album matches a lens when one of its versions does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MusicFilter {
    pub hires: bool,
    pub lossless: bool,
    /// Albums with more than one version (#63).
    pub multi: bool,
    /// Songs never played (#80).
    pub unplayed: bool,
    /// Issue #79: artists: everyone credited on a track, not only album artists.
    pub credited: bool,
    pub genre: Option<String>,
    /// The first year of a decade (1990 for the nineties).
    pub decade: Option<i64>,
}

impl MusicFilter {
    pub fn is_empty(&self) -> bool {
        *self == MusicFilter::default()
    }
}

/// A string as an SQL literal.
fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The files that belong to `i`: a track's own, an album's tracks'.
const ITEM_FILES: &str = "(SELECT f.id FROM file f JOIN item t ON t.id = f.item_id
    WHERE (i.kind = 'album' AND t.parent_id = i.id) OR (i.kind = 'track' AND t.id = i.id))";

/// The SQL conditions (each starting with " AND ") for a music filter on items `i`.
pub fn music_where(m: &MusicFilter) -> String {
    let mut out = String::new();
    if m.hires {
        out.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM file_audio fa WHERE fa.file_id IN {ITEM_FILES}
                AND fa.lossless = 1 AND (coalesce(fa.bit_depth, 0) > 16 OR coalesce(fa.sample_rate, 0) > 48000))"
        ));
    }
    if m.lossless {
        out.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM file_audio fa WHERE fa.file_id IN {ITEM_FILES} AND fa.lossless = 1)"
        ));
    }
    if m.multi {
        out.push_str(" AND (SELECT count(*) FROM album_version v WHERE v.album_id = i.id) > 1");
    }
    if m.unplayed {
        out.push_str(" AND coalesce(p.play_count, 0) = 0");
    }
    if let Some(g) = &m.genre {
        out.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM music mg WHERE mg.item_id = (CASE WHEN i.kind = 'track' THEN i.parent_id ELSE i.id END) AND mg.genre = {})",
            quoted(g)
        ));
    }
    if let Some(d) = m.decade {
        out.push_str(&format!(" AND i.year BETWEEN {d} AND {}", d + 9));
    }
    out
}

/// Issue #79: artists with an album (album artists), unless `credited` asks for everyone with a
/// credit; `hires`: with at least one Hi-Res album.
fn artist_where(m: &MusicFilter) -> String {
    let mut out = String::new();
    if !m.credited {
        out.push_str(
            " AND EXISTS (SELECT 1 FROM item a WHERE a.parent_id = i.id AND a.kind = 'album')",
        );
    }
    if m.hires {
        out.push_str(
            " AND EXISTS (SELECT 1 FROM item a JOIN item t ON t.parent_id = a.id JOIN file f ON f.item_id = t.id
                JOIN file_audio fa ON fa.file_id = f.id
                WHERE a.parent_id = i.id AND fa.lossless = 1 AND (coalesce(fa.bit_depth, 0) > 16 OR coalesce(fa.sample_rate, 0) > 48000))",
        );
    }
    out
}

/// The SQL conditions of a music filter for a kind of item.
pub fn filter_where(kind: &str, m: &MusicFilter) -> String {
    if kind == "artist" {
        artist_where(m)
    } else {
        music_where(m)
    }
}

/// Issue #78: "FLAC 24/96", "MP3 320k": a file's format for the cover ribbon and the Songs
/// list, from `codec|depth|rate|kbps|lossless` as SUMMARY_COLS selects it.
pub fn format_label(raw: &str) -> (String, bool) {
    let p: Vec<&str> = raw.split('|').collect();
    if p.len() < 5 || p[0].is_empty() {
        return (String::new(), false);
    }
    let codec = match p[0].to_ascii_lowercase().as_str() {
        "flac" => "FLAC".to_string(),
        "alac" => "ALAC".to_string(),
        "mp3" | "mp3float" => "MP3".to_string(),
        "aac" => "AAC".to_string(),
        "vorbis" => "Ogg".to_string(),
        "opus" => "Opus".to_string(),
        "wavpack" => "WavPack".to_string(),
        c if c.starts_with("pcm") => "PCM".to_string(),
        c if c.starts_with("dsd") => "DSD".to_string(),
        c => c.to_uppercase(),
    };
    let depth: i64 = p[1].parse().unwrap_or(0);
    let rate: i64 = p[2].parse().unwrap_or(0);
    let kbps: i64 = p[3].parse().unwrap_or(0);
    let lossless = p[4] == "1";
    let hires = crate::music::hires(lossless, Some(depth), Some(rate));
    let label = if codec == "DSD" {
        "DSD".to_string()
    } else if lossless && rate > 0 {
        let k = rate / 1000;
        if depth > 0 {
            format!("{codec} {depth}/{k}")
        } else {
            format!("{codec} {k} kHz")
        }
    } else if kbps > 0 {
        format!("{codec} {kbps}k")
    } else {
        codec
    };
    (label, hires)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    /// "movie", "show", or a parent's children when `parent` is set.
    pub kind: String,
    pub parent: Option<i64>,
    pub sort: Sort,
    pub unwatched_only: bool,
    /// Issue #4: only items with a saved copy.
    pub saved_only: bool,
    /// Issue #66: books "unread", "in_progress" or "finished"; persons "author" or "narrator";
    /// "" for all.
    pub book_filter: &'static str,
    /// Issue #78: the music filters.
    pub music: MusicFilter,
    pub offset: u32,
    pub limit: u32,
}

/// Issue #4: a complete saved copy of the item, one of its episodes or one of its seasons'.
macro_rules! saved_sql {
    () => {
        "EXISTS (SELECT 1 FROM local_copy lc JOIN item x ON x.id = lc.item_id LEFT JOIN item y ON y.id = x.parent_id
           WHERE lc.kind = 'saved' AND lc.main_id IS NULL AND lc.state = 'done'
             AND (x.id = i.id OR x.parent_id = i.id OR y.parent_id = i.id))"
    };
}

const SUMMARY_COLS: &str = concat!("i.id, i.kind, i.title, i.year, i.season_no, i.episode_no,
    (SELECT local_path FROM artwork a WHERE a.item_id = i.id AND a.role IN ('poster_grid','still') AND local_path IS NOT NULL ORDER BY a.role LIMIT 1),
    p.position_ms, p.duration_ms, p.watched,
    CASE WHEN i.kind = 'show' THEN (SELECT count(*) FROM item s JOIN item e ON e.parent_id = s.id
           LEFT JOIN playstate pe ON pe.item_id = e.id AND pe.profile_id = 1
           WHERE s.parent_id = i.id AND s.season_no > 0 AND coalesce(pe.watched, 0) = 0
             AND EXISTS (SELECT 1 FROM file f WHERE f.item_id = e.id))
         WHEN i.kind = 'season' THEN (SELECT count(*) FROM item e
           LEFT JOIN playstate pe ON pe.item_id = e.id AND pe.profile_id = 1
           WHERE e.parent_id = i.id AND coalesce(pe.watched, 0) = 0
             AND EXISTS (SELECT 1 FROM file f WHERE f.item_id = e.id))
         ELSE 0 END,
    coalesce(i.overview, ''),
    (SELECT local_path FROM artwork a WHERE a.item_id = i.id AND a.role = 'poster_grid_l' AND local_path IS NOT NULL LIMIT 1),
    CASE i.kind
      WHEN 'show' THEN 1
      WHEN 'season' THEN EXISTS (SELECT 1 FROM item e JOIN file f ON f.item_id = e.id WHERE e.parent_id = i.id)
      ELSE EXISTS (SELECT 1 FROM file f WHERE f.item_id = i.id) END,
    i.runtime_min, ", saved_sql!(), ",
    CASE WHEN i.kind IN ('album','track') THEN (SELECT p2.title FROM item p2 WHERE p2.id = i.parent_id)
         WHEN i.kind = 'audiobook' THEN coalesce((SELECT p2.title FROM book_credit bc JOIN item p2 ON p2.id = bc.person_id
           WHERE bc.book_id = i.id AND bc.role = 'author' ORDER BY bc.position LIMIT 1), '')
         ELSE '' END,
    i.parent_id,
    CASE WHEN i.kind = 'album' THEN (SELECT count(*) FROM album_version v WHERE v.album_id = i.id) ELSE 0 END,
    CASE WHEN i.kind = 'audiobook' THEN (SELECT b.cover_ratio FROM book b WHERE b.item_id = i.id) END,
    CASE WHEN i.kind IN ('album','track') THEN (SELECT coalesce(fa.codec, '') || '|' || coalesce(fa.bit_depth, 0) || '|' || coalesce(fa.sample_rate, 0)
           || '|' || coalesce(fa.bitrate, 0) || '|' || coalesce(fa.lossless, 0)
         FROM file_audio fa WHERE fa.file_id IN (SELECT f.id FROM file f JOIN item t ON t.id = f.item_id
           WHERE (i.kind = 'album' AND t.parent_id = i.id) OR (i.kind = 'track' AND t.id = i.id))
         ORDER BY fa.lossless DESC, coalesce(fa.bit_depth, 0) DESC, coalesce(fa.sample_rate, 0) DESC, coalesce(fa.bitrate, 0) DESC LIMIT 1) END,
    CASE WHEN i.kind = 'artist' THEN (SELECT count(*) FROM item a WHERE a.parent_id = i.id AND a.kind = 'album') ELSE 0 END,
    CASE WHEN i.kind = 'artist' THEN (SELECT count(*) FROM item a WHERE a.parent_id = i.id AND a.kind = 'album'
      AND EXISTS (SELECT 1 FROM item t JOIN file f ON f.item_id = t.id JOIN file_audio fa ON fa.file_id = f.id
        WHERE t.parent_id = a.id AND fa.lossless = 1 AND (coalesce(fa.bit_depth, 0) > 16 OR coalesce(fa.sample_rate, 0) > 48000))) ELSE 0 END,
    CASE WHEN i.kind = 'track' THEN (SELECT group_concat(ta, ', ') FROM (SELECT a.title AS ta FROM credit c JOIN item a ON a.id = c.artist_id
      WHERE c.item_id = i.id AND c.role = 'artist' ORDER BY c.position)) ELSE '' END,
    CASE WHEN i.kind = 'track' THEN (SELECT max(coalesce(fa.duration_ms, 0)) FROM file f JOIN file_audio fa ON fa.file_id = f.id WHERE f.item_id = i.id) ELSE 0 END,
    coalesce(p.play_count, 0)");

fn summary(r: &rusqlite::Row) -> rusqlite::Result<Summary> {
    let mut s = summary_base(r)?;
    if let Some(raw) = r.get::<_, Option<String>>(20)? {
        (s.format, s.hires) = format_label(&raw);
    }
    Ok(s)
}

fn summary_base(r: &rusqlite::Row) -> rusqlite::Result<Summary> {
    let st = PlayState {
        position_ms: r.get::<_, Option<i64>>(7)?.unwrap_or(0) as u64,
        duration_ms: r.get::<_, Option<i64>>(8)?.unwrap_or(0) as u64,
        watched: r.get::<_, Option<i64>>(9)?.unwrap_or(0) != 0,
    };
    Ok(Summary {
        id: r.get(0)?,
        kind: r.get(1)?,
        title: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
        year: r.get(3)?,
        season_no: r.get(4)?,
        episode_no: r.get(5)?,
        poster: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
        poster_large: r.get::<_, Option<String>>(12)?.unwrap_or_default(),
        on_disk: r.get(13)?,
        runtime_min: r.get(14)?,
        saved: r.get(15)?,
        subtitle: r.get::<_, Option<String>>(16)?.unwrap_or_default(),
        parent_id: r.get(17)?,
        versions: r.get(18)?,
        ratio: r.get::<_, Option<f64>>(19)?.unwrap_or(0.0),
        format: String::new(),
        hires: false,
        albums: r.get(21)?,
        hires_albums: r.get(22)?,
        artist: r.get::<_, Option<String>>(23)?.unwrap_or_default(),
        duration_ms: r.get::<_, Option<i64>>(24)?.unwrap_or(0),
        plays: r.get::<_, Option<i64>>(25)?.unwrap_or(0),
        progress: st.progress(),
        watched: st.watched,
        unwatched: r.get(10)?,
        overview: r.get(11)?,
    })
}

/// Items that have something playable: movies and episodes with files; shows and seasons with
/// at least one such episode; music (2.0): tracks with a file, albums with such a track, artists
/// with such an album or a credit on such a track.
const VISIBLE: &str = "(
    (i.kind IN ('movie','episode','track') AND EXISTS (SELECT 1 FROM file f WHERE f.item_id = i.id AND f.source_id != 1))
 OR (i.kind = 'album' AND EXISTS (SELECT 1 FROM item t JOIN file f ON f.item_id = t.id WHERE t.parent_id = i.id))
 OR (i.kind = 'artist' AND (EXISTS (SELECT 1 FROM item a JOIN item t ON t.parent_id = a.id JOIN file f ON f.item_id = t.id WHERE a.parent_id = i.id)
                            OR EXISTS (SELECT 1 FROM credit c JOIN file f ON f.item_id = c.item_id WHERE c.artist_id = i.id)))
 OR (i.kind = 'audiobook' AND EXISTS (SELECT 1 FROM book_file bf WHERE bf.book_id = i.id))
 OR (i.kind = 'person' AND EXISTS (SELECT 1 FROM book_credit bc WHERE bc.person_id = i.id))
 OR (i.kind = 'book_series' AND EXISTS (SELECT 1 FROM item b WHERE b.parent_id = i.id AND b.kind = 'audiobook'))
 OR (i.kind = 'season' AND EXISTS (SELECT 1 FROM item e JOIN file f ON f.item_id = e.id WHERE e.parent_id = i.id))
 OR (i.kind = 'show' AND EXISTS (SELECT 1 FROM item s JOIN item e ON e.parent_id = s.id JOIN file f ON f.item_id = e.id WHERE s.parent_id = i.id)))";

/// Issue #78: how often an item was played (an album: its tracks' plays).
const PLAYS: &str =
    "(CASE WHEN i.kind = 'album' THEN (SELECT coalesce(sum(pt.play_count), 0) FROM item t
    JOIN playstate pt ON pt.item_id = t.id AND pt.profile_id = 1 WHERE t.parent_id = i.id)
    ELSE coalesce(p.play_count, 0) END)";

/// Issue #8: when an item was played last (an album: its tracks' last play).
const PLAYED: &str = "(CASE WHEN i.kind = 'album' THEN (SELECT max(pt.last_played_at) FROM item t
    JOIN playstate pt ON pt.item_id = t.id AND pt.profile_id = 1 WHERE t.parent_id = i.id)
    ELSE p.last_played_at END)";

/// Issue #80: a song's quality for sorting, best first: DSD, then lossless by sample rate and
/// bit depth, then lossy by bit rate (the best of its files).
const QUALITY: &str = "(SELECT max(CASE WHEN fa.codec LIKE 'dsd%' THEN 3000000000 + coalesce(fa.sample_rate, 0)
    WHEN fa.lossless = 1 THEN 2000000000 + coalesce(fa.sample_rate, 0) * 100 + coalesce(fa.bit_depth, 0)
    ELSE coalesce(fa.bitrate, 0) END) FROM file f JOIN file_audio fa ON fa.file_id = f.id WHERE f.item_id = i.id)";
const ARTIST_NAME: &str = "(SELECT a.sort_title FROM credit c JOIN item a ON a.id = c.artist_id
    WHERE c.item_id = i.id AND c.role = 'artist' ORDER BY c.position LIMIT 1)";
const ALBUM_NAME: &str = "(SELECT al.sort_title FROM item al WHERE al.id = i.parent_id)";
const DURATION: &str = "(SELECT max(coalesce(fa.duration_ms, 0)) FROM file f JOIN file_audio fa ON fa.file_id = f.id WHERE f.item_id = i.id)";

/// The FROM and WHERE of a browse query, and its ORDER BY.
fn browse_sql(q: &Query) -> (String, String) {
    let filter = match q.parent {
        Some(_) => "i.parent_id = ?1".to_string(),
        None if q.kind == "album" || q.kind == "track" || q.kind == "audiobook" => {
            "i.kind = ?1".to_string()
        }
        None => "i.kind = ?1 AND i.parent_id IS NULL".to_string(),
    };
    let unwatched = if q.unwatched_only {
        " AND coalesce(p.watched, 0) = 0"
    } else {
        ""
    };
    let saved = if q.saved_only {
        concat!(" AND ", saved_sql!())
    } else {
        ""
    };
    let book = match q.book_filter {
        "unread" => " AND coalesce(p.position_ms, 0) = 0 AND coalesce(p.watched, 0) = 0",
        "in_progress" => " AND coalesce(p.position_ms, 0) > 0 AND coalesce(p.watched, 0) = 0",
        "finished" => " AND coalesce(p.watched, 0) = 1",
        "author" => {
            " AND EXISTS (SELECT 1 FROM book_credit bc WHERE bc.person_id = i.id AND bc.role = 'author')"
        }
        "narrator" => {
            " AND EXISTS (SELECT 1 FROM book_credit bc WHERE bc.person_id = i.id AND bc.role = 'narrator')"
        }
        _ => "",
    };
    let music = filter_where(&q.kind, &q.music);
    let played = if q.sort == Sort::Played && q.parent.is_none() {
        format!(" AND {PLAYED} IS NOT NULL")
    } else {
        String::new()
    };
    let order = match (q.parent.is_some(), q.sort) {
        (true, _) => {
            "coalesce(i.season_no, 0) = 0, i.season_no, i.episode_no, i.sort_title".to_string()
        }
        (false, Sort::Title) => "i.sort_title, i.year".to_string(),
        (false, Sort::Added) => "i.added_at DESC, i.sort_title".to_string(),
        (false, Sort::Year) => "i.year DESC, i.sort_title".to_string(),
        (false, Sort::Played) => format!("{PLAYED} DESC, i.sort_title"),
        (false, Sort::Plays) => format!("{PLAYS} DESC, i.sort_title"),
        (false, Sort::Artist) => format!("{ARTIST_NAME}, {ALBUM_NAME}, i.sort_title"),
        (false, Sort::Album) => format!("{ALBUM_NAME}, i.id"),
        (false, Sort::Duration) => format!("{DURATION} DESC, i.sort_title"),
        (false, Sort::Quality) => format!("{QUALITY} DESC, i.sort_title"),
    };
    let from = format!(
        "FROM item i LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE}
         WHERE {filter} AND {VISIBLE}{unwatched}{saved}{book}{music}{played}"
    );
    (from, order)
}

/// Issue #80: the ids a browse query lists, in its order (at most `cap`), for playing them.
pub fn browse_ids(c: &Connection, q: &Query, cap: u32) -> Result<Vec<i64>> {
    let (from, order) = browse_sql(q);
    let key: rusqlite::types::Value = match q.parent {
        Some(p) => p.into(),
        None => q.kind.clone().into(),
    };
    let mut st = c.prepare(&format!("SELECT i.id {from} ORDER BY {order} LIMIT ?2"))?;
    let rows = st.query_map(params![key, cap], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// One page of items plus the total count.
pub fn browse(c: &Connection, q: &Query) -> Result<(u32, Vec<Summary>)> {
    let (from, order) = browse_sql(q);
    let key: rusqlite::types::Value = match q.parent {
        Some(p) => p.into(),
        None => q.kind.clone().into(),
    };
    let total: u32 = c.query_row(&format!("SELECT count(*) {from}"), [&key], |r| r.get(0))?;
    let mut st = c.prepare(&format!(
        "SELECT {SUMMARY_COLS} {from} ORDER BY {order} LIMIT ?2 OFFSET ?3"
    ))?;
    let rows = st.query_map(params![key, q.limit, q.offset], summary)?;
    Ok((total, rows.collect::<rusqlite::Result<_>>()?))
}

/// Issue #78: what the Quality lens and the music filters offer for a kind ("album" or
/// "track"): how many items each lens would show (within the chosen genre and decade), the
/// genres and decades (within the other filters), and where each initial starts in the A–Z
/// order of the current filter (for the letter rail).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Facets {
    /// Everything within the chosen genre and decade, no lens.
    pub all: u32,
    /// With every filter.
    pub total: u32,
    pub hires: u32,
    pub lossless: u32,
    pub multi: u32,
    pub saved: u32,
    pub unplayed: u32,
    pub genres: Vec<(String, u32)>,
    pub decades: Vec<(i64, u32)>,
    /// (initial, offset of its first item in the A–Z order); "#" for digits and signs.
    pub letters: Vec<(String, u32)>,
}

pub fn facets(c: &Connection, kind: &str, music: &MusicFilter, saved_only: bool) -> Result<Facets> {
    let k = quoted(kind);
    let from = format!(
        "FROM item i LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE}
         WHERE i.kind = {k} AND {VISIBLE}"
    );
    let saved = |on: bool| {
        if on {
            concat!(" AND ", saved_sql!())
        } else {
            ""
        }
    };
    let count = |extra: &str| -> Result<u32> {
        Ok(c.query_row(&format!("SELECT count(*) {from}{extra}"), [], |r| r.get(0))?)
    };
    let ctx = MusicFilter {
        genre: music.genre.clone(),
        decade: music.decade,
        ..Default::default()
    };
    let artists = kind == "artist";
    let base = if artists {
        filter_where(
            kind,
            &MusicFilter {
                credited: music.credited,
                ..Default::default()
            },
        )
    } else {
        music_where(&ctx)
    };
    let lens = |f: MusicFilter| {
        filter_where(
            kind,
            &MusicFilter {
                genre: ctx.genre.clone(),
                decade: ctx.decade,
                credited: music.credited,
                ..f
            },
        )
    };
    let full = format!("{}{}", filter_where(kind, music), saved(saved_only));
    let mut out = Facets {
        all: count(&base)?,
        total: count(&full)?,
        hires: count(&lens(MusicFilter {
            hires: true,
            ..Default::default()
        }))?,
        lossless: count(&lens(MusicFilter {
            lossless: true,
            ..Default::default()
        }))?,
        multi: if kind == "album" {
            count(&lens(MusicFilter {
                multi: true,
                ..Default::default()
            }))?
        } else {
            0
        },
        saved: count(&format!("{base}{}", saved(true)))?,
        unplayed: count(&lens(MusicFilter {
            unplayed: true,
            ..Default::default()
        }))?,
        ..Default::default()
    };
    if artists {
        out.letters = letters(c, &from, &full)?;
        return Ok(out);
    }
    // Genres within the other filters.
    let no_genre = format!(
        "{}{}",
        music_where(&MusicFilter {
            genre: None,
            ..music.clone()
        }),
        saved(saved_only)
    );
    let mut st = c.prepare(&format!(
        "SELECT mg.genre, count(*) {from}{no_genre}
         AND mg.genre IS NOT NULL AND mg.genre <> ''
         GROUP BY mg.genre ORDER BY lower(mg.genre)"
    ).replacen(
        "FROM item i LEFT JOIN playstate p",
        "FROM item i JOIN music mg ON mg.item_id = (CASE WHEN i.kind = 'track' THEN i.parent_id ELSE i.id END) LEFT JOIN playstate p",
        1,
    ))?;
    out.genres = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let no_decade = format!(
        "{}{}",
        music_where(&MusicFilter {
            decade: None,
            ..music.clone()
        }),
        saved(saved_only)
    );
    let mut st = c.prepare(&format!(
        "SELECT (i.year / 10) * 10 AS d, count(*) {from}{no_decade} AND coalesce(i.year, 0) > 0
         GROUP BY d ORDER BY d DESC"
    ))?;
    out.decades = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    out.letters = letters(c, &from, &full)?;
    Ok(out)
}

/// Initials in the A–Z order (`i.sort_title`, as browse sorts by title) with the offset of
/// each one's first item.
fn letters(c: &Connection, from: &str, filter: &str) -> Result<Vec<(String, u32)>> {
    let mut st = c.prepare(&format!(
        "SELECT substr(coalesce(i.sort_title, ''), 1, 1) AS l, count(*) {from}{filter}
         GROUP BY l ORDER BY l"
    ))?;
    let groups: Vec<(String, u32)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut out: Vec<(String, u32)> = Vec::new();
    let mut offset = 0;
    for (l, n) in groups {
        let label = match l.chars().next() {
            Some(ch) if ch.is_alphabetic() => ch.to_uppercase().collect::<String>(),
            _ => "#".to_string(),
        };
        if !out.iter().any(|(x, _)| *x == label) {
            out.push((label, offset));
        }
        offset += n;
    }
    Ok(out)
}

/// Full-text search over movies, shows and music (artists, albums, tracks); every word is a
/// prefix. `kind` limits it to one kind ("" for all; issue #10 groups results by kind).
pub fn search(c: &Connection, text: &str, kind: &str, limit: u32) -> Result<Vec<Summary>> {
    let words: Vec<String> = text
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .collect();
    if words.is_empty() {
        return Ok(vec![]);
    }
    let fts = words.join(" ");
    let mut st = c.prepare(&format!(
        "SELECT {SUMMARY_COLS} FROM item_fts JOIN item i ON i.id = item_fts.rowid
         LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE}
         WHERE item_fts MATCH ?1 AND {VISIBLE} AND (?3 = '' OR i.kind = ?3) ORDER BY rank LIMIT ?2"
    ))?;
    let rows = st.query_map(params![fts, limit, kind], summary)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn recently_added(c: &Connection, limit: u32) -> Result<Vec<Summary>> {
    let mut st = c.prepare(&format!(
        "SELECT {SUMMARY_COLS} FROM item i LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE}
         WHERE i.kind IN ('movie','show') AND {VISIBLE} ORDER BY i.added_at DESC LIMIT ?1"
    ))?;
    let rows = st.query_map([limit], summary)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileInfo {
    pub id: i64,
    pub path: String,
    pub size: i64,
    pub source_id: i64,
    pub source_name: String,
    /// local, smb, nfs, webdav, ftp, sftp
    pub source_kind: String,
    /// The media probe's result (issue #33), "" until the file is analysed.
    pub probe_json: String,
    /// Set by the core: whether the file's source can be reached right now.
    pub online: bool,
    /// Set by the core from the file name: "Director's Cut", "Extended", ... (issue #34).
    pub edition: String,
    /// This version's own resume position, for a cut of a different length (issue #34).
    pub position_ms: u64,
    /// Issue #48: a complete, current saved copy of this file is on this computer, so it plays
    /// even while its source cannot be reached.
    pub saved: bool,
}

/// The files (versions) of a movie or episode, largest first.
pub fn files_of(c: &Connection, item_id: i64) -> Result<Vec<FileInfo>> {
    let mut st = c.prepare_cached(&format!(
        "SELECT {ABS_PATH_SQL}, coalesce(f.size, 0), f.id, s.id, s.name, s.kind, coalesce(f.probe_json, ''),
                coalesce(fs.position_ms, 0),
                EXISTS (SELECT 1 FROM local_copy lc WHERE lc.file_id = f.id AND lc.kind = 'saved'
                          AND lc.main_id IS NULL AND lc.state = 'done')
         FROM file f JOIN source s ON s.id = f.source_id
         LEFT JOIN file_state fs ON fs.file_id = f.id AND fs.profile_id = {DEFAULT_PROFILE}
         WHERE f.item_id = ?1 ORDER BY f.size DESC"
    ))?;
    let rows = st.query_map([item_id], |r| {
        Ok(FileInfo {
            path: r.get(0)?,
            size: r.get(1)?,
            id: r.get(2)?,
            source_id: r.get(3)?,
            source_name: r.get(4)?,
            source_kind: r.get(5)?,
            probe_json: r.get(6)?,
            online: true,
            edition: String::new(),
            position_ms: r.get::<_, i64>(7)? as u64,
            saved: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Detail {
    pub item: Summary,
    pub original_title: String,
    pub rating: Option<f64>,
    pub runtime_min: Option<i64>,
    pub meta_json: String,
    pub backdrop: String,
    pub poster_full: String,
    pub match_state: String,
    pub tmdb_id: Option<i64>,
    pub parent_title: String,
    pub files: Vec<FileInfo>,
    pub position_ms: u64,
    /// The item's duration as last played (to tell versions of another cut apart).
    pub duration_ms: u64,
    /// The version played last (0 = none), preselected on the page (issue #34).
    pub last_file_id: i64,
    pub children: Vec<Summary>,
    /// For shows and seasons: the episode "Play" should start.
    pub next_up: Option<Summary>,
    /// Issue #4: the state of its saved copy (copies, for seasons and shows).
    pub save: crate::copies::SaveInfo,
}

fn art(c: &Connection, item: i64, role: &str) -> Result<String> {
    Ok(c
        .query_row("SELECT local_path FROM artwork WHERE item_id = ?1 AND role = ?2 AND local_path IS NOT NULL", params![item, role], |r| r.get::<_, String>(0))
        .optional()?
        .unwrap_or_default())
}

pub fn detail(c: &Connection, id: i64) -> Result<Option<Detail>> {
    let Some(item) = c
        .query_row(
            &format!("SELECT {SUMMARY_COLS} FROM item i LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE} WHERE i.id = ?1"),
            [id],
            summary,
        )
        .optional()?
    else {
        return Ok(None);
    };
    type Extra = (
        Option<String>,
        Option<f64>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
    );
    let (original_title, rating, runtime_min, meta_json, match_state, tmdb_id, parent): Extra =
        c.query_row(
            "SELECT original_title, rating, runtime_min, meta_json, match_state, tmdb_id, parent_id FROM item WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )?;
    // Backdrop and full poster: the item's own, else the show's (for seasons and episodes).
    let show_id: Option<i64> = match item.kind.as_str() {
        "season" => parent,
        "episode" => c
            .query_row(
                "SELECT parent_id FROM item WHERE id = ?1",
                [parent.unwrap_or(0)],
                |r| r.get(0),
            )
            .optional()?
            .flatten(),
        _ => None,
    };
    let mut backdrop = art(c, id, "backdrop")?;
    if backdrop.is_empty()
        && let Some(s) = show_id
    {
        backdrop = art(c, s, "backdrop")?;
    }
    let mut poster_full = art(c, id, "poster")?;
    if poster_full.is_empty()
        && let Some(s) = show_id
    {
        poster_full = art(c, s, "poster")?;
    }
    let parent_title: String = match show_id {
        Some(s) => c
            .query_row("SELECT title FROM item WHERE id = ?1", [s], |r| {
                r.get::<_, Option<String>>(0)
            })?
            .unwrap_or_default(),
        None => String::new(),
    };
    let files = files_of(c, id)?;
    let st = crate::queries::playstate(c, id)?;
    let (position_ms, duration_ms) = (st.position_ms, st.duration_ms);
    let last_file_id = last_file_id(c, id)?;
    let children = match item.kind.as_str() {
        "show" | "season" => children(c, id)?,
        _ => vec![],
    };
    let next_up = match item.kind.as_str() {
        "show" | "season" => next_up(c, id, &item.kind)?,
        _ => None,
    };
    Ok(Some(Detail {
        item,
        original_title: original_title.unwrap_or_default(),
        rating,
        runtime_min,
        meta_json: meta_json.unwrap_or_default(),
        backdrop,
        poster_full,
        match_state: match_state.unwrap_or_default(),
        tmdb_id,
        parent_title,
        files,
        position_ms,
        duration_ms,
        last_file_id,
        children,
        next_up,
        save: crate::copies::save_info(c, id)?,
    }))
}

/// The version played last (0 = none).
fn last_file_id(c: &Connection, item_id: i64) -> Result<i64> {
    Ok(c.prepare_cached(
        "SELECT coalesce(last_file_id, 0) FROM playstate WHERE item_id = ?1 AND profile_id = ?2",
    )?
    .query_row(params![item_id, DEFAULT_PROFILE], |r| r.get(0))
    .optional()?
    .unwrap_or(0))
}

/// Issue #48: one episode tile of the show page, everything it shows without a further query.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EpisodeRow {
    pub item: Summary,
    /// The versions, largest first. `probe_json` is the full probe here; the core trims it to
    /// what labels and the summary line need before it reaches the UI.
    pub files: Vec<FileInfo>,
    pub last_file_id: i64,
    /// The episode's duration as last played (another cut keeps its own resume position).
    pub duration_ms: u64,
    pub position_ms: u64,
    pub save: crate::copies::SaveInfo,
}

/// Issue #48: one page of a season's episodes for the show page, including the ones only TMDB
/// knows, with their versions and saved-copy state. Plus the season's episode count.
pub fn episodes(
    c: &Connection,
    season: i64,
    offset: u32,
    limit: u32,
) -> Result<(u32, Vec<EpisodeRow>)> {
    let total: u32 = c.query_row(
        "SELECT count(*) FROM item WHERE parent_id = ?1 AND kind = 'episode'",
        [season],
        |r| r.get(0),
    )?;
    let items: Vec<Summary> = {
        let mut st = c.prepare(&format!(
            "SELECT {SUMMARY_COLS} FROM item i
             LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE}
             WHERE i.parent_id = ?1 AND i.kind = 'episode'
             ORDER BY i.episode_no, i.id LIMIT ?2 OFFSET ?3"
        ))?;
        st.query_map(params![season, limit, offset], summary)?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let id = item.id;
        let st = crate::queries::playstate(c, id)?;
        let on_disk = item.on_disk;
        out.push(EpisodeRow {
            files: if on_disk { files_of(c, id)? } else { vec![] },
            last_file_id: last_file_id(c, id)?,
            duration_ms: st.duration_ms,
            position_ms: st.position_ms,
            save: if on_disk {
                crate::copies::save_info(c, id)?
            } else {
                crate::copies::SaveInfo {
                    state: "none".into(),
                    ..Default::default()
                }
            },
            item,
        });
    }
    Ok((total, out))
}

/// Seasons of a show or episodes of a season for the show page, including the ones only TMDB
/// knows (`on_disk` false), regular seasons first and specials last.
pub fn children(c: &Connection, id: i64) -> Result<Vec<Summary>> {
    let mut st = c.prepare(&format!(
        "SELECT {SUMMARY_COLS} FROM item i
         LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE}
         WHERE i.parent_id = ?1
         ORDER BY coalesce(i.season_no, 0) = 0 AND i.kind = 'season', i.season_no, i.episode_no"
    ))?;
    let rows = st.query_map([id], summary)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The episode to play next: the most recently started unfinished one, else the first unwatched
/// (regular seasons before specials). When everything on disk is watched, the first episode, so
/// the page offers "Play again" (the summary says `watched`).
fn next_up(c: &Connection, id: i64, kind: &str) -> Result<Option<Summary>> {
    let scope = if kind == "show" {
        "e.parent_id IN (SELECT id FROM item WHERE parent_id = ?1)"
    } else {
        "e.parent_id = ?1"
    };
    let sql = format!(
        "SELECT {cols} FROM item i LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE}
         WHERE i.id IN (SELECT e.id FROM item e WHERE {scope}) AND coalesce(p.watched, 0) = 0
           AND EXISTS (SELECT 1 FROM file f WHERE f.item_id = i.id)
         ORDER BY coalesce(p.position_ms, 0) > 0 DESC, p.updated_at DESC,
                  coalesce(i.season_no, 0) = 0, i.season_no, i.episode_no LIMIT 1",
        cols = SUMMARY_COLS
    );
    if let Some(s) = c.query_row(&sql, [id], summary).optional()? {
        return Ok(Some(s));
    }
    let again = format!(
        "SELECT {cols} FROM item i LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = {DEFAULT_PROFILE}
         WHERE i.id IN (SELECT e.id FROM item e WHERE {scope})
           AND EXISTS (SELECT 1 FROM file f WHERE f.item_id = i.id)
         ORDER BY coalesce(i.season_no, 0) = 0, i.season_no, i.episode_no LIMIT 1",
        cols = SUMMARY_COLS
    );
    Ok(c.query_row(&again, [id], summary).optional()?)
}

/// Absolute path of the best file of a playable item (the largest version).
pub fn playable_path(c: &Connection, item_id: i64) -> Result<Option<String>> {
    Ok(playable_files(c, item_id)?
        .into_iter()
        .next()
        .map(|f| f.path))
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayableFile {
    pub file_id: i64,
    pub source_id: i64,
    pub kind: String,
    /// Absolute: an OS path for local files, the path inside the share for network ones.
    pub path: String,
}

/// Every file of a playable item, best first: local copies, then the largest version.
/// The caller skips sources that are offline.
pub fn playable_files(c: &Connection, item_id: i64) -> Result<Vec<PlayableFile>> {
    let mut st = c.prepare(&format!(
        "SELECT f.source_id, s.kind, {ABS_PATH_SQL}, f.id FROM file f JOIN source s ON s.id = f.source_id
         WHERE f.item_id = ?1 ORDER BY s.kind = 'local' DESC, f.size DESC"
    ))?;
    let rows = st.query_map([item_id], |r| {
        Ok(PlayableFile {
            source_id: r.get(0)?,
            kind: r.get(1)?,
            path: r.get(2)?,
            file_id: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

// ---- media probe (issue #33) --------------------------------------------------------------------

/// Library files without probe data, or with data of an older schema (re-probed in the
/// background). Music files are not probed (2.0): their tags give codec, rate, depth, duration. Changed files lose their data in the scan, so they come back here too.
pub fn files_to_probe(c: &Connection, schema: u32) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT f.id FROM file f JOIN source s ON s.id = f.source_id
         WHERE f.item_id IS NOT NULL AND s.root IS NOT NULL AND s.content = 'video'
           AND (f.probe_json IS NULL OR coalesce(json_extract(f.probe_json, '$.schema'), 0) < ?1)
         ORDER BY f.id",
    )?;
    let rows = st.query_map([schema], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Where a file is: (source id, source kind, absolute path in the source, size, item).
pub type ProbeTarget = (i64, String, String, u64, i64);

pub fn probe_target(c: &Connection, file_id: i64) -> Result<Option<ProbeTarget>> {
    Ok(c.query_row(
        &format!(
            "SELECT f.source_id, s.kind, {ABS_PATH_SQL}, coalesce(f.size, 0), f.item_id
             FROM file f JOIN source s ON s.id = f.source_id
             WHERE f.id = ?1 AND f.item_id IS NOT NULL AND s.content IN ('video', 'audiobooks')"
        ),
        [file_id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get::<_, i64>(3)? as u64,
                r.get(4)?,
            ))
        },
    )
    .optional()?)
}

/// The library file at an absolute path (in a given source, or in any).
pub fn file_for_path(c: &Connection, source: Option<i64>, abs_path: &str) -> Result<Option<i64>> {
    Ok(c.query_row(
        &format!(
            "SELECT f.id FROM file f JOIN source s ON s.id = f.source_id
             WHERE {ABS_PATH_SQL} = ?2 AND (?1 IS NULL OR f.source_id = ?1) LIMIT 1"
        ),
        params![source, abs_path],
        |r| r.get(0),
    )
    .optional()?)
}

/// A file's probe JSON, if any.
pub fn probe_json(c: &Connection, file_id: i64) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT probe_json FROM file WHERE id = ?1",
        [file_id],
        |r| r.get(0),
    )
    .optional()?
    .flatten())
}

/// A file's duration from its probe data (0 when not analysed).
pub fn file_duration_ms(c: &Connection, file_id: i64) -> Result<u64> {
    Ok(c.query_row(
        "SELECT coalesce(json_extract(probe_json, '$.duration_ms'), 0) FROM file WHERE id = ?1",
        [file_id],
        |r| r.get::<_, i64>(0),
    )
    .optional()?
    .unwrap_or(0) as u64)
}

/// "This is a different movie" (issue #34): move `file_id` out of its movie into a new item of
/// its own, titled `title`. The new item is locked, so a rescan does not merge it back, and
/// goes to the review list to be matched. Returns the new item.
pub fn split_file(c: &Connection, file_id: i64, title: &str, year: Option<u16>) -> Result<i64> {
    let (old, key): (i64, Option<String>) = c.query_row(
        "SELECT i.id, i.group_key FROM file f JOIN item i ON i.id = f.item_id WHERE f.id = ?1",
        [file_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    c.execute(
        "INSERT INTO item(kind, title, sort_title, year, match_state, locked, added_at, group_key)
         SELECT kind, ?2, ?3, ?4, 'needs_review', 1, ?5, ?6 FROM item WHERE id = ?1",
        params![
            old,
            title,
            sort_key(title),
            year,
            now_ms(),
            format!("{}|split:{file_id}", key.unwrap_or_default())
        ],
    )?;
    let id = c.last_insert_rowid();
    c.execute(
        "INSERT INTO item_fts(rowid, title) VALUES (?1, ?2)",
        params![id, title],
    )?;
    c.execute(
        "UPDATE file SET item_id = ?2 WHERE id = ?1",
        params![file_id, id],
    )?;
    c.execute(
        "UPDATE playstate SET last_file_id = NULL WHERE item_id = ?1 AND last_file_id = ?2",
        params![old, file_id],
    )?;
    Ok(id)
}

/// Store a probe result. Returns the file's item.
pub fn set_probe(c: &Connection, file_id: i64, json: &str) -> Result<Option<i64>> {
    c.execute(
        "UPDATE file SET probe_json = ?2 WHERE id = ?1",
        params![file_id, json],
    )?;
    Ok(
        c.query_row("SELECT item_id FROM file WHERE id = ?1", [file_id], |r| {
            r.get(0)
        })
        .optional()?
        .flatten(),
    )
}

// ---- matching and metadata writes ---------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MatchTarget {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub year: Option<u16>,
    pub locked: bool,
    pub tmdb_id: Option<i64>,
}

pub fn match_target(c: &Connection, id: i64) -> Result<Option<MatchTarget>> {
    Ok(c.query_row(
        "SELECT id, kind, title, year, locked, tmdb_id FROM item WHERE id = ?1",
        [id],
        |r| {
            Ok(MatchTarget {
                id: r.get(0)?,
                kind: r.get(1)?,
                title: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                year: r.get::<_, Option<i64>>(3)?.map(|y| y as u16),
                locked: r.get::<_, i64>(4)? != 0,
                tmdb_id: r.get(5)?,
            })
        },
    )
    .optional()?)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CandidateRow {
    pub provider_id: i64,
    pub title: String,
    pub year: Option<u16>,
    pub score: f64,
    pub poster_path: Option<String>,
    pub overview: String,
}

pub fn set_match_state(
    c: &mut Connection,
    id: i64,
    state: &str,
    candidates: &[CandidateRow],
) -> Result<()> {
    let tx = c.transaction()?;
    tx.execute(
        "UPDATE item SET match_state = ?2, meta_fetched_at = ?3 WHERE id = ?1",
        params![id, state, now_ms()],
    )?;
    tx.execute("DELETE FROM match_candidate WHERE item_id = ?1", [id])?;
    for k in candidates {
        tx.execute(
            "INSERT INTO match_candidate(item_id, provider_id, title, year, score, poster_path, overview) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![id, k.provider_id, k.title, k.year, k.score, k.poster_path, k.overview],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// When the matching rules last changed. Items whose last attempt is older are matched again.
pub const MATCHER_EPOCH_MS: i64 = 1_794_341_019_000; // 2026-11-10 20:03 UTC; never in the future
const RETRY_AFTER_MS: i64 = 7 * 24 * 3600 * 1000;

/// Movies and shows that are not (confidently) matched and are due for another attempt:
/// never tried, tried before the matcher last improved, or tried more than a week ago.
pub fn due_for_matching(c: &Connection) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT id FROM item WHERE kind IN ('movie','show') AND locked = 0
           AND coalesce(match_state, 'unmatched') IN ('unmatched', 'needs_review')
           AND (meta_fetched_at IS NULL OR meta_fetched_at < ?1 OR meta_fetched_at < ?2)",
    )?;
    let rows = st.query_map(params![MATCHER_EPOCH_MS, now_ms() - RETRY_AFTER_MS], |r| {
        r.get(0)
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn candidates(c: &Connection, id: i64) -> Result<Vec<CandidateRow>> {
    let mut st = c.prepare("SELECT provider_id, title, year, score, poster_path, overview FROM match_candidate WHERE item_id = ?1 ORDER BY score DESC")?;
    let rows = st.query_map([id], |r| {
        Ok(CandidateRow {
            provider_id: r.get(0)?,
            title: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            year: r.get::<_, Option<i64>>(2)?.map(|y| y as u16),
            score: r.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
            poster_path: r.get(4)?,
            overview: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Movies and shows waiting for the user to pick a match.
pub fn review_list(c: &Connection) -> Result<Vec<(Summary, Vec<CandidateRow>)>> {
    let ids: Vec<i64> = {
        let mut st = c.prepare("SELECT i.id FROM item i WHERE i.match_state = 'needs_review' AND i.kind IN ('movie','show') ORDER BY i.sort_title")?;
        st.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut out = Vec::new();
    for id in ids {
        if let Some(d) = detail(c, id)? {
            out.push((d.item, candidates(c, id)?));
        }
    }
    Ok(out)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MetaUpdate {
    pub tmdb_id: Option<i64>,
    pub title: Option<String>,
    pub original_title: Option<String>,
    pub year: Option<u16>,
    pub overview: Option<String>,
    pub rating: Option<f64>,
    pub runtime_min: Option<u32>,
    pub imdb_id: Option<String>,
    pub meta_json: Option<String>,
    pub match_state: Option<&'static str>,
}

/// Write provider metadata onto an item. Locked items keep their user-set title and year.
pub fn apply_meta(c: &Connection, id: i64, m: &MetaUpdate) -> Result<()> {
    // A lock protects the user's choice: updates for the chosen TMDB id apply in full, anything
    // else (a different id, or no id) keeps the title and year.
    let (locked, current): (bool, Option<i64>) = c
        .query_row(
            "SELECT locked, tmdb_id FROM item WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, i64>(0)? != 0, r.get(1)?)),
        )
        .optional()?
        .unwrap_or((false, None));
    let locked = locked && (m.tmdb_id.is_none() || m.tmdb_id != current);
    c.execute(
        "UPDATE item SET
           tmdb_id = coalesce(?2, tmdb_id),
           title = CASE WHEN ?11 THEN title ELSE coalesce(nullif(?3, ''), title) END,
           sort_title = CASE WHEN ?11 THEN sort_title ELSE coalesce(?12, sort_title) END,
           original_title = coalesce(?4, original_title),
           year = CASE WHEN ?11 THEN year ELSE coalesce(?5, year) END,
           overview = coalesce(nullif(?6, ''), overview),
           rating = coalesce(?7, rating), runtime_min = coalesce(?8, runtime_min),
           imdb_id = coalesce(?9, imdb_id), meta_json = coalesce(?10, meta_json),
           match_state = coalesce(?13, match_state), meta_fetched_at = ?14
         WHERE id = ?1",
        params![
            id,
            m.tmdb_id,
            m.title,
            m.original_title,
            m.year,
            m.overview,
            m.rating,
            m.runtime_min,
            m.imdb_id,
            m.meta_json,
            locked,
            m.title.as_deref().map(sort_key),
            m.match_state,
            now_ms()
        ],
    )?;
    let (kind, title, original, overview): (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = c.query_row(
        "SELECT kind, title, original_title, overview FROM item WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    if kind == "movie" || kind == "show" {
        c.execute("DELETE FROM item_fts WHERE rowid = ?1", [id])?;
        c.execute(
            "INSERT INTO item_fts(rowid, title, original_title, overview) VALUES (?1, ?2, ?3, ?4)",
            params![id, title, original, overview],
        )?;
    }
    Ok(())
}

fn sort_key(t: &str) -> String {
    let l = t.trim().to_lowercase();
    for a in [
        "the ", "a ", "an ", "der ", "die ", "das ", "le ", "la ", "les ", "el ",
    ] {
        if let Some(rest) = l.strip_prefix(a)
            && !rest.is_empty()
        {
            return rest.to_string();
        }
    }
    l
}

/// The user picked a match (or typed a title): pin it so rescans never overwrite it.
/// The user's choice: lock the item to `tmdb_id`. If another item already has that id, the two
/// are merged first (issue #32). Returns the item that now carries the choice.
pub fn lock(c: &Connection, id: i64, tmdb_id: Option<i64>) -> Result<i64> {
    let id = match tmdb_id {
        Some(t) => crate::merge::claim_tmdb(c, id, t)?,
        None => id,
    };
    c.execute("UPDATE item SET locked = 1, tmdb_id = coalesce(?2, tmdb_id), match_state = 'matched' WHERE id = ?1", params![id, tmdb_id])?;
    c.execute("DELETE FROM match_candidate WHERE item_id = ?1", [id])?;
    Ok(id)
}

/// Record a remote image for an item; the artwork job downloads it.
pub fn set_artwork(c: &Connection, item: i64, role: &str, remote: Option<&str>) -> Result<()> {
    let current: Option<Option<String>> = c
        .query_row(
            "SELECT remote_path FROM artwork WHERE item_id = ?1 AND role = ?2",
            params![item, role],
            |r| r.get(0),
        )
        .optional()?;
    match (current, remote) {
        (Some(cur), Some(r)) if cur.as_deref() == Some(r) => {}
        (Some(_), r) => {
            c.execute("UPDATE artwork SET remote_path = ?3, local_path = NULL WHERE item_id = ?1 AND role = ?2", params![item, role, r])?;
            if role == "poster_grid" {
                // The larger tier was made from the old image.
                c.execute(
                    "DELETE FROM artwork WHERE item_id = ?1 AND role = 'poster_grid_l'",
                    [item],
                )?;
            }
        }
        (None, Some(r)) => {
            c.execute(
                "INSERT INTO artwork(item_id, role, remote_path) VALUES (?1, ?2, ?3)",
                params![item, role, r],
            )?;
        }
        (None, None) => {}
    }
    Ok(())
}

/// Images still to download for an item and its children.
pub fn pending_artwork(c: &Connection, item: i64) -> Result<Vec<(i64, String, String)>> {
    let mut st = c.prepare(
        "SELECT a.item_id, a.role, a.remote_path FROM artwork a
         WHERE a.local_path IS NULL AND a.remote_path IS NOT NULL
           AND (a.item_id = ?1 OR a.item_id IN (SELECT id FROM item WHERE parent_id = ?1)
                OR a.item_id IN (SELECT e.id FROM item e JOIN item s ON e.parent_id = s.id WHERE s.parent_id = ?1))",
    )?;
    let rows = st.query_map([item], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn set_artwork_local(
    c: &Connection,
    item: i64,
    role: &str,
    local: &str,
    w: u32,
    h: u32,
) -> Result<()> {
    c.execute(
        "UPDATE artwork SET local_path = ?3, width = ?4, height = ?5 WHERE item_id = ?1 AND role = ?2",
        params![item, role, local, w, h],
    )?;
    Ok(())
}

/// Record a locally generated image tier (no remote of its own), replacing an older one.
pub fn set_artwork_tier(
    c: &Connection,
    item: i64,
    role: &str,
    local: &str,
    w: u32,
    h: u32,
) -> Result<()> {
    c.execute(
        "DELETE FROM artwork WHERE item_id = ?1 AND role = ?2",
        params![item, role],
    )?;
    c.execute(
        "INSERT INTO artwork(item_id, role, local_path, width, height) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![item, role, local, w, h],
    )?;
    Ok(())
}

/// Items with a grid thumbnail but no `poster_grid_l` tier yet, and a downloaded full-size
/// poster to make it from (offline): (item, local path of the full-size poster).
pub fn missing_large_tiers(c: &Connection, limit: u32) -> Result<Vec<(i64, String)>> {
    let mut st = c.prepare(
        "SELECT g.item_id, p.local_path FROM artwork g
         JOIN artwork p ON p.item_id = g.item_id AND p.role = 'poster' AND p.local_path IS NOT NULL
         WHERE g.role = 'poster_grid' AND g.local_path IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM artwork l WHERE l.item_id = g.item_id AND l.role = 'poster_grid_l')
         ORDER BY g.item_id LIMIT ?1",
    )?;
    let rows = st.query_map([limit], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// A season TMDB knows but the library has no file of: created as a virtual item (shown dimmed
/// on the show page). Returns the existing season when there is one.
pub fn ensure_season(c: &Connection, show: i64, season_no: i64, title: &str) -> Result<i64> {
    if let Some(id) = c
        .query_row(
            "SELECT id FROM item WHERE parent_id = ?1 AND kind = 'season' AND season_no = ?2",
            params![show, season_no],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    c.execute(
        "INSERT INTO item(kind, parent_id, title, sort_title, season_no, match_state, virtual)
         VALUES ('season', ?1, ?2, ?3, ?4, 'matched', 1)",
        params![show, title, sort_key(title), season_no],
    )?;
    Ok(c.last_insert_rowid())
}

/// An episode TMDB knows that is not on disk, as a virtual item. Returns the existing one.
pub fn ensure_episode(c: &Connection, season: i64, episode_no: i64, title: &str) -> Result<i64> {
    if let Some(id) = c
        .query_row(
            "SELECT id FROM item WHERE parent_id = ?1 AND kind = 'episode' AND episode_no = ?2",
            params![season, episode_no],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    let season_no: Option<i64> =
        c.query_row("SELECT season_no FROM item WHERE id = ?1", [season], |r| {
            r.get(0)
        })?;
    c.execute(
        "INSERT INTO item(kind, parent_id, title, sort_title, season_no, episode_no, match_state, virtual)
         VALUES ('episode', ?1, ?2, ?3, ?4, ?5, 'matched', 1)",
        params![season, title, sort_key(title), season_no, episode_no],
    )?;
    Ok(c.last_insert_rowid())
}

/// Seasons and episodes of a show, for applying season details: (season item, season_no,
/// [(episode item, episode_no)]).
/// (season item, season_no, [(episode item, episode_no)])
pub type ShowStructure = Vec<(i64, i64, Vec<(i64, i64)>)>;

/// `on_disk_only`: only seasons and episodes from the scanner, not the ones only TMDB knows
/// (matching compares those with a candidate's seasons).
pub fn show_structure(c: &Connection, show: i64, on_disk_only: bool) -> Result<ShowStructure> {
    let real = if on_disk_only { " AND virtual = 0" } else { "" };
    let seasons: Vec<(i64, i64)> = {
        let mut st = c.prepare(&format!("SELECT id, coalesce(season_no, 0) FROM item WHERE parent_id = ?1 AND kind = 'season'{real} ORDER BY season_no"))?;
        st.query_map([show], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut out = Vec::new();
    for (sid, no) in seasons {
        let mut st = c.prepare(&format!("SELECT id, coalesce(episode_no, 0) FROM item WHERE parent_id = ?1 AND kind = 'episode'{real}"))?;
        let eps = st
            .query_map([sid], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        out.push((sid, no, eps));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #78: lenses, genre and decade filters, the "most played" sort, format labels and
    /// facets on a small music library.
    fn music_fixture() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrate(&mut c).unwrap();
        c.execute_batch(
            "INSERT INTO source(id, kind, name, root, content) VALUES (2, 'local', 'Music', '/m', 'music');
             INSERT INTO item(id, kind, title, sort_title, year) VALUES
               (10, 'album', 'Kind of Blue', 'kind of blue', 1959),
               (20, 'album', 'The Wall', 'wall', 1979),
               (30, 'album', 'Abbey Road', 'abbey road', 1969),
               (40, 'album', '1999', '1999', 1982);
             INSERT INTO item(id, kind, title, sort_title, parent_id) VALUES
               (11, 'track', 'So What', 'so what', 10), (21, 'track', 'In the Flesh', 'in the flesh', 20),
               (31, 'track', 'Come Together', 'come together', 30), (41, 'track', '1999', '1999', 40);
             INSERT INTO album_version(id, album_id, key, source_id) VALUES (1, 20, 'v1', 2), (2, 20, 'v2', 2), (3, 10, 'v3', 2);
             INSERT INTO file(id, source_id, path, size, item_id) VALUES
               (100, 2, 'a.flac', 1, 11), (200, 2, 'b.mp3', 1, 21), (201, 2, 'b.flac', 1, 21), (300, 2, 'c.flac', 1, 31), (400, 2, 'd.mp3', 1, 41);
             INSERT INTO file_audio(file_id, codec, sample_rate, bit_depth, lossless, bitrate) VALUES
               (100, 'flac', 96000, 24, 1, 2500), (200, 'mp3', 44100, 0, 0, 320), (201, 'flac', 44100, 16, 1, 900),
               (300, 'flac', 44100, 16, 1, 900), (400, 'mp3', 44100, 0, 0, 256);
             INSERT INTO music(item_id, genre) VALUES (10, 'Jazz'), (20, 'Rock'), (30, 'Rock');
             INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at, play_count)
               VALUES (31, 1, 0, 0, 0, 1, 9), (11, 1, 0, 0, 0, 1, 2);",
        )
        .unwrap();
        c
    }

    fn titles(c: &Connection, music: MusicFilter, sort: Sort) -> Vec<String> {
        let q = Query {
            kind: "album".into(),
            parent: None,
            sort,
            unwatched_only: false,
            saved_only: false,
            book_filter: "",
            music,
            offset: 0,
            limit: 50,
        };
        browse(c, &q)
            .unwrap()
            .1
            .into_iter()
            .map(|s| s.title)
            .collect()
    }

    /// Issue #79: album artists by default, everyone credited on request, artists with Hi-Res
    /// albums, and the album counts on their tiles.
    #[test]
    fn artist_filters_and_counts() {
        let c = music_fixture();
        c.execute_batch(
            "INSERT INTO item(id, kind, title, sort_title) VALUES (50, 'artist', 'Miles Davis', 'miles davis'),
               (60, 'artist', 'The Band', 'band'), (70, 'artist', 'Guest Singer', 'guest singer');
             UPDATE item SET parent_id = 50 WHERE id = 10;
             UPDATE item SET parent_id = 60 WHERE id IN (20, 30);
             INSERT INTO credit(item_id, artist_id, role, position) VALUES (21, 70, 'artist', 1);",
        )
        .unwrap();
        let list = |m: MusicFilter| -> Vec<(String, i64, i64)> {
            let q = Query {
                kind: "artist".into(),
                parent: None,
                sort: Sort::Title,
                unwatched_only: false,
                saved_only: false,
                book_filter: "",
                music: m,
                offset: 0,
                limit: 50,
            };
            browse(&c, &q)
                .unwrap()
                .1
                .into_iter()
                .map(|s| (s.title, s.albums, s.hires_albums))
                .collect()
        };
        assert_eq!(
            list(MusicFilter::default()),
            [
                ("The Band".to_string(), 2, 0),
                ("Miles Davis".to_string(), 1, 1)
            ]
        );
        let everyone: Vec<String> = list(MusicFilter {
            credited: true,
            ..Default::default()
        })
        .into_iter()
        .map(|a| a.0)
        .collect();
        assert_eq!(everyone, ["The Band", "Guest Singer", "Miles Davis"]);
        assert_eq!(
            list(MusicFilter {
                hires: true,
                ..Default::default()
            }),
            [("Miles Davis".to_string(), 1, 1)]
        );
        let fc = facets(&c, "artist", &MusicFilter::default(), false).unwrap();
        assert_eq!((fc.all, fc.hires), (2, 1));
        assert_eq!(fc.letters, [("B".to_string(), 0), ("M".to_string(), 1)]);
    }

    /// Issue #80: songs by quality (best file first), never played, Hi-Res, and their ids in
    /// order for the queue.
    #[test]
    fn songs_sorts_and_filters() {
        let c = music_fixture();
        let q = |sort, m| Query {
            kind: "track".into(),
            parent: None,
            sort,
            unwatched_only: false,
            saved_only: false,
            book_filter: "",
            music: m,
            offset: 0,
            limit: 50,
        };
        let titles = |q: Query| -> Vec<String> {
            browse(&c, &q)
                .unwrap()
                .1
                .into_iter()
                .map(|s| s.title)
                .collect()
        };
        assert_eq!(
            titles(q(Sort::Quality, MusicFilter::default())),
            ["So What", "Come Together", "In the Flesh", "1999"]
        );
        assert_eq!(
            titles(q(
                Sort::Title,
                MusicFilter {
                    unplayed: true,
                    ..Default::default()
                }
            )),
            ["1999", "In the Flesh"]
        );
        assert_eq!(
            titles(q(
                Sort::Title,
                MusicFilter {
                    hires: true,
                    ..Default::default()
                }
            )),
            ["So What"]
        );
        let rows = browse(&c, &q(Sort::Plays, MusicFilter::default()))
            .unwrap()
            .1;
        assert_eq!(
            (rows[0].title.as_str(), rows[0].plays),
            ("Come Together", 9)
        );
        assert_eq!(rows[0].format, "FLAC 16/44");
        assert_eq!(
            browse_ids(&c, &q(Sort::Quality, MusicFilter::default()), 3).unwrap(),
            [11, 31, 21]
        );
    }

    #[test]
    fn music_lenses_and_facets() {
        let c = music_fixture();
        let all = titles(&c, MusicFilter::default(), Sort::Title);
        assert_eq!(all, ["1999", "Abbey Road", "Kind of Blue", "The Wall"]);
        let f = |m| titles(&c, m, Sort::Title);
        assert_eq!(
            f(MusicFilter {
                hires: true,
                ..Default::default()
            }),
            ["Kind of Blue"]
        );
        assert_eq!(
            f(MusicFilter {
                lossless: true,
                ..Default::default()
            }),
            ["Abbey Road", "Kind of Blue", "The Wall"]
        );
        assert_eq!(
            f(MusicFilter {
                multi: true,
                ..Default::default()
            }),
            ["The Wall"]
        );
        assert_eq!(
            f(MusicFilter {
                genre: Some("Rock".into()),
                lossless: true,
                ..Default::default()
            }),
            ["Abbey Road", "The Wall"]
        );
        assert_eq!(
            f(MusicFilter {
                decade: Some(1970),
                ..Default::default()
            }),
            ["The Wall"]
        );
        assert_eq!(
            titles(&c, MusicFilter::default(), Sort::Plays)[..2],
            ["Abbey Road", "Kind of Blue"]
        );

        // The best version's format: The Wall has an MP3 and a FLAC copy.
        let q = Query {
            kind: "album".into(),
            parent: None,
            sort: Sort::Title,
            unwatched_only: false,
            saved_only: false,
            book_filter: "",
            music: MusicFilter::default(),
            offset: 0,
            limit: 50,
        };
        let rows = browse(&c, &q).unwrap().1;
        let fmt: Vec<(String, bool)> = rows.iter().map(|s| (s.format.clone(), s.hires)).collect();
        assert_eq!(
            fmt,
            [
                ("MP3 256k".to_string(), false),
                ("FLAC 16/44".to_string(), false),
                ("FLAC 24/96".to_string(), true),
                ("FLAC 16/44".to_string(), false)
            ]
        );

        let fc = facets(
            &c,
            "album",
            &MusicFilter {
                genre: Some("Rock".into()),
                ..Default::default()
            },
            false,
        )
        .unwrap();
        assert_eq!(
            (fc.all, fc.total, fc.hires, fc.lossless, fc.multi),
            (2, 2, 0, 2, 1)
        );
        assert_eq!(
            fc.genres,
            [("Jazz".to_string(), 1), ("Rock".to_string(), 2)]
        );
        assert_eq!(fc.decades, [(1970, 1), (1960, 1)]);
        assert_eq!(fc.letters, [("A".to_string(), 0), ("W".to_string(), 1)]);
        let fc = facets(&c, "album", &MusicFilter::default(), false).unwrap();
        assert_eq!(
            fc.letters,
            [
                ("#".to_string(), 0),
                ("A".to_string(), 1),
                ("K".to_string(), 2),
                ("W".to_string(), 3)
            ]
        );
        assert_eq!(
            format_label("dsd_lsbf|1|2822400|0|1"),
            ("DSD".to_string(), true)
        );
        assert_eq!(
            format_label("opus|0|48000|160|0"),
            ("Opus 160k".to_string(), false)
        );
    }

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrate(&mut c).unwrap();
        c
    }

    #[test]
    fn library_items_browse_search_detail() {
        let mut c = db();
        let lib = add_library(&c, "Media", "/m", "video").unwrap();
        assert_eq!(add_library(&c, "Again", "/m/", "video").unwrap(), lib);
        c.execute_batch(&format!(
            "INSERT INTO item(id, kind, title, sort_title, year, added_at) VALUES (1,'movie','The Matrix','matrix',1999,10), (2,'movie','Alien','alien',1979,20);
             INSERT INTO item(id, kind, title, sort_title, added_at) VALUES (3,'show','Firefly','firefly',30);
             INSERT INTO item(id, kind, title, parent_id, season_no) VALUES (4,'season','Season 1',3,1);
             INSERT INTO item(id, kind, title, parent_id, season_no, episode_no) VALUES (5,'episode','Serenity',4,1,1), (6,'episode','Train Job',4,1,2);
             INSERT INTO file(source_id, path, size, item_id) VALUES ({lib},'Matrix.mkv',5,1), ({lib},'Alien.mkv',5,2), ({lib},'F/S01E01.mkv',1,5), ({lib},'F/S01E02.mkv',1,6);
             INSERT INTO item_fts(rowid, title) VALUES (1,'The Matrix'), (2,'Alien'), (3,'Firefly');"
        ))
        .unwrap();
        crate::queries::set_watched(&mut c, 5, true).unwrap();

        let q = |sort| Query {
            kind: "movie".into(),
            parent: None,
            sort,
            unwatched_only: false,
            saved_only: false,
            book_filter: "",
            music: Default::default(),
            offset: 0,
            limit: 50,
        };
        let (n, page) = browse(&c, &q(Sort::Title)).unwrap();
        assert_eq!(n, 2);
        assert_eq!(
            page.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(),
            ["Alien", "The Matrix"]
        );
        assert_eq!(browse(&c, &q(Sort::Added)).unwrap().1[0].title, "Alien");
        let (_, shows) = browse(
            &c,
            &Query {
                kind: "show".into(),
                ..q(Sort::Title)
            },
        )
        .unwrap();
        assert_eq!(
            (shows[0].title.as_str(), shows[0].unwatched),
            ("Firefly", 1)
        );

        assert_eq!(search(&c, "matr", "", 10).unwrap()[0].id, 1);
        // Issue #10: one kind of result at a time.
        assert_eq!(search(&c, "matr", "movie", 10).unwrap().len(), 1);
        assert!(search(&c, "matr", "show", 10).unwrap().is_empty());
        assert_eq!(
            search(&c, "\"firef", "", 10).unwrap()[0].id,
            3,
            "stray quotes do not break FTS"
        );
        assert!(search(&c, "  ", "", 10).unwrap().is_empty());

        let d = detail(&c, 3).unwrap().unwrap();
        assert_eq!(d.children.len(), 1);
        assert_eq!(d.next_up.unwrap().id, 6, "first unwatched episode");
        let e = detail(&c, 6).unwrap().unwrap();
        assert_eq!(e.parent_title, "Firefly");
        assert_eq!(e.files[0].path, "/m/F/S01E02.mkv");
        assert_eq!(playable_path(&c, 1).unwrap().unwrap(), "/m/Matrix.mkv");
        assert_eq!(library_item_for_path(&c, "/m/Alien.mkv").unwrap(), Some(2));
        assert_eq!(
            library_item_for_path(&c, "/elsewhere/Alien.mkv").unwrap(),
            None
        );

        remove_library(&mut c, lib).unwrap();
        assert_eq!(
            c.query_row("SELECT count(*) FROM item", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn large_grid_tier_is_listed_made_and_dropped_with_its_image() {
        let mut c = db();
        let lib = add_library(&c, "Media", "/m", "video").unwrap();
        c.execute_batch(&format!(
            "INSERT INTO item(id, kind, title, sort_title, added_at) VALUES (1,'movie','Alien','alien',1), (2,'season','Season 1','s',2);
             INSERT INTO file(source_id, path, size, item_id) VALUES ({lib},'Alien.mkv',5,1);"
        ))
        .unwrap();
        for (item, role) in [(1, "poster"), (1, "poster_grid"), (2, "poster_grid")] {
            set_artwork(&c, item, role, Some("/a.jpg")).unwrap();
            set_artwork_local(&c, item, role, &format!("/d/{item}/{role}.jpg"), 360, 540).unwrap();
        }
        // The season has no full-size original, so it cannot get the tier offline.
        assert_eq!(
            missing_large_tiers(&c, 10).unwrap(),
            vec![(1, "/d/1/poster.jpg".to_string())]
        );

        let one = |c: &mut Connection| {
            let q = Query {
                kind: "movie".into(),
                parent: None,
                sort: Sort::Title,
                unwatched_only: false,
                saved_only: false,
                book_filter: "",
                music: Default::default(),
                offset: 0,
                limit: 10,
            };
            browse(c, &q).unwrap().1.remove(0)
        };
        assert_eq!(one(&mut c).poster_large, "");
        set_artwork_tier(&c, 1, "poster_grid_l", "/d/1/poster_grid_l.jpg", 540, 810).unwrap();
        set_artwork_tier(&c, 1, "poster_grid_l", "/d/1/poster_grid_l.jpg", 540, 810).unwrap();
        let s = one(&mut c);
        assert_eq!(
            (s.poster.as_str(), s.poster_large.as_str()),
            ("/d/1/poster_grid.jpg", "/d/1/poster_grid_l.jpg")
        );
        assert!(missing_large_tiers(&c, 10).unwrap().is_empty());

        // A new poster drops the tier made from the old one.
        set_artwork(&c, 1, "poster_grid", Some("/b.jpg")).unwrap();
        assert_eq!(one(&mut c).poster_large, "");
    }

    /// Issue #32, show page: seasons with specials last, episodes TMDB knows but the library
    /// has not (`on_disk` false), and the main button in its four cases.
    #[test]
    fn show_page_children_and_next_episode() {
        let mut c = db();
        let lib = add_library(&c, "Media", "/m", "video").unwrap();
        c.execute_batch(&format!(
            "INSERT INTO item(id, kind, title) VALUES (1, 'show', 'Show');
             INSERT INTO item(id, kind, parent_id, title, season_no) VALUES (10, 'season', 1, 'Specials', 0), (11, 'season', 1, 'Season 1', 1), (12, 'season', 1, 'Season 2', 2);
             INSERT INTO item(id, kind, parent_id, title, season_no, episode_no, runtime_min) VALUES
               (100, 'episode', 10, 'Special', 0, 1, 30),
               (111, 'episode', 11, 'One', 1, 1, 44), (112, 'episode', 11, 'Two', 1, 2, 45),
               (121, 'episode', 12, 'Three', 2, 1, 46);
             INSERT INTO file(source_id, path, size, item_id) VALUES ({lib}, 's0e1', 1, 100), ({lib}, 's1e1', 1, 111), ({lib}, 's1e2', 1, 112), ({lib}, 's2e1', 1, 121);"
        ))
        .unwrap();
        // TMDB knows S02E02 and a third season the library does not have.
        let s3 = ensure_season(&c, 1, 3, "Season 3").unwrap();
        ensure_episode(&c, s3, 1, "Later").unwrap();
        ensure_episode(&c, 12, 2, "Four").unwrap();
        assert_eq!(ensure_season(&c, 1, 3, "again").unwrap(), s3, "idempotent");

        let seasons = children(&c, 1).unwrap();
        let order: Vec<_> = seasons.iter().map(|s| (s.season_no, s.on_disk)).collect();
        assert_eq!(
            order,
            [
                (Some(1), true),
                (Some(2), true),
                (Some(3), false),
                (Some(0), true)
            ],
            "specials last, season 3 not on disk"
        );
        let eps = children(&c, 12).unwrap();
        let rows: Vec<_> = eps
            .iter()
            .map(|e| (e.episode_no, e.on_disk, e.runtime_min))
            .collect();
        assert_eq!(rows, [(Some(1), true, Some(46)), (Some(2), false, None)]);

        let next = |c: &Connection| {
            let d = detail(c, 1).unwrap().unwrap();
            let n = d.next_up.unwrap();
            (n.id, n.watched)
        };
        // Nothing watched: S01E01.
        assert_eq!(next(&c), (111, false));
        // One episode half watched: resume it.
        crate::queries::save_progress(&mut c, 112, 600_000, 2_700_000, false).unwrap();
        assert_eq!(next(&c), (112, false));
        // Season 1 finished: the next season.
        crate::queries::set_watched(&mut c, 111, true).unwrap();
        crate::queries::set_watched(&mut c, 112, true).unwrap();
        assert_eq!(next(&c), (121, false));
        // Everything on disk watched: start again at S01E01 (the page says "Play again").
        crate::queries::set_watched(&mut c, 121, true).unwrap();
        crate::queries::set_watched(&mut c, 100, true).unwrap();
        assert_eq!(next(&c), (111, true));
    }

    #[test]
    fn metadata_respects_locks_and_updates_search() {
        let c = db();
        c.execute_batch("INSERT INTO item(id, kind, title, sort_title, year) VALUES (1,'movie','the thing','the thing',NULL); INSERT INTO item_fts(rowid,title) VALUES (1,'the thing');").unwrap();
        let m = MetaUpdate {
            tmdb_id: Some(1091),
            title: Some("The Thing".into()),
            year: Some(1982),
            overview: Some("Antarctica".into()),
            match_state: Some("matched"),
            ..Default::default()
        };
        apply_meta(&c, 1, &m).unwrap();
        let t = match_target(&c, 1).unwrap().unwrap();
        assert_eq!(
            (t.title.as_str(), t.year, t.tmdb_id),
            ("The Thing", Some(1982), Some(1091))
        );
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM item_fts WHERE item_fts MATCH 'antarctica'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        lock(&c, 1, Some(1091)).unwrap();
        apply_meta(
            &c,
            1,
            &MetaUpdate {
                title: Some("Something Else".into()),
                year: Some(2011),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            match_target(&c, 1).unwrap().unwrap().title,
            "The Thing",
            "locked title kept"
        );
        set_artwork(&c, 1, "poster", Some("/p.jpg")).unwrap();
        assert_eq!(pending_artwork(&c, 1).unwrap().len(), 1);
        set_artwork_local(&c, 1, "poster", "/cache/p.jpg", 780, 1170).unwrap();
        assert!(pending_artwork(&c, 1).unwrap().is_empty());
        set_artwork(&c, 1, "poster", Some("/p.jpg")).unwrap();
        assert!(
            pending_artwork(&c, 1).unwrap().is_empty(),
            "same remote image is not refetched"
        );
    }

    /// Issue #48: the fixture show of the episode tiles. One season of six episodes: E1 in two
    /// versions (one on the NAS), E2 only TMDB knows, E3 saved, E4 not analysed yet.
    pub(crate) fn example_show(c: &Connection) {
        c.execute_batch(
            r#"INSERT INTO source(id, kind, name, root) VALUES (2, 'local', 'Media', '/m'), (3, 'smb', 'NAS', '/share');
             INSERT INTO item(id, kind, title, sort_title, year) VALUES (50, 'show', 'Example Show', 'example show', 2020);
             INSERT INTO item(id, kind, title, parent_id, season_no) VALUES (51, 'season', 'Season 1', 50, 1);
             INSERT INTO item(id, kind, title, parent_id, season_no, episode_no) VALUES
               (61, 'episode', 'One', 51, 1, 1), (62, 'episode', 'Two', 51, 1, 2), (63, 'episode', 'Three', 51, 1, 3),
               (64, 'episode', 'Four', 51, 1, 4), (65, 'episode', 'Five', 51, 1, 5), (66, 'episode', 'Six', 51, 1, 6);
             INSERT INTO file(id, source_id, path, size, item_id, probe_json) VALUES
               (701, 2, 'Example Show/S01E01.1080p.mkv', 2000, 61, '{"schema":3,"video":[{"codec":"h264","width":1920,"height":1080}]}'),
               (702, 3, 'Example Show/S01E01.2160p.mkv', 9000, 61, '{"schema":3,"video":[{"codec":"hevc","width":3840,"height":2160,"hdr":"HDR10"}]}'),
               (703, 3, 'Example Show/S01E03.mkv', 3000, 63, '{"schema":3}'),
               (704, 2, 'Example Show/S01E04.mkv', 3000, 64, NULL),
               (705, 2, 'Example Show/S01E05.mkv', 3000, 65, '{"schema":3}'),
               (706, 2, 'Example Show/S01E06.mkv', 3000, 66, '{"schema":3}');
             INSERT INTO local_copy(id, kind, file_id, item_id, source_id, source_path, path, size, bytes_done, state, created_at)
               VALUES (900, 'saved', 703, 63, 3, '/share/Example Show/S01E03.mkv', '/store/S01E03.mkv', 3000, 3000, 'done', 1),
                      (901, 'export', 702, 61, 3, '/share/Example Show/S01E01.2160p.mkv', '/home/me/E01.mkv', 9000, 9000, 'done', 1);
             INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at, last_file_id)
               VALUES (61, 1, 60000, 1200000, 0, 5, 702);"#,
        )
        .unwrap();
    }

    #[test]
    fn episode_tiles_carry_versions_and_saved_state() {
        let c = db();
        example_show(&c);
        let (total, eps) = episodes(&c, 51, 0, 50).unwrap();
        assert_eq!(total, 6);
        assert_eq!(
            eps.iter()
                .map(|e| e.item.episode_no.unwrap())
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 6]
        );
        // E1: both versions, largest first, with the one played last and its resume point.
        let e1 = &eps[0];
        assert_eq!(
            e1.files.iter().map(|f| f.id).collect::<Vec<_>>(),
            [702, 701]
        );
        assert_eq!(e1.files[0].source_name, "NAS");
        assert_eq!(
            (e1.last_file_id, e1.position_ms, e1.duration_ms),
            (702, 60000, 1200000)
        );
        assert!(e1.files[0].probe_json.contains("HDR10"));
        // E1 was only exported with Save as…: no saved copy, nothing to delete.
        assert_eq!(e1.save.state, "none");
        assert_eq!(e1.save.saveable, 1);
        assert!(!e1.item.saved && !e1.files[0].saved);
        // E2: only TMDB knows it; no versions, nothing to save.
        assert!(!eps[1].item.on_disk);
        assert!(eps[1].files.is_empty());
        assert_eq!(eps[1].save.saveable, 0);
        // E3: saved, and the file says so (it plays while the NAS is away).
        assert_eq!(eps[2].save.state, "done");
        assert!(eps[2].item.saved);
        assert!(eps[2].files[0].saved);
        assert!(!e1.files[0].saved);
        // E4: not analysed yet.
        assert_eq!(eps[3].files[0].probe_json, "");
        // The detail page reads the same files.
        let d = detail(&c, 61).unwrap().unwrap();
        assert_eq!(d.files, e1.files);
    }

    #[test]
    fn episode_pages_cover_a_long_season() {
        let c = db();
        example_show(&c);
        let mut sql = String::new();
        for n in 7..=200 {
            sql.push_str(&format!(
                "INSERT INTO item(id, kind, title, parent_id, season_no, episode_no) VALUES ({}, 'episode', 'E{n}', 51, 1, {n});
                 INSERT INTO file(source_id, path, size, item_id) VALUES (2, 'Example Show/S01E{n:03}.mkv', 10, {});",
                1000 + n,
                1000 + n
            ));
        }
        c.execute_batch(&sql).unwrap();
        let mut seen = Vec::new();
        let mut offset = 0;
        loop {
            let (total, page) = episodes(&c, 51, offset, 60).unwrap();
            assert_eq!(total, 200);
            if page.is_empty() {
                break;
            }
            offset += page.len() as u32;
            seen.extend(page.into_iter().map(|e| e.item.episode_no.unwrap()));
        }
        assert_eq!(seen, (1..=200).collect::<Vec<_>>());
    }
}
