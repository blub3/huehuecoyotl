//! Scan pipeline steps 1–3 (SPEC 7): enumerate a source, diff against the `file` table by path,
//! size and modification time, and group files into items. Matching (step 4) and metadata
//! (step 5) run later as jobs on the items this returns.

use std::collections::{HashMap, HashSet};

use huehuecoyotl_library::rusqlite::{self, OptionalExtension, Transaction, params};
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
    // Episode: show (by folder when there is one), season, episode.
    let show_name = p.show_folder.as_deref().unwrap_or(&p.title);
    let key = normalize(show_name);
    let mut created = None;
    let show: i64 = match tx
        .query_row(
            "SELECT id FROM item WHERE kind = 'show' AND group_key = ?1",
            [&key],
            |r| r.get(0),
        )
        .optional()?
    {
        Some(id) => id,
        None => {
            let id = insert_item(
                tx,
                "show",
                &p.title,
                p.year,
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
    tx.execute(
        "UPDATE item SET added_at = ?2 WHERE id = ?1",
        params![show, now],
    )?;
    Ok((episode, created))
}

/// Remove items that lost their last file. Items with play state are kept (a drive may come
/// back), but stay invisible in grids, which only list items with files.
fn prune(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "DELETE FROM item WHERE kind IN ('movie','episode')
           AND NOT EXISTS (SELECT 1 FROM file f WHERE f.item_id = item.id)
           AND NOT EXISTS (SELECT 1 FROM playstate p WHERE p.item_id = item.id);
         DELETE FROM item WHERE kind = 'season'
           AND NOT EXISTS (SELECT 1 FROM item e WHERE e.parent_id = item.id);
         DELETE FROM item WHERE kind = 'show'
           AND NOT EXISTS (SELECT 1 FROM item s WHERE s.parent_id = item.id);
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
