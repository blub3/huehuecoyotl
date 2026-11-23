//! The music scan pipeline (issue #7, SPEC 7 "Music library"): for library folders with the
//! content type `music`. Files are diffed by path, size and modification time like video; new and
//! changed files have their tags read (range reads only, see [`crate::tags`]); tracks are grouped
//! into albums and artists. Video items are never touched.

use std::collections::{HashMap, HashSet};

use huehuecoyotl_library::rusqlite::{self, OptionalExtension, Transaction, params};
use huehuecoyotl_library::{Library, versions};
use huehuecoyotl_vfs::{Source, VPath, VfsError};

use crate::scan::{ScanError, fingerprint_with_data, normalize, sort_title};
use crate::tags::{self, Credit, TrackTags};

/// Extensions a music folder is scanned for (SPEC 2, "Music in 2.0").
pub const AUDIO_EXT: &[&str] = &[
    "flac", "m4a", "mp4", "wav", "aif", "aiff", "mp3", "aac", "ogg", "oga", "opus", "wv",
];
/// Sidecar covers, in this order of preference.
const COVER_NAMES: &[&str] = &["cover", "folder", "front", "album"];
const COVER_EXT: &[&str] = &["jpg", "jpeg", "png"];
const SKIP_DIRS: &[&str] = &[
    "@eadir",
    "#recycle",
    "$recycle.bin",
    "system volume information",
    "lost+found",
    ".trash",
];
/// Files read at a time over the network.
const READ_AHEAD: usize = 8;
pub const VARIOUS_ARTISTS: &str = "Various Artists";

fn ext(name: &str) -> Option<String> {
    let (stem, e) = name.rsplit_once('.')?;
    (!stem.is_empty()).then(|| e.to_ascii_lowercase())
}

pub fn is_audio(name: &str) -> bool {
    ext(name).is_some_and(|e| AUDIO_EXT.contains(&e.as_str()))
}

/// The rank of a sidecar cover (lower is better), `None` for other files.
fn cover_rank(name: &str) -> Option<usize> {
    let (stem, e) = name.rsplit_once('.')?;
    if !COVER_EXT.contains(&e.to_ascii_lowercase().as_str()) {
        return None;
    }
    COVER_NAMES
        .iter()
        .position(|c| stem.eq_ignore_ascii_case(c))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Where an album's cover comes from: the scan hands these to the core, which writes the tiers.
#[derive(Clone, Debug, PartialEq)]
pub enum Cover {
    Embedded(Vec<u8>),
    Sidecar(VPath),
}

#[derive(Debug, Default)]
pub struct MusicReport {
    pub files_seen: usize,
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
    pub moved: usize,
    /// Files whose tags could not be read: they are listed under their file name.
    pub unreadable: usize,
    /// Albums created by this scan (to look up online).
    pub new_albums: Vec<i64>,
    /// A cover for albums that have none yet.
    pub covers: Vec<(i64, Cover)>,
}

struct Walked {
    rel: String,
    path: VPath,
    size: u64,
    mtime: i64,
}

/// Audio files, and the best sidecar cover of each folder (by relative folder).
async fn walk(
    src: &dyn Source,
    root: &VPath,
) -> Result<(Vec<Walked>, HashMap<String, VPath>), VfsError> {
    let prefix = format!("{}/", root.as_str().trim_end_matches('/'));
    let rel = |p: &VPath| {
        p.as_str()
            .strip_prefix(&prefix)
            .unwrap_or(p.as_str())
            .to_string()
    };
    let mut files = Vec::new();
    let mut covers: HashMap<String, (usize, VPath)> = HashMap::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let entries = match src.list(&dir).await {
            Ok(e) => e,
            Err(e) if dir == *root => return Err(e),
            Err(e) => {
                tracing::warn!(%dir, "skipping unreadable folder: {e}");
                continue;
            }
        };
        for e in entries {
            if e.is_dir {
                if !SKIP_DIRS.contains(&e.name.to_lowercase().as_str()) {
                    stack.push(e.path);
                }
            } else if is_audio(&e.name) {
                files.push(Walked {
                    rel: rel(&e.path),
                    path: e.path,
                    size: e.size,
                    mtime: e.mtime,
                });
            } else if let Some(rank) = cover_rank(&e.name) {
                let folder = parent(&rel(&e.path)).to_string();
                if covers.get(&folder).is_none_or(|(r, _)| rank < *r) {
                    covers.insert(folder, (rank, e.path));
                }
            }
        }
    }
    Ok((
        files,
        covers.into_iter().map(|(k, (_, p))| (k, p)).collect(),
    ))
}

fn parent(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(d, _)| d)
}

