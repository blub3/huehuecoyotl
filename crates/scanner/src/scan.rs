//! Scan pipeline steps 1–3 (SPEC 7): enumerate a source, diff against the `file` table by path,
//! size and modification time, and group files into items. Matching (step 4) and metadata
//! (step 5) run later as jobs on the items this returns.

use std::collections::{HashMap, HashSet};

use huehuecoyotl_library::merge;
use huehuecoyotl_library::rusqlite::{self, Connection, OptionalExtension, Transaction, params};
use huehuecoyotl_library::{DbError, Library};
use huehuecoyotl_vfs::{MediaKind, Source, VPath, VfsError, media_kind};

use crate::parse::{Kind, Parsed, parse};

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error(transparent)]
    Vfs(#[from] VfsError),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("library folder looks empty or unreachable; keeping the existing entries")]
    LooksUnreachable,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScanReport {
    pub files_seen: usize,
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
    pub moved: usize,
    pub extras_skipped: usize,
    /// Existing files moved to a different item because the parser reads them differently now.
    pub regrouped: usize,
    /// New movies and shows that need matching against the metadata provider.
    pub new_items: Vec<i64>,
}

const SKIP_DIRS: &[&str] = &[
    "@eadir",
    "#recycle",
    "$recycle.bin",
    "system volume information",
    "lost+found",
    ".trash",
];
const FP_CHUNK: u32 = 64 * 1024;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// FNV-1a: stable across Rust versions (unlike `DefaultHasher`), plenty for move detection.
fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// size + hash of the first and last 64 KiB (SPEC 7).
pub async fn fingerprint(src: &dyn Source, path: &VPath, size: u64) -> Result<Vec<u8>, VfsError> {
    let head = src.read_at(path, 0, FP_CHUNK).await?;
    let tail_off = size.saturating_sub(u64::from(FP_CHUNK));
    let tail = src.read_at(path, tail_off, FP_CHUNK).await?;
    let mut out = Vec::with_capacity(24);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&fnv1a(&head).to_le_bytes());
    out.extend_from_slice(&fnv1a(&tail).to_le_bytes());
    Ok(out)
}

/// Lower-case alphanumerics and single spaces: the key that makes "The.Matrix" and
/// "the matrix" the same title.
pub fn normalize(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Sort without leading articles: "The Matrix" sorts under M.
pub fn sort_title(title: &str) -> String {
    let l = title.trim().to_lowercase();
    for a in [
        "the ", "a ", "an ", "der ", "die ", "das ", "le ", "la ", "les ", "el ",
    ] {
        if let Some(rest) = l.strip_prefix(a) {
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    l
}

struct Walked {
    rel: String,
    path: VPath,
    size: u64,
    mtime: i64,
}

async fn walk(src: &dyn Source, root: &VPath) -> Result<Vec<Walked>, VfsError> {
    let prefix = format!("{}/", root.as_str().trim_end_matches('/'));
    let mut out = Vec::new();
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
            } else if media_kind(&e.name) == MediaKind::Video {
                let rel = e
                    .path
                    .as_str()
                    .strip_prefix(&prefix)
                    .unwrap_or(e.path.as_str())
                    .to_string();
                out.push(Walked {
                    rel,
                    path: e.path,
                    size: e.size,
                    mtime: e.mtime,
                });
            }
        }
    }
    Ok(out)
}

/// (relative path, size, mtime, fingerprint, parsed name)
type NewFile = (String, u64, i64, Option<Vec<u8>>, Parsed);

struct Existing {
    id: i64,
    size: i64,
    mtime: i64,
    fingerprint: Option<Vec<u8>>,
    /// The file's movie item may be regrouped when the parser now reads the name differently:
    /// not matched, not locked, no play state.
    revisable: bool,
    item_key: Option<String>,
}