fn is_disc_folder(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    for p in ["cd", "disc", "disk", "dvd"] {
        if let Some(rest) = n.strip_prefix(p) {
            let rest = rest.trim_start_matches([' ', '-', '_', '.']);
            if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

/// The album's folder: the file's folder, or its parent when the file sits in `CD1`, `Disc 2`.
pub fn album_folder(rel: &str) -> &str {
    let dir = parent(rel);
    match dir.rsplit_once('/') {
        Some((up, last)) if is_disc_folder(last) => up,
        None if is_disc_folder(dir) => "",
        _ => dir,
    }
}

/// Everything a track item is built from: its tags, or what the path says when it has none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackInput {
    pub title: String,
    pub artists: Vec<Credit>,
    pub album: String,
    pub album_artists: Vec<Credit>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<u16>,
    pub tags: TrackTags,
}

/// Leading track numbers: "01 - ", "01. ", "1-01 " (disc 1, track 1), "01_".
fn strip_number(stem: &str) -> (Option<u32>, Option<u32>, &str) {
    let b = stem.as_bytes();
    let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 3 {
        return (None, None, stem);
    }
    let first: u32 = stem[..digits].parse().unwrap_or(0);
    let mut rest = &stem[digits..];
    let (mut disc, mut track) = (None, Some(first));
    // "1-01 Title": disc and track.
    if let Some(r) = rest.strip_prefix('-') {
        let d2 = r.bytes().take_while(|c| c.is_ascii_digit()).count();
        if (2..=3).contains(&d2) && r[d2..].starts_with([' ', '.', '_']) {
            disc = Some(first);
            track = r[..d2].parse().ok();
            rest = &r[d2..];
        }
    }
    let trimmed = rest.trim_start_matches([' ', '.', '-', '_']);
    if trimmed.len() == rest.len() || trimmed.is_empty() {
        // "1984.flac" is a title, not a track number.
        return (None, None, stem);
    }
    (disc, track, trimmed)
}

/// "Album (2001)", "[2001] Album", "2001 - Album" → (album, year).
fn album_and_year(name: &str) -> (String, Option<u16>) {
    let n = name.trim();
    let year_of = |s: &str| s.parse::<u16>().ok().filter(|y| (1900..=2100).contains(y));
    if let Some(open) = n.rfind(['(', '[']) {
        let inner = n[open + 1..].trim_end_matches([')', ']']);
        if let Some(y) = year_of(inner) {
            return (n[..open].trim().to_string(), Some(y));
        }
    }
    if n.starts_with('[')
        && let Some(close) = n.find(']')
        && let Some(y) = year_of(&n[1..close])
    {
        return (
            n[close + 1..]
                .trim_start_matches([' ', '-'])
                .trim()
                .to_string(),
            Some(y),
        );
    }
    if n.len() > 7
        && n.as_bytes()[4] == b' '
        && let Some(y) = year_of(&n[..4])
    {
        let rest = n[4..].trim_start_matches([' ', '-']).trim();
        if !rest.is_empty() {
            return (rest.to_string(), Some(y));
        }
    }
    (n.to_string(), None)
}

fn one(name: &str) -> Vec<Credit> {
    vec![Credit {
        name: name.to_string(),
        join: String::new(),
    }]
}

/// What a file without (usable) tags is called, from its path relative to the library folder:
/// `Artist/Album (Year)/01 - Title.ext`, `Artist - Album/01. Title.ext`, `01 Artist - Title.ext`.
pub fn from_path(rel: &str) -> TrackInput {
    let file = rel.rsplit('/').next().unwrap_or(rel);
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let (disc, track, rest) = strip_number(stem);
    let mut title = rest.trim().to_string();
    let mut artist = String::new();
    let folder = album_folder(rel);
    let mut parts: Vec<&str> = folder.split('/').filter(|p| !p.is_empty()).collect();
    let mut album = String::new();
    let mut year = None;
    if let Some(dir) = parts.pop() {
        let (a, y) = album_and_year(dir);
        year = y;
        match a.split_once(" - ") {
            Some((ar, al)) => {
                artist = ar.trim().to_string();
                album = al.trim().to_string();
            }
            None => album = a,
        }
        if artist.is_empty()
            && let Some(up) = parts.pop()
        {
            artist = up.trim().to_string();
        }
    }
    // "Artist - Title" in the file name: when the folders name no artist, the same one, or only
    // say "various" (a compilation). Otherwise " - " belongs to the title ("Song - Live").
    let various = [
        "various",
        "various artists",
        "va",
        "v a",
        "compilations",
        "sampler",
    ]
    .contains(&normalize(&artist).as_str());
    if let Some((a, t)) = title.clone().split_once(" - ")
        && (artist.is_empty() || various || normalize(a) == normalize(&artist) || album.is_empty())
    {
        artist = a.trim().to_string();
        title = t.trim().to_string();
    }
    if title.is_empty() {
        title = stem.to_string();
    }
    TrackInput {
        title,
        artists: if artist.is_empty() {
            vec![]
        } else {
            one(&artist)
        },
        album,
        album_artists: vec![],
        track_no: track,
        disc_no: disc,
        year,
        tags: TrackTags::default(),
    }
}

/// Tags where they exist, the path for what they lack.
pub fn input(rel: &str, tags: Option<TrackTags>) -> TrackInput {
    let path = from_path(rel);
    let Some(t) = tags else {
        return path;
    };
    TrackInput {
        title: t.title.clone().unwrap_or(path.title),
        artists: if t.artists.is_empty() {
            path.artists
        } else {
            t.artists.clone()
        },
        album: t.album.clone().unwrap_or(path.album),
        album_artists: t.album_artists.clone(),
        track_no: t.track_no.or(path.track_no),
        disc_no: t.disc_no.or(path.disc_no),
        year: t.year.or(path.year),
        tags: t,
    }
}

/// The album a track belongs to: by MusicBrainz release id, else by album artist (when tagged),
/// album title and year within the album's folder (SPEC 7, scan step 3).
pub fn album_key(rel: &str, t: &TrackInput) -> String {
    if let Some(mb) = &t.tags.mb_release {
        return format!("mb:{}", mb.to_lowercase());
    }
    let aa = t
        .album_artists
        .first()
        .map(|c| normalize(&c.name))
        .unwrap_or_default();
    format!(
        "dir:{}|{}|{}|{}",
        album_folder(rel),
        aa,
        normalize(&t.album),
        t.year.map(|y| y.to_string()).unwrap_or_default()
    )
}

/// A file the library knows: (id, size, mtime, fingerprint).
type Known = (i64, i64, i64, Option<Vec<u8>>);

/// (path, size, mtime, fingerprint, tags or None when they could not be read)
type Read = (String, u64, i64, Option<Vec<u8>>, Option<TrackTags>);

pub async fn scan_music_source(
    lib: &Library,
    src: &dyn Source,
    source_id: i64,
    root: &VPath,
) -> Result<MusicReport, ScanError> {
    let (walked, sidecars) = walk(src, root).await?;
    let existing: HashMap<String, Known> = lib
        .read(move |c| {
            let mut st = c.prepare(
                "SELECT path, id, coalesce(size, -1), coalesce(mtime, -1), fingerprint FROM file WHERE source_id = ?1",
            )?;
            let rows = st.query_map([source_id], |r| {
                Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await?;
    if walked.is_empty() && !existing.is_empty() {
        return Err(ScanError::LooksUnreachable);
    }
    let mut report = MusicReport {
        files_seen: walked.len(),
        ..Default::default()
    };
    let mut unchanged = Vec::new();
    let mut to_read: Vec<(&Walked, Option<i64>)> = Vec::new();
    let mut seen = HashSet::new();
    for w in &walked {
        seen.insert(w.rel.as_str());
        match existing.get(&w.rel) {
            Some((id, size, mtime, _)) if *size == w.size as i64 && *mtime == w.mtime => {
                unchanged.push(*id)
            }
            Some((id, ..)) => to_read.push((w, Some(*id))),
            None => to_read.push((w, None)),
        }
    }
    // Read tags (and fingerprints of new files), a few files at a time.
    let mut reads: Vec<(Option<i64>, Read)> = Vec::with_capacity(to_read.len());
    for chunk in to_read.chunks(READ_AHEAD) {
        let batch = chunk.iter().map(|(w, id)| read_one(src, w, *id));
        reads.extend(futures_util::future::join_all(batch).await);
    }
    report.unreadable = reads.iter().filter(|(_, r)| r.4.is_none()).count();
    let missing: Vec<(String, i64, Option<Vec<u8>>)> = existing
        .iter()
        .filter(|(p, _)| !seen.contains(p.as_str()))
        .map(|(p, (id, _, _, fp))| (p.clone(), *id, fp.clone()))
        .collect();
    let scan_id = now_ms();
    let sidecars: HashMap<String, VPath> = sidecars;
    let (new_albums, covers, counts) = lib
        .write(move |c| {
            let tx = c.transaction()?;
            let exists: i64 = tx.query_row("SELECT count(*) FROM source WHERE id = ?1", [source_id], |r| r.get(0))?;
            if exists == 0 {
                return Ok((vec![], vec![], (0, 0, 0, 0)));
            }
            for id in &unchanged {
                tx.execute("UPDATE file SET last_seen_scan = ?2 WHERE id = ?1", params![id, scan_id])?;
            }
            // Moves: a new path with the fingerprint of a missing file keeps that file (and with
            // it the track, its play count and offline copies).
            let mut moved_from = HashSet::new();
            let mut placed = Vec::new();
            let (mut added, mut changed, mut moved) = (0, 0, 0);
            for (id, (rel, size, mtime, fp, tags)) in reads {
                let mut file_id = id;
                if file_id.is_none()
                    && let Some(fp) = &fp
                        && let Some((old, mid, _)) = missing.iter().find(|(p, _, f)| f.as_ref() == Some(fp) && !moved_from.contains(p)) {
                            moved_from.insert(old.clone());
                            file_id = Some(*mid);
                            moved += 1;
                        }
                let t = input(&rel, tags);
                let file_id = match file_id {
                    Some(fid) => {
                        if id.is_some() {
                            changed += 1;
                        }
                        tx.execute(
                            "UPDATE file SET path = ?2, size = ?3, mtime = ?4, last_seen_scan = ?5, probe_json = NULL WHERE id = ?1",
                            params![fid, rel, size as i64, mtime, scan_id],
                        )?;
                        fid
                    }
                    None => {
                        added += 1;
                        tx.execute(
                            "INSERT INTO file(source_id, path, size, mtime, fingerprint, last_seen_scan) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            params![source_id, rel, size as i64, mtime, fp, scan_id],
                        )?;
                        tx.last_insert_rowid()
                    }
                };
                placed.push((file_id, rel, t));
            }
            let mut removed = 0;
            // Issue #150: what the library keeps says where its original was.
            let leaving: Vec<i64> = missing
                .iter()
                .filter(|(p, ..)| !moved_from.contains(p))
                .map(|(_, id, _)| *id)
                .collect();
            huehuecoyotl_library::managed::record_gone(&tx, source_id, &leaving, scan_id / 1000)?;
            for (p, id, _) in &missing {
                if !moved_from.contains(p) {
                    tx.execute("DELETE FROM file WHERE id = ?1", [id])?;
                    removed += 1;
                }
            }
            let mut new_albums = Vec::new();
            let mut touched: HashSet<i64> = HashSet::new();
            let mut covers: HashMap<i64, Cover> = HashMap::new();
            for (file_id, rel, t) in &placed {
                let (album, created) = place(&tx, source_id, file_id, rel, t, scan_id)?;
                if created {
                    new_albums.push(album);
                }
                touched.insert(album);
                if let std::collections::hash_map::Entry::Vacant(e) = covers.entry(album) {
                    if let Some(img) = &t.tags.cover {
                        e.insert(Cover::Embedded(img.clone()));
                    } else if let Some(p) = sidecars.get(album_folder(rel)).or_else(|| sidecars.get(parent(rel))) {
                        e.insert(Cover::Sidecar(p.clone()));
                    }
                }
            }
            for album in &touched {
                settle_album(&tx, *album)?;
                versions::settle_title(&tx, *album)?;
            }
            // Issue #63: a folder with the album in several formats holds several versions; copies
            // whose track lists differ are not versions of one album.
            for album in touched.clone() {
                let vs: Vec<i64> = {
                    let mut st = tx.prepare("SELECT id FROM album_version WHERE album_id = ?1")?;
                    st.query_map([album], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
                };
                for v in vs {
                    versions::split_by_format(&tx, v)?;
                }
            }
            for album in touched.clone() {
                for made in huehuecoyotl_library::versions::check_track_lists(&tx, album, scan_id)? {
                    settle_album(&tx, made)?;
                    new_albums.push(made);
                    touched.insert(made);
                }
            }
            prune(&tx)?;
            // Covers only for albums that have none yet (and still exist).
            let mut out = Vec::new();
            for (album, cover) in covers {
                let has: bool = tx.query_row(
                    "SELECT EXISTS (SELECT 1 FROM item WHERE id = ?1)
                       AND NOT EXISTS (SELECT 1 FROM artwork WHERE item_id = ?1 AND role = 'poster_grid' AND local_path IS NOT NULL)",
                    [album],
                    |r| r.get(0),
                )?;
                if has {
                    out.push((album, cover));
                }
            }
            tx.execute("UPDATE source SET last_scan = ?2 WHERE id = ?1", params![source_id, scan_id])?;
            tx.commit()?;
            new_albums.retain(|a| touched.contains(a));
            Ok((new_albums, out, (added, changed, removed, moved)))
        })
        .await?;
    report.new_albums = new_albums;
    report.covers = covers;
    (report.added, report.changed, report.removed, report.moved) = counts;
    tracing::info!(
        source_id,
        files = report.files_seen,
        added = report.added,
        changed = report.changed,
        removed = report.removed,
        moved = report.moved,
        unreadable = report.unreadable,
        "music scan finished"
    );
    Ok(report)
}

async fn read_one(src: &dyn Source, w: &Walked, id: Option<i64>) -> (Option<i64>, Read) {
    // New files need a fingerprint (moves); its two reads also start the tag reading.
    let (fp, seed) = if id.is_none() {
        match fingerprint_with_data(src, &w.path, w.size).await {
            Ok((fp, seed)) => (Some(fp), seed),
            Err(_) => (None, Vec::new()),
        }
    } else {
        (None, Vec::new())
    };
    let tags = match tags::read_seeded(src, &w.path, w.size, seed).await {
        Ok(r) => Some(r.tags),
        Err(e) => {
            tracing::warn!(path = %w.rel, "tags not readable: {e}");
            None
        }
    };
    (id, (w.rel.clone(), w.size, w.mtime, fp, tags))
}

fn insert_item(
    tx: &Transaction,
    kind: &str,
    title: &str,
    year: Option<u16>,
    parent: Option<i64>,
    key: &str,
    now: i64,
) -> rusqlite::Result<i64> {
    tx.execute(
        "INSERT INTO item(kind, parent_id, title, sort_title, year, match_state, added_at, group_key)
         VALUES (?1, ?2, ?3, ?4, ?5, 'unmatched', ?6, ?7)",
        params![kind, parent, title, sort_title(title), year, now, key],
    )?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO item_fts(rowid, title) VALUES (?1, ?2)",
        params![id, title],
    )?;
    Ok(id)
}

/// The artist item for a name (or a MusicBrainz artist id when the tags carry one).
fn artist(tx: &Transaction, name: &str, mbid: Option<&str>, now: i64) -> rusqlite::Result<i64> {
    if let Some(mb) = mbid {
        let found: Option<i64> = tx
            .query_row(
                "SELECT e.item_id FROM external_id e JOIN item i ON i.id = e.item_id
                 WHERE e.provider = 'musicbrainz' AND e.kind = 'artist' AND e.value = ?1 AND i.kind = 'artist'",
                [mb.to_lowercase()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = found {
            return Ok(id);
        }
    }
    // Issue #146: the name without case and accents ("Rosalia" = "Rosalía"), but never an artist
    // that carries another artist ID; a merged-away artist's key leads to the one that stayed.
    let key = huehuecoyotl_library::artists::key(name);
    let mb = mbid.map(str::to_lowercase);
    let found: Option<i64> = tx
        .query_row(
            "SELECT i.id FROM item i WHERE i.kind = 'artist' AND i.group_key = ?1
               AND NOT EXISTS (SELECT 1 FROM external_id e WHERE e.item_id = i.id AND e.provider = 'musicbrainz'
                               AND e.kind = 'artist' AND ?2 IS NOT NULL AND e.value <> ?2)
             ORDER BY i.id LIMIT 1",
            params![key, mb],
            |r| r.get(0),
        )
        .optional()?;
    let found = match found {
        Some(id) => Some(id),
        None if mb.is_none() => tx
            .query_row(
                "SELECT a.item_id FROM group_alias a JOIN item i ON i.id = a.item_id
                 WHERE a.kind = 'artist' AND a.key = ?1",
                [&key],
                |r| r.get(0),
            )
            .optional()?,
        None => None,
    };
    let id = match found {
        Some(id) => id,
        None => {
            let id = insert_item(tx, "artist", name, None, None, &key, now)?;
            // Issue #148: the files' spelling, kept apart from the name shown.
            tx.execute(
                "UPDATE item SET files_title = ?2 WHERE id = ?1",
                params![id, name],
            )?;
            id
        }
    };
    if let Some(mb) = mb {
        tx.execute(
            "INSERT INTO external_id(item_id, provider, kind, value)
             SELECT ?1, 'musicbrainz', 'artist', ?2 WHERE NOT EXISTS (
               SELECT 1 FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = 'artist')",
            params![id, mb],
        )?;
    }
    Ok(id)
}

fn set_credits(
    tx: &Transaction,
    item: i64,
    role: &str,
    credits: &[Credit],
    mbids: &[String],
    now: i64,
) -> rusqlite::Result<Vec<i64>> {
    tx.execute(
        "DELETE FROM credit WHERE item_id = ?1 AND role = ?2",
        params![item, role],
    )?;
    let mut ids = Vec::new();
    for (i, c) in credits.iter().enumerate() {
        // Ids pair with names only when there are as many of both.
        let mb = (mbids.len() == credits.len()).then(|| mbids[i].as_str());
        let a = artist(tx, &c.name, mb, now)?;
        tx.execute(
            "INSERT OR REPLACE INTO credit(item_id, artist_id, role, position, join_phrase) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![item, a, role, i as i64, c.join],
        )?;
        ids.push(a);
    }
    Ok(ids)
}

/// Put one file's track into its album version and album (issue #63). Returns (album, whether
/// the album is new).
fn place(
    tx: &Transaction,
    source_id: i64,
    file_id: &i64,
    rel: &str,
    t: &TrackInput,
    now: i64,
) -> rusqlite::Result<(i64, bool)> {
    let tg = &t.tags;
    let title = if t.album.is_empty() {
        "Unknown album".to_string()
    } else {
        t.album.clone()
    };
    // The version: this copy of the album (its folder on this source), keyed as 2.0 keyed albums.
    let vkey = versions::version_key(source_id, &album_key(rel, t));
    let mut created = false;
    // The folder's copy in this file's format, when the folder holds the album in several formats
    // (split by an earlier scan); else the folder's copy.
    let fmt = format!(
        "{vkey}|{}",
        versions::format_key(
            &tg.audio.codec,
            tg.audio.sample_rate,
            tg.audio.bit_depth.map(u32::from)
        )
    );
    let found: Option<(i64, i64)> = tx
        .query_row(
            "SELECT id, album_id FROM album_version WHERE key IN (?1, ?2) ORDER BY key = ?2 DESC LIMIT 1",
            params![vkey, fmt],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (version, album) = match found {
        Some(v) => {
            // Issue #147: tags that now name a release, for a copy without one; the reconcile
            // after the scan acts on it. A release chosen by a match is not overwritten.
            if let Some(r) = &tg.mb_release {
                tx.execute(
                    "UPDATE album_version SET release_id = ?2, release_group = coalesce(?3, release_group) WHERE id = ?1 AND release_id IS NULL",
                    params![
                        v.0,
                        r.to_lowercase(),
                        tg.mb_release_group.as_ref().map(|g| g.to_lowercase())
                    ],
                )?;
            }
            v
        }
        None => {
            // A new copy joins the album of its release (by key, or a match that named it),
            // else the album with its tags. Another release of the group is another album.
            let identity = versions::album_identity(tg.mb_release.as_deref(), &album_key(rel, t));
            let album = match tx
                .query_row(
                    "SELECT id FROM item WHERE kind = 'album' AND locked = 0 AND (group_key = ?1 OR id IN
                       (SELECT item_id FROM external_id WHERE provider = 'musicbrainz' AND kind = 'release' AND value = ?2))
                     ORDER BY id LIMIT 1",
                    params![identity, tg.mb_release.as_ref().map(|r| r.to_lowercase())],
                    |r| r.get(0),
                )
                .optional()?
            {
                Some(id) => id,
                None => {
                    created = true;
                    insert_item(tx, "album", &title, t.year, None, &identity, now)?
                }
            };
            tx.execute(
                "INSERT INTO album_version(album_id, key, source_id, title, year, release_id, release_group)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    album,
                    vkey,
                    source_id,
                    title,
                    t.year,
                    tg.mb_release.as_ref().map(|r| r.to_lowercase()),
                    tg.mb_release_group.as_ref().map(|g| g.to_lowercase())
                ],
            )?;
            (tx.last_insert_rowid(), album)
        }
    };
    if !t.album_artists.is_empty() {
        set_credits(
            tx,
            album,
            "album_artist",
            &t.album_artists,
            &tg.mb_album_artists,
            now,
        )?;
    }
    if let Some(mb) = &tg.mb_release {
        tx.execute(
            "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value)
             SELECT ?1, 'musicbrainz', 'release', ?2
             WHERE NOT EXISTS (SELECT 1 FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = 'release')",
            params![album, mb.to_lowercase()],
        )?;
    }
    if let Some(mb) = &tg.mb_release_group {
        tx.execute(
            "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value)
             SELECT ?1, 'musicbrainz', 'release_group', ?2
             WHERE NOT EXISTS (SELECT 1 FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = 'release_group')",
            params![album, mb.to_lowercase()],
        )?;
    }
    tx.execute(
        "INSERT INTO music(item_id, release_date, release_type, compilation, genre, disc_total, rg_album_gain, rg_album_peak)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(item_id) DO UPDATE SET
           release_date = coalesce(music.release_date, excluded.release_date),
           release_type = coalesce(music.release_type, excluded.release_type),
           compilation = max(music.compilation, excluded.compilation),
           genre = coalesce(music.genre, excluded.genre),
           disc_total = max(coalesce(music.disc_total, 0), coalesce(excluded.disc_total, 0)),
           rg_album_gain = coalesce(excluded.rg_album_gain, music.rg_album_gain),
           rg_album_peak = coalesce(excluded.rg_album_peak, music.rg_album_peak)",
        params![album, tg.date, tg.release_type, tg.compilation, tg.genre, tg.disc_total, tg.rg_album_gain, tg.rg_album_peak],
    )?;
    if t.year.is_some() {
        tx.execute(
            "UPDATE item SET year = coalesce(year, ?2) WHERE id = ?1",
            params![album, t.year],
        )?;
    }
    // The track: the album's track with this disc and title (another version, or a replaced
    // file, has it already); else the file's own track when no other file shares it (play
    // counts stay); else a new one.
    let tkey = versions::track_key(
        i64::from(t.disc_no.unwrap_or(1)),
        &t.title,
        t.track_no.map(i64::from),
    );
    let (same, tkey) = versions::find_track(tx, album, version, &tkey, t.track_no.map(i64::from))
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let own: Option<i64> = tx
        .query_row(
            "SELECT f.item_id FROM file f JOIN item i ON i.id = f.item_id
             WHERE f.id = ?1 AND i.kind = 'track'
               AND NOT EXISTS (SELECT 1 FROM file o WHERE o.item_id = f.item_id AND o.id != f.id)",
            [file_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let track = match (same, own) {
        (Some(id), _) => id,
        (None, Some(id)) => {
            tx.execute(
                "UPDATE item SET parent_id = ?2, title = ?3, sort_title = ?4, year = ?5, group_key = ?6 WHERE id = ?1",
                params![id, album, t.title, sort_title(&t.title), t.year, tkey],
            )?;
            tx.execute(
                "UPDATE item_fts SET title = ?2 WHERE rowid = ?1",
                params![id, t.title],
            )?;
            id
        }
        (None, None) => insert_item(tx, "track", &t.title, t.year, Some(album), &tkey, now)?,
    };
    tx.execute(
        "UPDATE file SET item_id = ?2, version_id = ?3 WHERE id = ?1",
        params![file_id, track, version],
    )?;
    // Disc and track also in the item's season and episode columns: ordered child queries (the
    // album's tracks) sort by them, as for a season's episodes.
    tx.execute(
        "UPDATE item SET season_no = ?2, episode_no = ?3 WHERE id = ?1",
        params![track, t.disc_no.unwrap_or(1), t.track_no],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO music(item_id, disc_no, track_no, track_total, release_date, genre, rg_track_gain, rg_track_peak)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![track, t.disc_no, t.track_no, tg.track_total, tg.date, tg.genre, tg.rg_track_gain, tg.rg_track_peak],
    )?;
    let artists = if t.artists.is_empty() {
        &t.album_artists
    } else {
        &t.artists
    };
    set_credits(tx, track, "artist", artists, &tg.mb_artists, now)?;
    if let Some(mb) = &tg.mb_recording {
        tx.execute("DELETE FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = 'recording'", [track])?;
        tx.execute(
            "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value) VALUES (?1, 'musicbrainz', 'recording', ?2)",
            params![track, mb.to_lowercase()],
        )?;
    }
    let a = &tg.audio;
    tx.execute(
        "INSERT OR REPLACE INTO file_audio(file_id, codec, sample_rate, bit_depth, channels, lossless, bitrate, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            file_id,
            (!a.codec.is_empty()).then_some(&a.codec),
            a.sample_rate,
            a.bit_depth,
            a.channels,
            a.lossless,
            a.bitrate,
            (a.duration_ms > 0).then_some(a.duration_ms as i64)
        ],
    )?;
    Ok((album, created))
}

/// An album's artist (its `parent_id`): the first album artist; without one, the artist of all
/// its tracks when they share one, else "Various Artists" (a compilation).
fn settle_album(tx: &Transaction, album: i64) -> rusqlite::Result<()> {
    let tagged: Option<i64> = tx
        .query_row(
            "SELECT artist_id FROM credit WHERE item_id = ?1 AND role = 'album_artist' ORDER BY position LIMIT 1",
            [album],
            |r| r.get(0),
        )
        .optional()?;
    let artist = match tagged {
        Some(a) => a,
        None => {
            let firsts: Vec<i64> = {
                let mut st = tx.prepare(
                    "SELECT DISTINCT c.artist_id FROM item t JOIN credit c ON c.item_id = t.id AND c.role = 'artist' AND c.position = 0
                     WHERE t.parent_id = ?1 AND t.kind = 'track'",
                )?;
                st.query_map([album], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            };
            match firsts.as_slice() {
                [one] => *one,
                [] => artist(tx, "Unknown artist", None, now_ms())?,
                _ => {
                    tx.execute(
                        "UPDATE music SET compilation = 1 WHERE item_id = ?1",
                        [album],
                    )?;
                    artist(tx, VARIOUS_ARTISTS, None, now_ms())?
                }
            }
        }
    };
    tx.execute(
        "UPDATE item SET parent_id = ?2 WHERE id = ?1",
        params![album, artist],
    )?;
    Ok(())
}

/// Music items no file needs any more. Video kinds are not touched.
fn prune(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "DELETE FROM item WHERE kind = 'track' AND NOT EXISTS (SELECT 1 FROM file f WHERE f.item_id = item.id);
         DELETE FROM album_version WHERE NOT EXISTS (SELECT 1 FROM file f WHERE f.version_id = album_version.id);
         DELETE FROM item WHERE kind = 'album' AND NOT EXISTS (SELECT 1 FROM item t WHERE t.parent_id = item.id);
         DELETE FROM item WHERE kind = 'artist'
           AND NOT EXISTS (SELECT 1 FROM item a WHERE a.parent_id = item.id)
           AND NOT EXISTS (SELECT 1 FROM credit c WHERE c.artist_id = item.id);
         DELETE FROM item_fts WHERE rowid NOT IN (SELECT id FROM item);",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real-world folder and file names of untagged music (issue #7): what the path says.
    #[test]
    fn names_from_paths() {
        // (path, artist, album, year, disc, track, title)
        type Case = (
            &'static str,
            &'static str,
            &'static str,
            Option<u16>,
            Option<u32>,
            Option<u32>,
            &'static str,
        );
        let corpus: &[Case] = &[
            (
                "Pink Floyd/The Wall (1979)/01 - In the Flesh.flac",
                "Pink Floyd",
                "The Wall",
                Some(1979),
                None,
                Some(1),
                "In the Flesh",
            ),
            (
                "Pink Floyd/The Wall (1979)/CD2/03 - Comfortably Numb.flac",
                "Pink Floyd",
                "The Wall",
                Some(1979),
                None,
                Some(3),
                "Comfortably Numb",
            ),
            (
                "Radiohead - OK Computer/02. Paranoid Android.mp3",
                "Radiohead",
                "OK Computer",
                None,
                None,
                Some(2),
                "Paranoid Android",
            ),
            (
                "Various/Now 42/07 Artist X - Song Y.mp3",
                "Artist X",
                "Now 42",
                None,
                None,
                Some(7),
                "Song Y",
            ),
            (
                "Miles Davis/[1959] Kind of Blue/1-02 Freddie Freeloader.flac",
                "Miles Davis",
                "Kind of Blue",
                Some(1959),
                Some(1),
                Some(2),
                "Freddie Freeloader",
            ),
            (
                "Bowie/1977 - Low/04_Always Crashing in the Same Car.ogg",
                "Bowie",
                "Low",
                Some(1977),
                None,
                Some(4),
                "Always Crashing in the Same Car",
            ),
            (
                "Van Halen/1984/1984.flac",
                "Van Halen",
                "1984",
                None,
                None,
                None,
                "1984",
            ),
            ("Loose Track.mp3", "", "", None, None, None, "Loose Track"),
            (
                "Artist - Title.mp3",
                "Artist",
                "",
                None,
                None,
                None,
                "Title",
            ),
            (
                "Björk/Homogenic (1997)/01 Hunter.m4a",
                "Björk",
                "Homogenic",
                Some(1997),
                None,
                Some(1),
                "Hunter",
            ),
        ];
        for (path, artist, album, year, disc, track, title) in corpus {
            let t = from_path(path);
            let got_artist = t.artists.first().map(|c| c.name.as_str()).unwrap_or("");
            assert_eq!(
                (
                    got_artist,
                    t.album.as_str(),
                    t.year,
                    t.disc_no,
                    t.track_no,
                    t.title.as_str()
                ),
                (*artist, *album, *year, *disc, *track, *title),
                "{path}"
            );
        }
    }

    #[test]
    fn disc_folders_belong_to_the_album() {
        assert_eq!(album_folder("Artist/Album/CD1/01.flac"), "Artist/Album");
        assert_eq!(album_folder("Artist/Album/Disc 2/01.flac"), "Artist/Album");
        assert_eq!(album_folder("Artist/Album/01.flac"), "Artist/Album");
        assert_eq!(album_folder("Artist/Cdx/01.flac"), "Artist/Cdx");
        assert_eq!(album_folder("01.flac"), "");
    }
}