/// Grouping key of a show: the normalised title without a leading article, so "The Office"
/// and "Office" meet. The year is compared separately ([`find_show`]); a country suffix such as
/// "US" stays in the key, since it names a different show.
pub fn show_key(title: &str) -> String {
    let n = normalize(title);
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = n.strip_prefix(article) {
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    n
}

/// The show for a grouping key, found by its key or an alias left by a merge. With several
/// candidates the one with the same year wins; an unknown year on either side matches.
pub fn find_show(c: &Connection, key: &str, year: Option<u16>) -> rusqlite::Result<Option<i64>> {
    let mut st = c.prepare(
        "SELECT id, year FROM item WHERE kind = 'show' AND parent_id IS NULL AND group_key = ?1
         UNION
         SELECT i.id, i.year FROM group_alias a JOIN item i ON i.id = a.item_id
         WHERE a.kind = 'show' AND a.key = ?1
         ORDER BY id",
    )?;
    let found: Vec<(i64, Option<u16>)> = st
        .query_map([key], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let same = found.iter().find(|(_, y)| year.is_some() && *y == year);
    let open = found.iter().find(|(_, y)| y.is_none() || year.is_none());
    Ok(same.or(open).map(|(id, _)| *id))
}

/// Grouping key of a movie (matches `item.group_key`).
fn movie_key(p: &Parsed) -> String {
    format!(
        "{}|{}",
        normalize(&p.title),
        p.year.map(|y| y.to_string()).unwrap_or_default()
    )
}

/// Scan one library folder. `source_id` must be a source whose files are stored relative to
/// `root`.
pub async fn scan_source(
    lib: &Library,
    src: &dyn Source,
    source_id: i64,
    root: &VPath,
) -> Result<ScanReport, ScanError> {
    let walked = walk(src, root).await?;
    let existing: HashMap<String, Existing> = lib
        .read(move |c| {
            let mut st = c.prepare(
                "SELECT f.id, f.path, f.size, f.mtime, f.fingerprint,
                        i.kind = 'movie' AND i.locked = 0 AND coalesce(i.match_state, 'unmatched') != 'matched'
                          AND NOT EXISTS (SELECT 1 FROM playstate p WHERE p.item_id = i.id),
                        i.group_key
                 FROM file f LEFT JOIN item i ON i.id = f.item_id WHERE f.source_id = ?1",
            )?;
            let rows = st.query_map([source_id], |r| {
                Ok((
                    r.get::<_, String>(1)?,
                    Existing {
                        id: r.get(0)?,
                        size: r.get::<_, Option<i64>>(2)?.unwrap_or(-1),
                        mtime: r.get::<_, Option<i64>>(3)?.unwrap_or(-1),
                        fingerprint: r.get(4)?,
                        revisable: r.get::<_, Option<bool>>(5)?.unwrap_or(false),
                        item_key: r.get(6)?,
                    },
                ))
            })?;
            Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
        })
        .await?;

    if walked.is_empty() && !existing.is_empty() {
        return Err(ScanError::LooksUnreachable);
    }

    let mut report = ScanReport {
        files_seen: walked.len(),
        ..Default::default()
    };
    let mut unchanged = Vec::new();
    let mut changed = Vec::new();
    let mut fresh = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    // Files whose name the parser now reads differently (better rules for discs, extras, ...).
    let mut regroup: Vec<(i64, Parsed)> = Vec::new();
    let mut dropped: Vec<i64> = Vec::new();
    for w in &walked {
        seen.insert(&w.rel);
        match existing.get(&w.rel) {
            Some(e) => {
                if e.size == w.size as i64 && e.mtime == w.mtime {
                    unchanged.push(e.id);
                } else {
                    changed.push((e.id, w.size as i64, w.mtime));
                }
                if e.revisable {
                    let p = parse(&w.rel);
                    match p.kind {
                        Kind::Extra => dropped.push(e.id),
                        Kind::Episode => regroup.push((e.id, p)),
                        Kind::Movie if e.item_key.as_deref() != Some(movie_key(&p).as_str()) => {
                            regroup.push((e.id, p))
                        }
                        Kind::Movie => {}
                    }
                }
            }
            None => fresh.push(w),
        }
    }
    let missing: Vec<(&String, &Existing)> = existing
        .iter()
        .filter(|(p, _)| !seen.contains(p.as_str()))
        .collect();

    // Fingerprints for new files, then move detection against the missing ones.
    let mut new_files = Vec::new();
    for w in fresh {
        let parsed = parse(&w.rel);
        if parsed.kind == Kind::Extra {
            report.extras_skipped += 1;
            continue;
        }
        let fp = fingerprint(src, &w.path, w.size).await.ok();
        new_files.push((w, parsed, fp));
    }
    let mut moves = Vec::new();
    let mut moved_from = HashSet::new();
    new_files.retain(|(w, _, fp)| {
        let hit = fp.as_ref().and_then(|fp| {
            missing
                .iter()
                .find(|(p, e)| e.fingerprint.as_ref() == Some(fp) && !moved_from.contains(*p))
        });
        match hit {
            Some((p, e)) => {
                moved_from.insert(*p);
                moves.push((e.id, w.rel.clone(), w.mtime));
                false
            }
            None => true,
        }
    });
    let mut gone: Vec<i64> = missing
        .iter()
        .filter(|(p, _)| !moved_from.contains(*p))
        .map(|(_, e)| e.id)
        .collect();
    report.extras_skipped += dropped.len();
    report.regrouped = regroup.len();
    gone.extend(dropped);

    report.changed = changed.len();
    report.moved = moves.len();
    report.removed = gone.len();
    report.added = new_files.len();

    let to_insert: Vec<NewFile> = new_files
        .into_iter()
        .map(|(w, p, fp)| (w.rel.clone(), w.size, w.mtime, fp, p))
        .collect();
    let scan_id = now_ms();
    let new_items = lib
        .write(move |c| {
            let tx = c.transaction()?;
            // The folder may have been removed while it was being scanned.
            let exists: i64 = tx.query_row("SELECT count(*) FROM source WHERE id = ?1", [source_id], |r| r.get(0))?;
            if exists == 0 {
                return Ok(Vec::new());
            }
            for id in &unchanged {
                tx.execute("UPDATE file SET last_seen_scan = ?2 WHERE id = ?1", params![id, scan_id])?;
            }
            for (id, size, mtime) in &changed {
                tx.execute(
                    "UPDATE file SET size = ?2, mtime = ?3, last_seen_scan = ?4, probe_json = NULL WHERE id = ?1",
                    params![id, size, mtime, scan_id],
                )?;
            }
            for (id, rel, mtime) in &moves {
                tx.execute("UPDATE file SET path = ?2, mtime = ?3, last_seen_scan = ?4 WHERE id = ?1", params![id, rel, mtime, scan_id])?;
            }
            for id in &gone {
                tx.execute("DELETE FROM file WHERE id = ?1", [id])?;
            }
            let mut new_items = Vec::new();
            for (file_id, parsed) in &regroup {
                let (item, created) = group(&tx, parsed, scan_id)?;
                if let Some(id) = created {
                    new_items.push(id);
                }
                tx.execute("UPDATE file SET item_id = ?2 WHERE id = ?1", params![file_id, item])?;
            }
            for (rel, size, mtime, fp, parsed) in &to_insert {
                let (item, created) = group(&tx, parsed, scan_id)?;
                if let Some(id) = created {
                    new_items.push(id);
                }
                tx.execute(
                    "INSERT INTO file(source_id, path, size, mtime, fingerprint, item_id, last_seen_scan)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![source_id, rel, *size as i64, mtime, fp, item, scan_id],
                )?;
            }
            prune(&tx)?;
            tx.execute("UPDATE source SET last_scan = ?2 WHERE id = ?1", params![source_id, scan_id])?;
            tx.commit()?;
            Ok(new_items)
        })
        .await?;
    report.new_items = new_items;
    tracing::info!(source_id, ?report, "scan finished");
    Ok(report)
}

#[allow(clippy::too_many_arguments)] // internal helper mirroring the item columns
fn insert_item(
    tx: &Transaction,
    kind: &str,
    title: &str,
    year: Option<u16>,
    parent: Option<i64>,
    season: Option<u16>,
    episode: Option<u16>,
    key: Option<&str>,
    now: i64,
) -> rusqlite::Result<i64> {
    tx.execute(
        "INSERT INTO item(kind, parent_id, title, sort_title, year, season_no, episode_no, match_state, added_at, group_key)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'unmatched', ?8, ?9)",
        params![kind, parent, title, sort_title(title), year, season, episode, now, key],
    )?;
    let id = tx.last_insert_rowid();
    if kind == "movie" || kind == "show" {
        tx.execute(
            "INSERT INTO item_fts(rowid, title) VALUES (?1, ?2)",
            params![id, title],
        )?;
    }
    Ok(id)
}

/// Find or create the item a parsed file belongs to. Returns (item for the file, newly created
/// movie or show that needs matching).
fn group(tx: &Transaction, p: &Parsed, now: i64) -> rusqlite::Result<(i64, Option<i64>)> {
    if p.kind == Kind::Movie {
        let key = format!(
            "{}|{}",
            normalize(&p.title),
            p.year.map(|y| y.to_string()).unwrap_or_default()
        );
        let found: Option<i64> = tx
            .query_row(
                "SELECT id FROM item WHERE kind = 'movie' AND group_key = ?1",
                [&key],
                |r| r.get(0),
            )
            .optional()?;
        return Ok(match found {
            Some(id) => (id, None),
            None => {
                let id = insert_item(
                    tx,
                    "movie",
                    &p.title,
                    p.year,
                    None,
                    None,
                    None,
                    Some(&key),
                    now,
                )?;
                (id, Some(id))
            }
        });
    }
    // Episode: show (issue #32: named by the show folder or the release name, never by a
    // season folder), season, episode.
    let show_name = p.show_title.as_deref().unwrap_or(&p.title);
    let show_year = p.show_year.or(p.year);
    let key = show_key(show_name);
    let mut created = None;
    let show: i64 = match find_show(tx, &key, show_year)? {
        Some(id) => id,
        None => {
            let id = insert_item(
                tx,
                "show",
                show_name,
                show_year,
                None,
                None,
                None,
                Some(&key),
                now,
            )?;
            created = Some(id);
            id
        }
    };
    let season_no = p.season.unwrap_or(1);
    let season: i64 = match tx
        .query_row(
            "SELECT id FROM item WHERE kind = 'season' AND parent_id = ?1 AND season_no = ?2",
            params![show, season_no],
            |r| r.get(0),
        )
        .optional()?
    {
        Some(id) => id,
        None => {
            let t = if season_no == 0 {
                "Specials".to_string()
            } else {
                format!("Season {season_no}")
            };
            insert_item(
                tx,
                "season",
                &t,
                None,
                Some(show),
                Some(season_no),
                None,
                None,
                now,
            )?
        }
    };
    let ep_no = p.episodes.first().copied().unwrap_or(0);
    let episode: i64 = match tx
        .query_row(
            "SELECT id FROM item WHERE kind = 'episode' AND parent_id = ?1 AND episode_no = ?2",
            params![season, ep_no],
            |r| r.get(0),
        )
        .optional()?
    {
        Some(id) => id,
        None => {
            let t = match p.episodes.as_slice() {
                [a, .., b] => format!("Episode {a}–{b}"),
                _ => format!("Episode {ep_no}"),
            };
            insert_item(
                tx,
                "episode",
                &t,
                None,
                Some(season),
                Some(season_no),
                Some(ep_no),
                None,
                now,
            )?
        }
    };
    // A season or episode only TMDB knew so far now has a file.
    tx.execute(
        "UPDATE item SET virtual = 0 WHERE id IN (?1, ?2) AND virtual != 0",
        params![season, episode],
    )?;
    tx.execute(
        "UPDATE item SET added_at = ?2 WHERE id = ?1",
        params![show, now],
    )?;
    Ok((episode, created))
}

/// What [`regroup_shows`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RegroupReport {
    /// Shows that got the key the current parser derives (kept their match and history).
    pub rekeyed: usize,
    /// Shows merged into another one with the same key or TMDB id.
    pub merged: usize,
    /// Unmatched shows whose title changed: match them again.
    pub rematch: Vec<i64>,
}

/// Offline repair for libraries scanned before issue #32: derive each show's key again from
/// its episodes' file paths with the current parser, then rename the show to that key or merge
/// it into the show that already has it. Matches, artwork and watch state are kept; a user's
/// locked choice is not touched. No file is read.
pub fn regroup_shows(c: &Connection) -> Result<RegroupReport, DbError> {
    let mut report = RegroupReport::default();
    let shows: Vec<(i64, Option<String>, bool, bool)> = {
        let mut st = c.prepare(
            "SELECT id, group_key, locked != 0, coalesce(match_state, '') = 'matched'
             FROM item WHERE kind = 'show' AND parent_id IS NULL ORDER BY id",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for (show, old_key, locked, matched) in shows {
        if locked {
            continue;
        }
        // Still there? (It may have been merged into an earlier show in this loop.)
        let exists: bool =
            c.query_row("SELECT count(*) > 0 FROM item WHERE id = ?1", [show], |r| {
                r.get(0)
            })?;
        if !exists {
            continue;
        }
        let paths: Vec<String> = {
            let mut st = c.prepare(
                "SELECT f.path FROM item se JOIN item e ON e.parent_id = se.id
                 JOIN file f ON f.item_id = e.id WHERE se.parent_id = ?1",
            )?;
            let rows = st.query_map([show], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        // The most common (key, title, year) among the show's episodes.
        let mut votes: HashMap<String, (usize, String, Option<u16>)> = HashMap::new();
        for path in &paths {
            let p = parse(path);
            if p.kind != Kind::Episode {
                continue;
            }
            let title = p.show_title.clone().unwrap_or_else(|| p.title.clone());
            let e = votes
                .entry(show_key(&title))
                .or_insert((0, title, p.show_year.or(p.year)));
            e.0 += 1;
        }
        let Some((key, (_, title, year))) = votes.into_iter().max_by_key(|(_, v)| v.0) else {
            continue;
        };
        if old_key.as_deref() == Some(key.as_str()) {
            continue;
        }
        // Never merge two shows matched to different TMDB ids: same-named shows from different
        // years are different shows, and a wrong match is for the review list to fix.
        let other = find_show(c, &key, year)?.filter(|o| *o != show).filter(|o| {
            let ids: (Option<i64>, Option<i64>) = c
                .query_row(
                    "SELECT (SELECT tmdb_id FROM item WHERE id = ?1), (SELECT tmdb_id FROM item WHERE id = ?2)",
                    params![show, o],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap_or((None, None));
            !matches!(ids, (Some(a), Some(b)) if a != b)
        });
        // This show takes the new key; its old key stays as an alias.
        let rekey = |id: i64| -> rusqlite::Result<()> {
            c.execute(
                "INSERT OR IGNORE INTO group_alias(kind, key, item_id)
                 SELECT 'show', group_key, id FROM item WHERE id = ?1 AND group_key IS NOT NULL",
                [id],
            )?;
            c.execute(
                "UPDATE item SET group_key = ?2 WHERE id = ?1",
                params![id, key],
            )?;
            c.execute(
                "DELETE FROM group_alias WHERE kind = 'show' AND key = ?1",
                [&key],
            )?;
            Ok(())
        };
        let survivor = match other {
            None => {
                rekey(show)?;
                report.rekeyed += 1;
                show
            }
            Some(o) => {
                let keep = merge::preferred(c, show, o)?;
                let drop = if keep == show { o } else { show };
                if keep == show {
                    rekey(show)?;
                }
                merge::merge_into(c, keep, drop)?;
                report.merged += 1;
                keep
            }
        };
        // An unmatched show gets the clean title (not "Show S01 2160p") and is matched again.
        let survivor_matched: bool = c.query_row(
            "SELECT coalesce(match_state, '') = 'matched' OR locked != 0 FROM item WHERE id = ?1",
            [survivor],
            |r| r.get(0),
        )?;
        if !survivor_matched && !(matched && survivor == show) {
            c.execute(
                "UPDATE item SET title = ?2, sort_title = ?3, year = coalesce(?4, year),
                        match_state = 'unmatched'
                 WHERE id = ?1",
                params![survivor, title, sort_title(&title), year],
            )?;
            c.execute(
                "UPDATE item_fts SET title = ?2 WHERE rowid = ?1",
                params![survivor, title],
            )?;
            c.execute("DELETE FROM match_candidate WHERE item_id = ?1", [survivor])?;
            if !report.rematch.contains(&survivor) {
                report.rematch.push(survivor);
            }
        }
    }
    report.merged += merge::merge_tmdb_duplicates(c)?;
    report.rematch.retain(|id| {
        c.query_row("SELECT count(*) > 0 FROM item WHERE id = ?1", [id], |r| {
            r.get::<_, bool>(0)
        })
        .unwrap_or(false)
    });
    tracing::info!(?report, "shows regrouped");
    Ok(report)
}

/// Remove items that lost their last file. Items with play state are kept (a drive may come
/// back), but stay invisible in grids, which only list items with files. Seasons and episodes
/// only TMDB knows (`virtual`) stay while their show has anything from the scanner.
fn prune(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "DELETE FROM item WHERE kind IN ('movie','episode') AND virtual = 0
           AND NOT EXISTS (SELECT 1 FROM file f WHERE f.item_id = item.id)
           AND NOT EXISTS (SELECT 1 FROM playstate p WHERE p.item_id = item.id);
         DELETE FROM item WHERE kind = 'show'
           AND NOT EXISTS (SELECT 1 FROM item s JOIN item e ON e.parent_id = s.id
                           WHERE s.parent_id = item.id AND e.virtual = 0);
         DELETE FROM item WHERE kind = 'season' AND virtual = 0
           AND NOT EXISTS (SELECT 1 FROM item e WHERE e.parent_id = item.id);
         DELETE FROM item_fts WHERE rowid NOT IN (SELECT id FROM item);",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use huehuecoyotl_vfs::LocalSource;

    fn touch(root: &std::path::Path, rel: &str, bytes: usize) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![rel.len() as u8; bytes]).unwrap();
    }

    async fn setup() -> (tempfile::TempDir, Library, i64, VPath) {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::open(&d.path().join("db/library.db")).unwrap();
        let media = d.path().join("media");
        std::fs::create_dir_all(&media).unwrap();
        let root = VPath::new(media.to_string_lossy());
        let r = root.to_string();
        let sid = lib
            .write(move |c| {
                c.execute(
                    "INSERT INTO source(kind, name, root) VALUES ('local', 'Media', ?1)",
                    [r],
                )?;
                Ok(c.last_insert_rowid())
            })
            .await
            .unwrap();
        (d, lib, sid, root)
    }

    async fn count(lib: &Library, sql: &'static str) -> i64 {
        lib.read(move |c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn scan_groups_diffs_and_detects_moves() {
        let (d, lib, sid, root) = setup().await;
        let m = d.path().join("media");
        touch(&m, "Movies/Alien (1979)/Alien.1979.1080p.mkv", 10);
        touch(&m, "Movies/Alien (1979)/Alien.1979.2160p.mkv", 11); // second version, same item
        touch(&m, "Movies/Alien (1979)/Extras/Making of.mkv", 5);
        touch(&m, "Shows/Firefly/Season 1/Firefly.S01E01.mkv", 20);
        touch(&m, "Shows/Firefly/Season 1/Firefly.S01E02.mkv", 21);
        touch(&m, "Shows/Firefly/Season 2/02 - Something.mkv", 22);
        touch(&m, "notes.txt", 1);

        let r = scan_source(&lib, &LocalSource, sid, &root).await.unwrap();
        assert_eq!((r.files_seen, r.added, r.extras_skipped), (6, 5, 1));
        assert_eq!(r.new_items.len(), 2, "one movie and one show to match");
        assert_eq!(
            count(&lib, "SELECT count(*) FROM item WHERE kind='movie'").await,
            1
        );
        assert_eq!(
            count(
                &lib,
                "SELECT count(*) FROM file WHERE item_id = (SELECT id FROM item WHERE kind='movie')"
            )
            .await,
            2
        );
        assert_eq!(
            count(&lib, "SELECT count(*) FROM item WHERE kind='season'").await,
            2
        );
        assert_eq!(
            count(&lib, "SELECT count(*) FROM item WHERE kind='episode'").await,
            3
        );
        assert_eq!(
            count(
                &lib,
                "SELECT count(*) FROM item_fts WHERE item_fts MATCH 'firefly'"
            )
            .await,
            1
        );

        // Rescan without changes: nothing new.
        let r = scan_source(&lib, &LocalSource, sid, &root).await.unwrap();
        assert_eq!(
            (r.added, r.changed, r.removed, r.new_items.len()),
            (0, 0, 0, 0)
        );

        // Watch an episode, then rename it: the move keeps the item and its play state.
        let ep: i64 = lib
            .read(|c| {
                Ok(c.query_row(
                    "SELECT item_id FROM file WHERE path LIKE '%S01E01%'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .await
            .unwrap();
        lib.write(move |c| huehuecoyotl_library::queries::set_watched(c, ep, true))
            .await
            .unwrap();
        std::fs::rename(
            m.join("Shows/Firefly/Season 1/Firefly.S01E01.mkv"),
            m.join("Shows/Firefly/Season 1/Firefly - S01E01 - Serenity.mkv"),
        )
        .unwrap();
        std::fs::remove_file(m.join("Shows/Firefly/Season 2/02 - Something.mkv")).unwrap();
        let r = scan_source(&lib, &LocalSource, sid, &root).await.unwrap();
        assert_eq!((r.moved, r.removed, r.added), (1, 1, 0));
        let still: i64 = lib
            .read(move |c| {
                Ok(c.query_row(
                    "SELECT watched FROM playstate WHERE item_id = ?1",
                    [ep],
                    |r| r.get(0),
                )?)
            })
            .await
            .unwrap();
        assert_eq!(still, 1);
        assert_eq!(
            count(&lib, "SELECT count(*) FROM item WHERE kind='season'").await,
            1,
            "empty season pruned"
        );
    }

    /// Issue #32: every common layout gives exactly one show, named after the show (not after
    /// a season or release folder), with the right seasons and episodes.
    #[tokio::test]
    async fn every_layout_groups_into_one_show() {
        let (d, lib, sid, root) = setup().await;
        let m = d.path().join("media");
        for f in [
            // Show (Year)/Season 01/Show - S01E01 - Title
            "Alpha Show (2022)/Season 01/Alpha Show - S01E01 - Pilot.mkv",
            "Alpha Show (2022)/Season 01/Alpha Show - S01E02 - Second.mkv",
            "Alpha Show (2022)/Season 02/Alpha Show - S02E01 - Back.mkv",
            // Show/S01/Show.S01E01
            "Beta Show/S01/Beta.Show.S01E01.1080p.mkv",
            "Beta Show/S01/Beta.Show.S01E02.1080p.mkv",
            // Show/release folder per season/file
            "Gamma Show/Gamma.Show.S01.1080p.WEB/Gamma.Show.S01E01.1080p.WEB.mkv",
            "Gamma Show/Gamma.Show.S01.1080p.WEB/Gamma.Show.S01E02.1080p.WEB.mkv",
            "Gamma Show/Gamma.Show.S02.2160p.WEB-DL/Gamma.Show.S02E01.2160p.WEB-DL.mkv",
            // flat in a generic folder
            "Shows/Delta.Show.S01E01.mkv",
            "Shows/Delta.Show.S01E02.mkv",
            // specials next to a season
            "Epsilon Show/Specials/Epsilon Show - S00E01.mkv",
            "Epsilon Show/Season 1/Epsilon Show - S01E01.mkv",
            // release folder per season at the library root, no show folder
            "Zeta.Show.S01.2160p.WEB-DL.DV.HDR.H265/Zeta.Show.S01E01.2160p.WEB-DL.mkv",
            "Zeta.Show.S01.2160p.WEB-DL.DV.HDR.H265/Zeta.Show.S01E02.2160p.WEB-DL.mkv",
            "Zeta.Show.S02.2160p.WEB-DL.DV.HDR.H265/Zeta.Show.S02E01.2160p.WEB-DL.mkv",
            // one folder per episode
            "Eta.Show.S02E01.First.Title.2160p.WEB/Eta.Show.S02E01.First.Title.2160p.WEB.mkv",
            "Eta.Show.S02E02.Second.Title.2160p.WEB/Eta.Show.S02E02.Second.Title.2160p.WEB.mkv",
            // release folder with a season folder inside, plus a plain show folder
            "Theta.Show.S01.1080p.BluRay/Season 1/Theta.Show.S01E01.mkv",
            "Theta Show (2019)/Season 2/Theta.Show.S02E01.mkv",
            // a category folder that is not the show
            "Kids/Mu.Show.S01E01.mkv",
            "Kids/Mu.Show.S01E02.mkv",
        ] {
            touch(&m, f, 10 + f.len());
        }
        // One show spread over two sources (two shares).
        let m2 = d.path().join("media2");
        touch(&m2, "Iota Show/Season 2/Iota.Show.S02E01.mkv", 30);
        touch(&m, "Iota Show/Season 1/Iota.Show.S01E01.mkv", 31);
        let root2 = VPath::new(m2.to_string_lossy());
        let r2 = root2.to_string();
        let sid2 = lib
            .write(move |c| {
                c.execute(
                    "INSERT INTO source(kind, name, root) VALUES ('local', 'Second', ?1)",
                    [r2],
                )?;
                Ok(c.last_insert_rowid())
            })
            .await
            .unwrap();
        scan_source(&lib, &LocalSource, sid, &root).await.unwrap();
        scan_source(&lib, &LocalSource, sid2, &root2).await.unwrap();

        // title -> sorted [(season, episodes)]
        let shows: Vec<(String, Vec<(i64, i64)>)> = lib
            .read(|c| {
                let mut st = c.prepare(
                    "SELECT s.id, s.title, se.season_no, count(e.id) FROM item s
                     JOIN item se ON se.parent_id = s.id AND se.kind = 'season'
                     JOIN item e ON e.parent_id = se.id AND e.kind = 'episode'
                     WHERE s.kind = 'show' GROUP BY s.id, se.id ORDER BY s.title, se.season_no",
                )?;
                let rows = st.query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                })?;
                // One entry per show item, so a duplicate shows up as a second entry.
                type Show = (i64, String, Vec<(i64, i64)>);
                let mut out: Vec<Show> = Vec::new();
                for row in rows {
                    let (id, t, s, n) = row?;
                    match out.last_mut() {
                        Some((lid, _, v)) if *lid == id => v.push((s, n)),
                        _ => out.push((id, t, vec![(s, n)])),
                    }
                }
                Ok(out.into_iter().map(|(_, t, v)| (t, v)).collect())
            })
            .await
            .unwrap();
        let want: Vec<(&str, Vec<(i64, i64)>)> = vec![
            ("Alpha Show", vec![(1, 2), (2, 1)]),
            ("Beta Show", vec![(1, 2)]),
            ("Delta Show", vec![(1, 2)]),
            ("Epsilon Show", vec![(0, 1), (1, 1)]),
            ("Eta Show", vec![(2, 2)]),
            ("Gamma Show", vec![(1, 2), (2, 1)]),
            ("Iota Show", vec![(1, 1), (2, 1)]),
            ("Mu Show", vec![(1, 2)]),
            ("Theta Show", vec![(1, 1), (2, 1)]),
            ("Zeta Show", vec![(1, 2), (2, 1)]),
        ];
        let got: Vec<(&str, Vec<(i64, i64)>)> =
            shows.iter().map(|(t, v)| (t.as_str(), v.clone())).collect();
        assert_eq!(got, want);
        assert_eq!(
            count(&lib, "SELECT count(*) FROM item WHERE kind = 'show'").await,
            10,
            "no empty or duplicate shows"
        );
    }

    /// A library as 1.1.0 left it: one show per release folder and per episode folder, one of
    /// them matched and watched. The offline regroup repairs it without losing anything.
    #[tokio::test]
    async fn regroup_repairs_shows_split_by_older_versions() {
        let (_d, lib, _sid, _root) = setup().await;
        let report = lib
            .write(|c| {
                c.execute_batch(
                    "INSERT INTO source(id, kind, name, root) VALUES (5, 'local', 'Old', '/old');
                     -- Zeta: one show per season release folder; season 1 matched (TMDB 99), watched
                     INSERT INTO item(id, kind, title, group_key, tmdb_id, match_state, year) VALUES
                       (100, 'show', 'Zeta Show', 'zeta show s01 2160p web dl dv hdr h265', 99, 'matched', 2020),
                       (200, 'show', 'Zeta Show S02', 'zeta show s02 2160p web dl dv hdr h265', NULL, 'needs_review', NULL);
                     INSERT INTO item(id, kind, parent_id, season_no) VALUES (101, 'season', 100, 1), (201, 'season', 200, 2);
                     INSERT INTO item(id, kind, parent_id, episode_no) VALUES (102, 'episode', 101, 1), (202, 'episode', 201, 1);
                     INSERT INTO file(source_id, path, item_id) VALUES
                       (5, 'Zeta.Show.S01.2160p.WEB-DL.DV.HDR.H265/Zeta.Show.S01E01.2160p.WEB-DL.mkv', 102),
                       (5, 'Zeta.Show.S02.2160p.WEB-DL.DV.HDR.H265/Zeta.Show.S02E01.2160p.WEB-DL.mkv', 202);
                     INSERT INTO playstate(item_id, position_ms, watched, updated_at) VALUES (102, 0, 1, 10), (202, 60000, 0, 20);
                     INSERT INTO match_candidate(item_id, provider_id, title) VALUES (200, 99, 'Zeta Show');
                     -- Eta: one unmatched show per episode folder
                     INSERT INTO item(id, kind, title, group_key, match_state) VALUES
                       (300, 'show', 'Eta Show', 'eta show s02e01 first title 2160p web', 'unmatched'),
                       (400, 'show', 'Eta Show', 'eta show s02e02 second title 2160p web', 'unmatched');
                     INSERT INTO item(id, kind, parent_id, season_no) VALUES (301, 'season', 300, 2), (401, 'season', 400, 2);
                     INSERT INTO item(id, kind, parent_id, episode_no) VALUES (302, 'episode', 301, 1), (402, 'episode', 401, 2);
                     INSERT INTO file(source_id, path, item_id) VALUES
                       (5, 'Eta.Show.S02E01.First.Title.2160p.WEB/Eta.Show.S02E01.First.Title.2160p.WEB.mkv', 302),
                       (5, 'Eta.Show.S02E02.Second.Title.2160p.WEB/Eta.Show.S02E02.Second.Title.2160p.WEB.mkv', 402);",
                )?;
                regroup_shows(c)
            })
            .await
            .unwrap();
        assert_eq!(report.merged, 2, "{report:?}");
        assert_eq!(
            count(&lib, "SELECT count(*) FROM item WHERE kind = 'show'").await,
            2
        );
        // Zeta: the matched item survives with its match and both seasons' watch state.
        assert_eq!(
            count(
                &lib,
                "SELECT count(*) FROM item WHERE kind = 'season' AND parent_id = 100"
            )
            .await,
            2
        );
        assert_eq!(
            count(&lib, "SELECT tmdb_id FROM item WHERE id = 100").await,
            99
        );
        assert_eq!(count(&lib, "SELECT count(*) FROM playstate").await, 2);
        assert_eq!(count(&lib, "SELECT count(*) FROM match_candidate").await, 0);
        // Eta: one unmatched show with a clean title, queued for matching once.
        assert_eq!(
            count(&lib, "SELECT count(*) FROM item WHERE kind = 'episode' AND parent_id IN (SELECT id FROM item WHERE parent_id = 300)").await,
            2
        );
        assert_eq!(report.rematch, vec![300]);
        // A rescan finds the repaired shows through their keys and aliases: nothing new.
        let again = lib.write(|c| regroup_shows(c)).await.unwrap();
        assert_eq!((again.merged, again.rekeyed), (0, 0));
        let found = lib
            .read(|c| {
                Ok(find_show(
                    c,
                    "zeta show s02 2160p web dl dv hdr h265",
                    None,
                )?)
            })
            .await
            .unwrap();
        assert_eq!(found, Some(100), "old keys lead to the survivor");

        // Same title, different TMDB shows (e.g. a remake): never merged.
        let r = lib
            .write(|c| {
                c.execute_batch(
                    "INSERT INTO item(id, kind, title, group_key, tmdb_id, match_state) VALUES
                       (500, 'show', 'Nu Show', 'nu show 1978', 1, 'matched'),
                       (600, 'show', 'Nu Show', 'nu show s01 2160p', 2, 'matched');
                     INSERT INTO item(id, kind, parent_id, season_no) VALUES (501, 'season', 500, 1), (601, 'season', 600, 1);
                     INSERT INTO item(id, kind, parent_id, episode_no) VALUES (502, 'episode', 501, 1), (602, 'episode', 601, 1);
                     INSERT INTO file(source_id, path, item_id) VALUES
                       (5, 'Nu Show/Season 1/Nu.Show.S01E01.mkv', 502),
                       (5, 'Nu.Show.S01.2160p/Nu.Show.S01E01.2160p.mkv', 602);",
                )?;
                regroup_shows(c)
            })
            .await
            .unwrap();
        assert_eq!(r.merged, 0, "{r:?}");
        assert_eq!(
            count(
                &lib,
                "SELECT count(*) FROM item WHERE kind = 'show' AND tmdb_id IN (1, 2)"
            )
            .await,
            2
        );
    }

    /// Episodes only TMDB knows survive rescans, become real when their file appears, and go
    /// with the show once nothing of it is on disk any more.
    #[tokio::test]
    async fn virtual_episodes_survive_rescans_and_turn_real() {
        let (d, lib, sid, root) = setup().await;
        let m = d.path().join("media");
        touch(&m, "Firefly/Season 1/Firefly.S01E01.mkv", 10);
        scan_source(&lib, &LocalSource, sid, &root).await.unwrap();
        let virtual_ep: i64 = lib
            .write(|c| {
                let season: i64 =
                    c.query_row("SELECT id FROM item WHERE kind = 'season'", [], |r| {
                        r.get(0)
                    })?;
                huehuecoyotl_library::catalog::ensure_episode(c, season, 2, "The Train Job")
            })
            .await
            .unwrap();
        scan_source(&lib, &LocalSource, sid, &root).await.unwrap();
        assert_eq!(
            count(&lib, "SELECT count(*) FROM item WHERE kind = 'episode'").await,
            2,
            "kept by the rescan"
        );
        touch(&m, "Firefly/Season 1/Firefly.S01E02.mkv", 11);
        scan_source(&lib, &LocalSource, sid, &root).await.unwrap();
        let (item, virt): (i64, i64) = lib
            .read(|c| {
                Ok(c.query_row(
                    "SELECT i.id, i.virtual FROM file f JOIN item i ON i.id = f.item_id WHERE f.path LIKE '%S01E02%'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .await
            .unwrap();
        assert_eq!(
            (item, virt),
            (virtual_ep, 0),
            "the file joins the TMDB episode"
        );
        std::fs::remove_dir_all(m.join("Firefly")).unwrap();
        touch(&m, "Other (2001).mkv", 3); // keep the folder from looking unreachable
        scan_source(&lib, &LocalSource, sid, &root).await.unwrap();
        assert_eq!(
            count(
                &lib,
                "SELECT count(*) FROM item WHERE kind IN ('show', 'season', 'episode')"
            )
            .await,
            0
        );
    }

    #[tokio::test]
    async fn better_parser_rules_regroup_unmatched_items() {
        let (d, lib, sid, root) = setup().await;
        let m = d.path().join("media");
        for n in ["00800.m2ts", "00801.m2ts"] {
            touch(&m, &format!("Prey.2022.2160p.UHD/BDMV/STREAM/{n}"), 30);
        }
        // Simulate rows from an older parser: one bogus movie per stream.
        scan_source(&lib, &LocalSource, sid, &root).await.unwrap();
        lib.write(|c| {
            c.execute_batch(
                "UPDATE item SET group_key = 'stale|' || id, title = 'old';
                 INSERT INTO item(id, kind, title, group_key) VALUES (900, 'movie', '00801', '00801|');
                 UPDATE file SET item_id = 900 WHERE path LIKE '%00801%';",
            )?;
            Ok(())
        })
        .await
        .unwrap();
        let r = scan_source(&lib, &LocalSource, sid, &root).await.unwrap();
        assert_eq!(r.regrouped, 2);
        assert_eq!(
            count(&lib, "SELECT count(*) FROM item WHERE kind = 'movie'").await,
            1,
            "one movie per disc"
        );
        let title: String = lib
            .read(|c| {
                Ok(
                    c.query_row("SELECT title FROM item WHERE kind='movie'", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .await
            .unwrap();
        assert_eq!(title, "Prey");
    }

    #[tokio::test]
    async fn unreachable_root_keeps_everything() {
        let (d, lib, sid, root) = setup().await;
        touch(&d.path().join("media"), "Up (2009).mkv", 3);
        scan_source(&lib, &LocalSource, sid, &root).await.unwrap();
        std::fs::remove_file(d.path().join("media/Up (2009).mkv")).unwrap();
        assert!(matches!(
            scan_source(&lib, &LocalSource, sid, &root).await,
            Err(ScanError::LooksUnreachable)
        ));
        std::fs::remove_dir(d.path().join("media")).unwrap();
        assert!(matches!(
            scan_source(&lib, &LocalSource, sid, &root).await,
            Err(ScanError::Vfs(VfsError::NotFound(_)))
        ));
        assert_eq!(count(&lib, "SELECT count(*) FROM file").await, 1);
    }

    #[test]
    fn keys() {
        assert_eq!(normalize("The.Matrix: Reloaded!"), "the matrix reloaded");
        assert_eq!(sort_title("The Matrix"), "matrix");
        assert_eq!(sort_title("A"), "a");
    }
}
