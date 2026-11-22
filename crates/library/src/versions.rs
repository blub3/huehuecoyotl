//! Album versions and editions (issue #63, SPEC 7 "Music library").
//!
//! A **version** is one copy of an album: the files of one folder on one source, keyed exactly as
//! 2.0 keyed an album (`v:<source>:<2.0 key>`). An **album** is what its versions have in common:
//! the MusicBrainz release (`rel:`, since issue #147), else album artist, title and year (`tag:`),
//! where copies count as versions of one album only when their track lists are the same. The
//! release group links the albums that are *editions* of one another. A **track** is one song of an album, keyed by disc and
//! title (`t:<disc>|<title>`), with a file in every version that has it; a version can also hold
//! two files of one track (a replaced file).
//!
//! The scanner places files into versions and versions into albums; this module merges what
//! belongs together (migration 9, migration 16, after scans and after online matching),
//! splits a version off ("This is a different album"), lists an album's versions and picks the
//! one to play.

use std::collections::{BTreeSet, HashMap};

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;
use crate::text::normalize;

// ---- keys -----------------------------------------------------------------------------------

/// A version's key from its source and the 2.0 album key (`mb:<release>` or
/// `dir:<folder>|<album artist>|<album>|<year>`).
pub fn version_key(source_id: i64, album_key: &str) -> String {
    format!("v:{source_id}:{album_key}")
}

/// A track's key within its album: disc and title, or the track number without a title.
pub fn track_key(disc: i64, title: &str, number: Option<i64>) -> String {
    let t = normalize(title);
    if t.is_empty() {
        format!("t:{disc}|#{}", number.unwrap_or(0))
    } else {
        format!("t:{disc}|{t}")
    }
}

/// An album's key as migration 9 made it (issue #63): by release group first. Migration 16
/// re-keys these, so upgrades from before 2.1 pass through the same states as everybody else.
fn identity_9(group: Option<&str>, release: Option<&str>, album_key: &str) -> String {
    match group.filter(|g| !g.is_empty()) {
        Some(g) => format!("rg:{}", g.to_lowercase()),
        None => album_identity(release, album_key),
    }
}

/// An album's key from its release or its 2.0 key (issue #147): the release, else album artist,
/// title and year. Editions in one release group are separate albums.
pub fn album_identity(release: Option<&str>, album_key: &str) -> String {
    if let Some(r) = release.filter(|r| !r.is_empty()) {
        return format!("rel:{}", r.to_lowercase());
    }
    if let Some(r) = album_key.strip_prefix("mb:") {
        return format!("rel:{r}");
    }
    // dir:<folder>|<album artist>|<album>|<year>: the folder goes.
    let rest = album_key.strip_prefix("dir:").unwrap_or(album_key);
    let parts: Vec<&str> = rest.rsplitn(4, '|').collect();
    match parts.as_slice() {
        [year, album, artist, _folder] => format!("tag:{artist}|{album}|{year}"),
        _ => format!("tag:{rest}"),
    }
}

// ---- schema (migration 9) -------------------------------------------------------------------

const SCHEMA: &str = r#"
CREATE TABLE album_version(id INTEGER PRIMARY KEY,
                           album_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                           key TEXT NOT NULL UNIQUE,
                           source_id INTEGER NOT NULL REFERENCES source(id) ON DELETE CASCADE,
                           title TEXT NOT NULL DEFAULT '',
                           year INTEGER,
                           release_id TEXT,
                           release_group TEXT,
                           split INTEGER NOT NULL DEFAULT 0);
CREATE INDEX album_version_album ON album_version(album_id);
ALTER TABLE file ADD COLUMN version_id INTEGER REFERENCES album_version(id) ON DELETE SET NULL;
CREATE INDEX file_version ON file(version_id);
CREATE INDEX item_parent_key ON item(parent_id, group_key);
"#;

fn external(c: &Connection, item: i64, kind: &str) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT value FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = ?2 ORDER BY value LIMIT 1",
        params![item, kind],
        |r| r.get(0),
    )
    .optional()?)
}

/// Migration 9: every 2.0 album becomes one version of an album keyed by its identity; tracks
/// get their disc-and-title keys; then copies of one album are merged into it, with their play
/// counts, the queue and saved copies kept.
pub fn migration_9(c: &Connection) -> Result<()> {
    c.execute_batch(SCHEMA)?;
    let albums: Vec<(i64, String, String, Option<i64>)> = {
        let mut st = c.prepare(
            "SELECT id, coalesce(group_key, ''), coalesce(title, ''), year FROM item WHERE kind = 'album' ORDER BY id",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (album, key, title, year) in &albums {
        let source: Option<i64> = c
            .query_row(
                "SELECT f.source_id FROM item t JOIN file f ON f.item_id = t.id WHERE t.parent_id = ?1 LIMIT 1",
                [album],
                |r| r.get(0),
            )
            .optional()?;
        let Some(source) = source else { continue };
        let release = external(c, *album, "release")?;
        let group = external(c, *album, "release_group")?;
        c.execute(
            "INSERT INTO album_version(album_id, key, source_id, title, year, release_id, release_group)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![album, version_key(source, key), source, title, year, release, group],
        )?;
        let version = c.last_insert_rowid();
        c.execute(
            "UPDATE file SET version_id = ?2 WHERE item_id IN (SELECT id FROM item WHERE parent_id = ?1 AND kind = 'track')",
            params![album, version],
        )?;
        let locked: i64 = c.query_row("SELECT locked FROM item WHERE id = ?1", [album], |r| {
            r.get(0)
        })?;
        let identity = if locked != 0 {
            format!("split:{version}")
        } else {
            identity_9(group.as_deref(), release.as_deref(), key)
        };
        c.execute(
            "UPDATE item SET group_key = ?2 WHERE id = ?1",
            params![album, identity],
        )?;
    }
    // Tracks: keyed by disc and title.
    let tracks: Vec<(i64, i64, String, Option<i64>)> = {
        let mut st = c.prepare(
            "SELECT id, coalesce(season_no, 1), coalesce(title, ''), episode_no FROM item WHERE kind = 'track'",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (id, disc, title, no) in tracks {
        c.execute(
            "UPDATE item SET group_key = ?2 WHERE id = ?1",
            params![id, track_key(disc, &title, no)],
        )?;
    }
    // Within an album, tracks with one key are one track (a replaced file).
    for (album, ..) in &albums {
        merge_duplicate_tracks(c, *album)?;
    }
    // Albums with one identity are one album (with the track-list rule for tag: keys).
    let mut by_key: HashMap<String, Vec<i64>> = HashMap::new();
    {
        let mut st =
            c.prepare("SELECT id, group_key FROM item WHERE kind = 'album' ORDER BY id")?;
        for r in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (id, key) = r?;
            by_key.entry(key).or_default().push(id);
        }
    }
    let mut groups: Vec<(String, Vec<i64>)> =
        by_key.into_iter().filter(|(_, v)| v.len() > 1).collect();
    groups.sort();
    for (key, ids) in groups {
        join_group(c, &key, &ids)?;
    }
    // One folder with the album in two formats: two versions.
    let versions: Vec<i64> = {
        let mut st = c.prepare("SELECT id FROM album_version ORDER BY id")?;
        st.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for v in versions {
        split_by_format(c, v)?;
    }
    Ok(())
}

/// Albums that share an identity: one album, except (for tag: keys) those whose track list
/// differs from the first one's, which keep a key of their own.
fn join_group(c: &Connection, key: &str, ids: &[i64]) -> Result<()> {
    let Some((&keep, rest)) = ids.split_first() else {
        return Ok(());
    };
    let first = track_list(c, keep)?;
    for &other in rest {
        if key.starts_with("tag:") && track_list(c, other)? != first {
            c.execute(
                "UPDATE item SET group_key = ?2 WHERE id = ?1",
                params![other, format!("{key}#{other}")],
            )?;
        } else {
            merge_albums(c, keep, other)?;
        }
    }
    Ok(())
}

/// The keys of an album's tracks (its track list).
fn track_list(c: &Connection, album: i64) -> Result<BTreeSet<String>> {
    let mut st = c.prepare_cached(
        "SELECT coalesce(group_key, '') FROM item WHERE parent_id = ?1 AND kind = 'track'",
    )?;
    Ok(st
        .query_map([album], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

/// The keys of the tracks a version has a file of.
fn version_track_list(c: &Connection, version: i64) -> Result<BTreeSet<String>> {
    let mut st = c.prepare_cached(
        "SELECT DISTINCT coalesce(t.group_key, '') FROM file f JOIN item t ON t.id = f.item_id WHERE f.version_id = ?1",
    )?;
    Ok(st
        .query_map([version], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

fn merge_duplicate_tracks(c: &Connection, album: i64) -> Result<()> {
    // One title twice with different numbers (a deluxe edition's acoustic take) is two songs:
    // the lowest number keeps the key, the others get theirs with the number.
    let numbered: Vec<(String, i64, i64)> = {
        let mut st = c.prepare(
            "SELECT group_key, id, coalesce(episode_no, 0) FROM item
             WHERE parent_id = ?1 AND kind = 'track' AND group_key IN (
               SELECT group_key FROM item WHERE parent_id = ?1 AND kind = 'track'
               GROUP BY group_key HAVING count(DISTINCT coalesce(episode_no, 0)) > 1)
             ORDER BY group_key, coalesce(episode_no, 0)",
        )?;
        st.query_map([album], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut lowest: HashMap<String, i64> = HashMap::new();
    for (key, id, no) in numbered {
        let low = *lowest.entry(key.clone()).or_insert(no);
        if no != low {
            c.execute(
                "UPDATE item SET group_key = ?2 WHERE id = ?1",
                params![id, format!("{key}#{no}")],
            )?;
        }
    }
    let dups: Vec<(String, i64)> = {
        let mut st = c.prepare(
            "SELECT group_key, min(id) FROM item WHERE parent_id = ?1 AND kind = 'track' AND group_key IS NOT NULL
             GROUP BY group_key HAVING count(*) > 1",
        )?;
        st.query_map([album], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (key, keep) in dups {
        let others: Vec<i64> = {
            let mut st = c.prepare(
                "SELECT id FROM item WHERE parent_id = ?1 AND kind = 'track' AND group_key = ?2 AND id != ?3",
            )?;
            st.query_map(params![album, key, keep], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        for o in others {
            merge_tracks(c, keep, o)?;
        }
    }
    Ok(())
}

/// The audio format part of a version key: codec, sample rate, bit depth.
pub fn format_key(codec: &str, rate: Option<u32>, depth: Option<u32>) -> String {
    format!("fmt:{}-{}-{}", codec, rate.unwrap_or(0), depth.unwrap_or(0))
}

/// One folder holding the album twice, in two formats (FLAC and MP3 side by side): when every
/// format has the same tracks, each format becomes a version of its own (`<key>|fmt:…`). A
/// replaced file of another format stays a second file of its track. Returns the new versions.
pub fn split_by_format(c: &Connection, version: i64) -> Result<Vec<i64>> {
    let rows: Vec<(i64, String, i64, i64, i64)> = {
        let mut st = c.prepare_cached(
            "SELECT f.id, coalesce(fa.codec, ''), coalesce(fa.sample_rate, 0), coalesce(fa.bit_depth, 0), coalesce(f.item_id, 0)
             FROM file f LEFT JOIN file_audio fa ON fa.file_id = f.id WHERE f.version_id = ?1 ORDER BY f.id",
        )?;
        st.query_map([version], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    // Formats in the order of their first file; the first stays.
    let mut groups: Vec<(String, BTreeSet<i64>, Vec<i64>)> = Vec::new();
    for (file, codec, rate, depth, track) in rows {
        let k = format!("fmt:{codec}-{rate}-{depth}");
        match groups.iter_mut().find(|(g, ..)| *g == k) {
            Some((_, tracks, files)) => {
                tracks.insert(track);
                files.push(file);
            }
            None => groups.push((k, BTreeSet::from([track]), vec![file])),
        }
    }
    if groups.len() < 2 || groups.iter().any(|(_, t, _)| *t != groups[0].1) {
        return Ok(vec![]);
    }
    let (key, album, source, title, year, release, group): (String, i64, i64, String, Option<i64>, Option<String>, Option<String>) =
        c.query_row(
            "SELECT key, album_id, source_id, title, year, release_id, release_group FROM album_version WHERE id = ?1",
            [version],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )?;
    let base = key.split("|fmt:").next().unwrap_or(&key).to_string();
    let mut made = Vec::new();
    for (fmt, _, files) in groups.into_iter().skip(1) {
        let vkey = format!("{base}|{fmt}");
        let existing: Option<i64> = c
            .query_row(
                "SELECT id FROM album_version WHERE key = ?1",
                [&vkey],
                |r| r.get(0),
            )
            .optional()?;
        let v = match existing {
            Some(v) => v,
            None => {
                c.execute(
                    "INSERT INTO album_version(album_id, key, source_id, title, year, release_id, release_group)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![album, vkey, source, title, year, release, group],
                )?;
                c.last_insert_rowid()
            }
        };
        for f in files {
            c.execute(
                "UPDATE file SET version_id = ?2 WHERE id = ?1",
                params![f, v],
            )?;
        }
        made.push(v);
    }
    Ok(made)
}

/// The track a file of `version` belongs to (issue #63), by its title key and number: the
/// album's track with this title and number; else one with this title that the version does not
/// have yet (another copy numbers it differently); else none, with the key a new track gets (the
/// title's, or with the number when the title is taken: two songs of one title).
pub fn find_track(
    c: &Connection,
    album: i64,
    version: i64,
    key: &str,
    number: Option<i64>,
) -> Result<(Option<i64>, String)> {
    let n = number.unwrap_or(0);
    let candidates: Vec<(i64, String, i64, bool)> = {
        let mut st = c.prepare_cached(
            "SELECT t.id, t.group_key, coalesce(t.episode_no, 0),
                    EXISTS (SELECT 1 FROM file f WHERE f.item_id = t.id AND f.version_id = ?3)
             FROM item t WHERE t.parent_id = ?1 AND t.kind = 'track'
               AND (t.group_key = ?2 OR t.group_key LIKE ?2 || '#%') ORDER BY t.id",
        )?;
        st.query_map(params![album, key, version], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    if let Some((id, k, ..)) = candidates.iter().find(|(_, _, no, _)| *no == n) {
        return Ok((Some(*id), k.clone()));
    }
    if let Some((id, k, ..)) = candidates.iter().find(|(_, _, _, here)| !here) {
        return Ok((Some(*id), k.clone()));
    }
    if candidates.is_empty() {
        Ok((None, key.to_string()))
    } else {
        Ok((None, format!("{key}#{n}")))
    }
}

// ---- merges ---------------------------------------------------------------------------------

/// One track from two: the files, the play count (added up), the last play, the queue and the
/// saved copies move to `keep`.
pub fn merge_tracks(c: &Connection, keep: i64, gone: i64) -> Result<()> {
    if keep == gone {
        return Ok(());
    }
    c.execute(
        "UPDATE file SET item_id = ?1 WHERE item_id = ?2",
        params![keep, gone],
    )?;
    c.execute(
        "UPDATE play_queue SET item_id = ?1 WHERE item_id = ?2",
        params![keep, gone],
    )?;
    c.execute(
        "UPDATE local_copy SET item_id = ?1 WHERE item_id = ?2",
        params![keep, gone],
    )?;
    c.execute(
        "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value)
         SELECT ?1, provider, kind, value FROM external_id WHERE item_id = ?2",
        params![keep, gone],
    )?;
    merge_playstate(c, keep, gone)?;
    delete_item(c, gone)
}

fn merge_playstate(c: &Connection, keep: i64, gone: i64) -> Result<()> {
    let has_keep: bool = c.query_row(
        "SELECT EXISTS (SELECT 1 FROM playstate WHERE item_id = ?1)",
        [keep],
        |r| r.get(0),
    )?;
    if has_keep {
        c.execute(
            "UPDATE playstate SET
               play_count = play_count + coalesce((SELECT g.play_count FROM playstate g WHERE g.item_id = ?2 AND g.profile_id = playstate.profile_id), 0),
               last_played_at = max(coalesce(last_played_at, 0), coalesce((SELECT g.last_played_at FROM playstate g WHERE g.item_id = ?2 AND g.profile_id = playstate.profile_id), 0)),
               last_file_id = coalesce(last_file_id, (SELECT g.last_file_id FROM playstate g WHERE g.item_id = ?2 AND g.profile_id = playstate.profile_id))
             WHERE item_id = ?1",
            params![keep, gone],
        )?;
        c.execute(
            "UPDATE playstate SET last_played_at = NULL WHERE item_id = ?1 AND last_played_at = 0",
            [keep],
        )?;
    } else {
        c.execute(
            "UPDATE playstate SET item_id = ?1 WHERE item_id = ?2",
            params![keep, gone],
        )?;
    }
    Ok(())
}

fn delete_item(c: &Connection, id: i64) -> Result<()> {
    c.execute(
        "DELETE FROM job WHERE json_extract(payload_json, '$.item_id') = ?1",
        [id],
    )?;
    c.execute("DELETE FROM item WHERE id = ?1", [id])?;
    c.execute("DELETE FROM item_fts WHERE rowid = ?1", [id])?;
    Ok(())
}

/// One album from two: `gone`'s versions and tracks move to `keep` (tracks with the same key
/// become one), with what `keep` lacks of artwork, ids and play state.
pub fn merge_albums(c: &Connection, keep: i64, gone: i64) -> Result<()> {
    if keep == gone {
        return Ok(());
    }
    c.execute(
        "UPDATE album_version SET album_id = ?1 WHERE album_id = ?2",
        params![keep, gone],
    )?;
    let tracks: Vec<(i64, Option<String>)> = {
        let mut st =
            c.prepare("SELECT id, group_key FROM item WHERE parent_id = ?1 AND kind = 'track'")?;
        st.query_map([gone], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (t, key) in tracks {
        let same: Option<i64> = match &key {
            Some(k) => c
                .query_row(
                    "SELECT id FROM item WHERE parent_id = ?1 AND kind = 'track' AND group_key = ?2 ORDER BY id LIMIT 1",
                    params![keep, k],
                    |r| r.get(0),
                )
                .optional()?,
            None => None,
        };
        match same {
            Some(s) => merge_tracks(c, s, t)?,
            None => {
                c.execute(
                    "UPDATE item SET parent_id = ?1 WHERE id = ?2",
                    params![keep, t],
                )?;
            }
        }
    }
    c.execute(
        "UPDATE artwork SET item_id = ?1 WHERE item_id = ?2 AND role NOT IN (SELECT role FROM artwork WHERE item_id = ?1)",
        params![keep, gone],
    )?;
    c.execute(
        "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value)
         SELECT ?1, provider, kind, value FROM external_id WHERE item_id = ?2",
        params![keep, gone],
    )?;
    c.execute(
        "UPDATE item SET added_at = min(coalesce(added_at, ?3), coalesce((SELECT added_at FROM item WHERE id = ?2), ?3)),
                         year = coalesce(year, (SELECT year FROM item WHERE id = ?2))
         WHERE id = ?1",
        params![keep, gone, i64::MAX],
    )?;
    merge_playstate(c, keep, gone)?;
    c.execute(
        "UPDATE local_copy SET item_id = ?1 WHERE item_id = ?2",
        params![keep, gone],
    )?;
    c.execute(
        "UPDATE play_queue SET item_id = ?1 WHERE item_id = ?2",
        params![keep, gone],
    )?;
    delete_item(c, gone)?;
    settle_title(c, keep)
}

/// After a lookup or a scan gave an album its release: other albums of the same release become
/// its versions, and the album is keyed by it. Editions (another release of the same group) stay
/// apart, and so do albums split by hand. Returns the album that remains (the oldest).
pub fn merge_same_release(c: &Connection, album: i64) -> Result<i64> {
    let mut keep = album;
    let others: Vec<i64> = {
        let mut st = c.prepare(
            "SELECT DISTINCT o.item_id FROM external_id mine
             JOIN external_id o ON o.provider = 'musicbrainz' AND o.kind = 'release' AND o.value = mine.value AND o.item_id != mine.item_id
             JOIN item i ON i.id = o.item_id AND i.kind = 'album' AND i.locked = 0
             JOIN item me ON me.id = mine.item_id AND me.locked = 0
             WHERE mine.item_id = ?1 AND mine.provider = 'musicbrainz' AND mine.kind = 'release'",
        )?;
        st.query_map([keep], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for o in others {
        let (k, g) = if o < keep { (o, keep) } else { (keep, o) };
        merge_albums(c, k, g)?;
        keep = k;
    }
    if let Some(r) = external(c, keep, "release")? {
        c.execute(
            "UPDATE item SET group_key = ?2 WHERE id = ?1 AND locked = 0",
            params![keep, format!("rel:{r}")],
        )?;
        // The match speaks for copies without an ID, so a later scan does not split them off.
        c.execute(
            "UPDATE album_version SET release_id = ?2 WHERE album_id = ?1 AND release_id IS NULL",
            params![keep, r],
        )?;
    }
    Ok(keep)
}

// ---- one album per release (issue #147, migration 16) ---------------------------------------

/// The release a version's own ids name: its tag, else its 2.0 key (`mb:<release>`).
fn version_release(key: &str, release: Option<String>) -> Option<String> {
    release.filter(|r| !r.is_empty()).or_else(|| {
        let rest = key.strip_prefix("v:")?.split_once(':')?.1;
        let id = rest.strip_prefix("mb:")?.split("|fmt:").next()?;
        (!id.is_empty()).then(|| id.to_lowercase())
    })
}

/// The `tag:` key of a version's copy (its 2.0 key without the source and the format).
fn version_tag_key(key: &str) -> String {
    let rest = key
        .strip_prefix("v:")
        .and_then(|k| k.split_once(':'))
        .map_or(key, |(_, k)| k);
    let rest = rest.split("|fmt:").next().unwrap_or(rest);
    album_identity(None, rest)
}

/// Play state, playlist entries and queue entries that name a file follow it to the track it
/// now belongs to. Play state without a file stays where it is, so nothing is counted twice.
fn follow_files(c: &Connection, tracks: &[i64]) -> Result<()> {
    for &t in tracks {
        let moved: Vec<(i64, i64)> = {
            let mut st = c.prepare_cached(
                "SELECT p.profile_id, f.item_id FROM playstate p JOIN file f ON f.id = p.last_file_id
                 WHERE p.item_id = ?1 AND f.item_id IS NOT NULL AND f.item_id != ?1",
            )?;
            st.query_map([t], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        for (profile, to) in moved {
            c.execute(
                "UPDATE playstate SET
                   play_count = play_count + (SELECT g.play_count FROM playstate g WHERE g.item_id = ?1 AND g.profile_id = ?3),
                   last_played_at = max(coalesce(last_played_at, 0), coalesce((SELECT g.last_played_at FROM playstate g WHERE g.item_id = ?1 AND g.profile_id = ?3), 0))
                 WHERE item_id = ?2 AND profile_id = ?3",
                params![t, to, profile],
            )?;
            if c.changes() > 0 {
                c.execute(
                    "DELETE FROM playstate WHERE item_id = ?1 AND profile_id = ?2",
                    params![t, profile],
                )?;
            } else {
                c.execute(
                    "UPDATE playstate SET item_id = ?2 WHERE item_id = ?1 AND profile_id = ?3",
                    params![t, to, profile],
                )?;
            }
        }
        for table in ["playlist_entry", "play_queue"] {
            c.execute(
                &format!(
                    "UPDATE {table} SET item_id = (SELECT f.item_id FROM file f WHERE f.id = {table}.file_id)
                     WHERE item_id = ?1 AND (SELECT f.item_id FROM file f WHERE f.id = {table}.file_id) IS NOT NULL"
                ),
                [t],
            )?;
        }
    }
    Ok(())
}

/// Replace an album's release and release group ids.
fn set_release(c: &Connection, album: i64, release: &str, group: Option<&str>) -> Result<()> {
    c.execute(
        "DELETE FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = 'release' AND value != ?2",
        params![album, release],
    )?;
    c.execute(
        "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value) VALUES (?1, 'musicbrainz', 'release', ?2)",
        params![album, release],
    )?;
    if let Some(g) = group {
        c.execute(
            "DELETE FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = 'release_group' AND value != ?2",
            params![album, g],
        )?;
        c.execute(
            "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value) VALUES (?1, 'musicbrainz', 'release_group', ?2)",
            params![album, g],
        )?;
    }
    c.execute(
        "UPDATE item SET group_key = ?2 WHERE id = ?1",
        params![album, format!("rel:{release}")],
    )?;
    Ok(())
}

/// Versions into one album: the first moves out under `key`, the others join it. The new album
/// counts as added when the old one was.
fn move_group(c: &Connection, from: i64, vs: &[i64], key: &str, now: i64) -> Result<i64> {
    let target = move_version(c, vs[0], key, false, now)?;
    for &v in &vs[1..] {
        let moved = move_version(c, v, &format!("{key}#v{v}"), false, now)?;
        merge_albums(c, target, moved)?;
    }
    c.execute(
        "UPDATE item SET added_at = (SELECT added_at FROM item WHERE id = ?1)
         WHERE id = ?2 OR (parent_id = ?2 AND kind = 'track' AND added_at = ?3)",
        params![from, target, now],
    )?;
    Ok(target)
}

/// One album holding several releases becomes one album per release. The release it was matched
/// to stays; a version without a release ID goes with the release that has its track list, else
/// becomes an album of its own. Returns the albums made.
pub fn split_by_release(c: &Connection, album: i64, now: i64) -> Result<Vec<i64>> {
    let rows: Vec<(i64, String, Option<String>, Option<String>)> = {
        let mut st = c.prepare(
            "SELECT id, key, release_id, release_group FROM album_version WHERE album_id = ?1 ORDER BY id",
        )?;
        st.query_map([album], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    if rows.is_empty() {
        return Ok(vec![]);
    }
    let matched: Vec<String> = {
        let mut st = c.prepare(
            "SELECT value FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = 'release' ORDER BY value",
        )?;
        st.query_map([album], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    // Releases in the order first met, each with its versions.
    let mut releases: Vec<(String, Vec<i64>)> = Vec::new();
    let mut loose: Vec<i64> = Vec::new();
    let mut groups: HashMap<String, String> = HashMap::new();
    for (id, key, release, group) in &rows {
        match version_release(key, release.clone()) {
            Some(r) => {
                if let Some(g) = group {
                    groups.entry(r.clone()).or_insert_with(|| g.clone());
                }
                match releases.iter_mut().find(|(x, _)| *x == r) {
                    Some((_, vs)) => vs.push(*id),
                    None => releases.push((r, vec![*id])),
                }
            }
            None => loose.push(*id),
        }
    }
    // Several matches and no version saying which is which: no release can be named, so the
    // album goes back to its tags and waits for a new match.
    if releases.is_empty() && matched.len() > 1 {
        c.execute(
            "DELETE FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = 'release'",
            [album],
        )?;
        c.execute(
            "UPDATE item SET group_key = ?2, match_state = 'unmatched' WHERE id = ?1",
            params![album, version_tag_key(&rows[0].1)],
        )?;
        return check_track_lists(c, album, now);
    }
    if releases.is_empty() {
        // One match (or none): every copy is that release, or the album has no release at all.
        match matched.first() {
            Some(r) => set_release(c, album, r, None)?,
            None => {
                let key: String = c.query_row(
                    "SELECT coalesce(group_key, '') FROM item WHERE id = ?1",
                    [album],
                    |r| r.get(0),
                )?;
                if key.starts_with("rg:") {
                    c.execute(
                        "UPDATE item SET group_key = ?2 WHERE id = ?1",
                        params![album, version_tag_key(&rows[0].1)],
                    )?;
                    return check_track_lists(c, album, now);
                }
            }
        }
        return Ok(vec![]);
    }
    // A version without an ID goes with the release whose track list it has.
    let mut own: Vec<i64> = Vec::new();
    let lists: Vec<BTreeSet<String>> = releases
        .iter()
        .map(|(_, vs)| version_track_list(c, vs[0]))
        .collect::<Result<_>>()?;
    for v in loose {
        let list = version_track_list(c, v)?;
        // Prefer the release the album was matched to, when both have this list.
        let hit = (0..releases.len())
            .filter(|&i| lists[i] == list)
            .max_by_key(|&i| matched.contains(&releases[i].0));
        match hit {
            Some(i) => releases[i].1.push(v),
            None => own.push(v),
        }
    }
    // The matched release stays in this album.
    let home = releases
        .iter()
        .position(|(r, _)| matched.contains(r))
        .unwrap_or(0);
    let (home_release, home_versions) = releases.remove(home);
    if releases.is_empty() && own.is_empty() {
        let group = groups.get(&home_release).cloned();
        set_release(c, album, &home_release, group.as_deref())?;
        return Ok(vec![]);
    }
    let tracks: Vec<i64> = {
        let mut st = c.prepare("SELECT id FROM item WHERE parent_id = ?1 AND kind = 'track'")?;
        st.query_map([album], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let album_group = external(c, album, "release_group")?;
    let mut made = Vec::new();
    for (release, vs) in &releases {
        let new = move_group(c, album, vs, &format!("rel:{release}"), now)?;
        let group = groups.get(release).cloned().or(album_group.clone());
        set_release(c, new, release, group.as_deref())?;
        made.push(new);
    }
    for v in own {
        let key = rows.iter().find(|r| r.0 == v).map_or("", |r| r.1.as_str());
        let new = move_group(
            c,
            album,
            &[v],
            &format!("{}#v{v}", version_tag_key(key)),
            now,
        )?;
        // Still an edition of the same work, until a match says more.
        if let Some(g) = &album_group {
            c.execute(
                "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value) VALUES (?1, 'musicbrainz', 'release_group', ?2)",
                params![new, g],
            )?;
        }
        made.push(new);
    }
    let group = groups.get(&home_release).cloned().or(album_group);
    set_release(c, album, &home_release, group.as_deref())?;
    // The album is named after the release that stayed.
    let (title, year): (String, Option<i64>) = c.query_row(
        "SELECT title, year FROM album_version WHERE id = ?1",
        [home_versions[0]],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let current: String = c.query_row(
        "SELECT coalesce(title, '') FROM item WHERE id = ?1",
        [album],
        |r| r.get(0),
    )?;
    let named = home_versions
        .iter()
        .try_fold(false, |seen, v| -> Result<bool> {
            let t: String =
                c.query_row("SELECT title FROM album_version WHERE id = ?1", [v], |r| {
                    r.get(0)
                })?;
            Ok(seen || normalize(&t) == normalize(&current))
        })?;
    if !named && !title.is_empty() {
        c.execute(
            "UPDATE item SET title = ?2, sort_title = ?3, year = coalesce(?4, year) WHERE id = ?1",
            params![album, title, crate::text::sort_title(&title), year],
        )?;
        c.execute(
            "UPDATE item_fts SET title = ?2 WHERE rowid = ?1",
            params![album, title],
        )?;
    }
    follow_files(c, &tracks)?;
    Ok(made)
}

/// One album per release across the library: albums holding several are split, albums of one
/// release are joined. Albums locked by hand stay as they are. Returns each album joined away
/// with the one it joined.
pub fn reconcile_releases(c: &Connection, now: i64) -> Result<Vec<(i64, i64)>> {
    let albums: Vec<i64> = {
        let mut st =
            c.prepare("SELECT id FROM item WHERE kind = 'album' AND locked = 0 ORDER BY id")?;
        st.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for &a in &albums {
        split_by_release(c, a, now)?;
    }
    // A locked album keeps its place but no longer carries the old key.
    c.execute(
        "UPDATE item SET group_key = 'rel:' || (SELECT min(e.value) FROM external_id e WHERE e.item_id = item.id AND e.provider = 'musicbrainz' AND e.kind = 'release')
         WHERE kind = 'album' AND locked != 0 AND group_key LIKE 'rg:%'
           AND EXISTS (SELECT 1 FROM external_id e WHERE e.item_id = item.id AND e.provider = 'musicbrainz' AND e.kind = 'release')",
        [],
    )?;
    let with_release: Vec<i64> = {
        let mut st = c.prepare(
            "SELECT DISTINCT a.id FROM item a JOIN external_id e ON e.item_id = a.id AND e.provider = 'musicbrainz' AND e.kind = 'release'
             WHERE a.kind = 'album' AND a.locked = 0 ORDER BY a.id",
        )?;
        st.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut joined = Vec::new();
    for a in with_release {
        let still: bool = c.query_row(
            "SELECT EXISTS (SELECT 1 FROM item WHERE id = ?1)",
            [a],
            |r| r.get(0),
        )?;
        if !still {
            continue;
        }
        let kept = merge_same_release(c, a)?;
        if kept != a {
            joined.push((a, kept));
        }
    }
    Ok(joined)
}

/// Migration 16 (issue #147): albums were release groups; now each release is an album.
pub fn migration_16(c: &rusqlite::Transaction) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    reconcile_releases(c, now)?;
    Ok(())
}

// ---- other editions (issue #147) ------------------------------------------------------------

/// Another release of the same release group, as the album page links it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherEdition {
    pub id: i64,
    pub title: String,
    pub year: Option<i64>,
    pub poster: String,
    pub codec: String,
    pub sample_rate: i64,
    pub bit_depth: i64,
    pub tracks: i64,
}

/// The other albums in this album's release group, oldest first.
pub fn other_editions(c: &Connection, album: i64) -> Result<Vec<OtherEdition>> {
    let mut out: Vec<OtherEdition> = {
        let mut st = c.prepare(
            "SELECT DISTINCT a.id, coalesce(a.title, ''), a.year
             FROM external_id mine
             JOIN external_id o ON o.provider = 'musicbrainz' AND o.kind = 'release_group' AND o.value = mine.value AND o.item_id != mine.item_id
             JOIN item a ON a.id = o.item_id AND a.kind = 'album'
             WHERE mine.item_id = ?1 AND mine.provider = 'musicbrainz' AND mine.kind = 'release_group'
             ORDER BY coalesce(a.year, 9999), a.sort_title, a.id",
        )?;
        st.query_map([album], |r| {
            Ok(OtherEdition {
                id: r.get(0)?,
                title: r.get(1)?,
                year: r.get(2)?,
                ..Default::default()
            })
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    for e in &mut out {
        e.poster = crate::music::art(c, e.id, "poster_grid")?;
        if let Some(best) = versions(c, e.id)?.first() {
            e.codec = best.codec.clone();
            e.sample_rate = best.sample_rate;
            e.bit_depth = best.bit_depth;
            e.tracks = best.tracks;
        }
    }
    Ok(out)
}

/// A tag-keyed album whose versions' track lists differ: versions that do not match the first
/// one become albums of their own (they are other albums with the same name, not copies).
/// Returns the albums made.
pub fn check_track_lists(c: &Connection, album: i64, now: i64) -> Result<Vec<i64>> {
    let key: String = c.query_row(
        "SELECT coalesce(group_key, '') FROM item WHERE id = ?1",
        [album],
        |r| r.get(0),
    )?;
    if !key.starts_with("tag:") {
        return Ok(vec![]);
    }
    let key = key.split('#').next().unwrap_or(&key).to_string();
    let versions: Vec<i64> = {
        let mut st = c.prepare("SELECT id FROM album_version WHERE album_id = ?1 ORDER BY id")?;
        st.query_map([album], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    // Versions with the same track list belong together; the first version's group stays.
    let mut groups: Vec<(BTreeSet<String>, Vec<i64>)> = Vec::new();
    for v in versions {
        let list = version_track_list(c, v)?;
        match groups.iter_mut().find(|(l, _)| *l == list) {
            Some((_, vs)) => vs.push(v),
            None => groups.push((list, vec![v])),
        }
    }
    // A group that does not stay joins another album of this name with its track list (made by
    // an earlier scan), else becomes one.
    let base = key.split('#').next().unwrap_or(&key).to_string();
    let mut made = Vec::new();
    for (list, vs) in groups.into_iter().skip(1) {
        let siblings: Vec<i64> = {
            let mut st = c.prepare(
                "SELECT id FROM item WHERE kind = 'album' AND locked = 0 AND id != ?1
                   AND (group_key = ?2 OR group_key LIKE ?2 || '#%') ORDER BY id",
            )?;
            st.query_map(params![album, base], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let mut home = None;
        for sib in siblings {
            let first: Option<i64> = c
                .query_row(
                    "SELECT id FROM album_version WHERE album_id = ?1 ORDER BY id LIMIT 1",
                    [sib],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(f) = first
                && version_track_list(c, f)? == list
            {
                home = Some(sib);
                break;
            }
        }
        let target = match home {
            Some(h) => h,
            None => {
                let first = vs[0];
                let new = move_version(c, first, &format!("{base}#v{first}"), false, now)?;
                made.push(new);
                new
            }
        };
        for &v in &vs {
            if c.query_row(
                "SELECT album_id FROM album_version WHERE id = ?1",
                [v],
                |r| r.get::<_, i64>(0),
            )? == target
            {
                continue;
            }
            let moved = move_version(c, v, &format!("{base}#v{v}"), false, now)?;
            merge_albums(c, target, moved)?;
        }
    }
    Ok(made)
}

/// An album is named after its plainest version: "Other" rather than "Other (2011 Remaster)".
pub fn settle_title(c: &Connection, album: i64) -> Result<()> {
    let current: String = c.query_row(
        "SELECT coalesce(title, '') FROM item WHERE id = ?1",
        [album],
        |r| r.get(0),
    )?;
    let titles: Vec<String> = {
        let mut st = c.prepare_cached(
            "SELECT title FROM album_version WHERE album_id = ?1 AND title != '' ORDER BY id",
        )?;
        st.query_map([album], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    // A version title that the current one starts with (and is shorter) is the plainer name.
    let cur = normalize(&current);
    let better = titles
        .iter()
        .filter(|t| {
            let n = normalize(t);
            !n.is_empty() && n.len() < cur.len() && cur.starts_with(&n)
        })
        .min_by_key(|t| normalize(t).len());
    if let Some(t) = better {
        c.execute(
            "UPDATE item SET title = ?2, sort_title = ?3 WHERE id = ?1",
            params![album, t, crate::text::sort_title(t)],
        )?;
        c.execute(
            "UPDATE item_fts SET title = ?2 WHERE rowid = ?1",
            params![album, t],
        )?;
    }
    Ok(())
}

/// "This is a different album": the version becomes an album of its own, locked so that scans
/// and lookups do not merge it again. Returns the new album.
pub fn split_version(c: &Connection, version: i64, now: i64) -> Result<i64> {
    c.execute(
        "UPDATE album_version SET split = 1 WHERE id = ?1",
        [version],
    )?;
    move_version(c, version, &format!("split:{version}"), true, now)
}

/// Move a version into a new album with `key`. Tracks that only this version has move along;
/// tracks other versions share are copied (title, numbers, credits) and the version's files move
/// to the copies.
fn move_version(c: &Connection, version: i64, key: &str, locked: bool, now: i64) -> Result<i64> {
    let (old, title, year): (i64, String, Option<i64>) = c.query_row(
        "SELECT album_id, title, year FROM album_version WHERE id = ?1",
        [version],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let (parent, old_title, old_year): (Option<i64>, String, Option<i64>) = c.query_row(
        "SELECT parent_id, coalesce(title, ''), year FROM item WHERE id = ?1",
        [old],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let title = if title.is_empty() { old_title } else { title };
    c.execute(
        "INSERT INTO item(kind, parent_id, title, sort_title, year, match_state, added_at, group_key, locked)
         VALUES ('album', ?1, ?2, ?3, ?4, 'unmatched', ?5, ?6, ?7)",
        params![parent, title, crate::text::sort_title(&title), year.or(old_year), now, key, locked],
    )?;
    let album = c.last_insert_rowid();
    c.execute(
        "INSERT INTO item_fts(rowid, title) VALUES (?1, ?2)",
        params![album, title],
    )?;
    c.execute(
        "INSERT INTO music(item_id, release_type, compilation, genre, release_date)
         SELECT ?1, release_type, compilation, genre, release_date FROM music WHERE item_id = ?2",
        params![album, old],
    )?;
    c.execute(
        "INSERT INTO credit(item_id, artist_id, role, position, join_phrase)
         SELECT ?1, artist_id, role, position, join_phrase FROM credit WHERE item_id = ?2",
        params![album, old],
    )?;
    // The cover is shared until a scan or lookup gives the new album its own.
    c.execute(
        "INSERT INTO artwork(item_id, role, remote_path, local_path, width, height)
         SELECT ?1, role, remote_path, local_path, width, height FROM artwork WHERE item_id = ?2 AND role LIKE 'poster%'",
        params![album, old],
    )?;
    c.execute(
        "UPDATE album_version SET album_id = ?1 WHERE id = ?2",
        params![album, version],
    )?;
    let tracks: Vec<i64> = {
        let mut st = c.prepare(
            "SELECT DISTINCT item_id FROM file WHERE version_id = ?1 AND item_id IS NOT NULL",
        )?;
        st.query_map([version], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for t in tracks {
        let shared: bool = c.query_row(
            "SELECT EXISTS (SELECT 1 FROM file WHERE item_id = ?1 AND coalesce(version_id, 0) != ?2)",
            params![t, version],
            |r| r.get(0),
        )?;
        if !shared {
            c.execute(
                "UPDATE item SET parent_id = ?1 WHERE id = ?2",
                params![album, t],
            )?;
            continue;
        }
        c.execute(
            "INSERT INTO item(kind, parent_id, title, sort_title, year, season_no, episode_no, match_state, added_at, group_key)
             SELECT kind, ?1, title, sort_title, year, season_no, episode_no, match_state, ?3, group_key FROM item WHERE id = ?2",
            params![album, t, now],
        )?;
        let copy = c.last_insert_rowid();
        c.execute(
            "INSERT INTO item_fts(rowid, title) SELECT ?1, title FROM item WHERE id = ?1",
            [copy],
        )?;
        c.execute(
            "INSERT INTO music(item_id, disc_no, track_no, track_total, release_date, genre, rg_track_gain, rg_track_peak)
             SELECT ?1, disc_no, track_no, track_total, release_date, genre, rg_track_gain, rg_track_peak FROM music WHERE item_id = ?2",
            params![copy, t],
        )?;
        c.execute(
            "INSERT INTO credit(item_id, artist_id, role, position, join_phrase)
             SELECT ?1, artist_id, role, position, join_phrase FROM credit WHERE item_id = ?2",
            params![copy, t],
        )?;
        c.execute(
            "UPDATE file SET item_id = ?1 WHERE item_id = ?2 AND version_id = ?3",
            params![copy, t, version],
        )?;
        c.execute(
            "UPDATE local_copy SET item_id = ?1 WHERE item_id = ?2 AND file_id IN (SELECT id FROM file WHERE item_id = ?1)",
            params![copy, t],
        )?;
    }
    Ok(album)
}

// ---- the versions of an album ---------------------------------------------------------------

/// One version as the album page shows it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub id: i64,
    /// "2011 Remaster", "Deluxe Edition", or a year; "" for the album as it is named.
    pub edition: String,
    pub source_id: i64,
    pub source_name: String,
    pub source_kind: String,
    /// The codec most of its files have.
    pub codec: String,
    pub sample_rate: i64,
    pub bit_depth: i64,
    pub lossless: bool,
    /// Average, for lossy files (kbit/s); 0 otherwise.
    pub bitrate: i64,
    pub size: i64,
    pub tracks: i64,
    pub hires: bool,
    /// Set by the core: the source can be reached, or every track has a saved copy.
    pub online: bool,
    pub saved: bool,
    pub split: bool,
}

/// `s` without `prefix`, ignoring case.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let mut it = s.char_indices();
    for pc in prefix.chars() {
        let (_, sc) = it.next()?;
        if !sc.to_lowercase().eq(pc.to_lowercase()) {
            return None;
        }
    }
    Some(it.as_str())
}

/// "Parachutes (2011 Remaster)" next to "Parachutes" → "2011 Remaster"; another title → it; the
/// same title in another year → the year; else "".
pub fn edition(
    album_title: &str,
    version_title: &str,
    album_year: Option<i64>,
    version_year: Option<i64>,
) -> String {
    if !normalize(version_title).is_empty() && normalize(version_title) != normalize(album_title) {
        let rest = strip_prefix_ci(version_title.trim(), album_title.trim())
            .map(|r| r.trim_matches(|c: char| c.is_whitespace() || "-–:([{)]}".contains(c)))
            .filter(|r| !r.is_empty())
            .unwrap_or(version_title.trim());
        return rest.to_string();
    }
    match (album_year, version_year) {
        (Some(a), Some(v)) if a != v => v.to_string(),
        _ => String::new(),
    }
}

pub fn versions(c: &Connection, album: i64) -> Result<Vec<VersionInfo>> {
    let (album_title, album_year): (String, Option<i64>) = c.query_row(
        "SELECT coalesce(title, ''), year FROM item WHERE id = ?1",
        [album],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut st = c.prepare(
        "SELECT v.id, v.title, v.year, v.source_id, coalesce(s.name, ''), coalesce(s.kind, ''), v.split,
                (SELECT fa.codec FROM file f JOIN file_audio fa ON fa.file_id = f.id WHERE f.version_id = v.id
                   GROUP BY fa.codec ORDER BY count(*) DESC LIMIT 1),
                (SELECT max(coalesce(fa.sample_rate, 0)) FROM file f JOIN file_audio fa ON fa.file_id = f.id WHERE f.version_id = v.id),
                (SELECT max(coalesce(fa.bit_depth, 0)) FROM file f JOIN file_audio fa ON fa.file_id = f.id WHERE f.version_id = v.id),
                (SELECT min(coalesce(fa.lossless, 0)) FROM file f JOIN file_audio fa ON fa.file_id = f.id WHERE f.version_id = v.id),
                (SELECT avg(coalesce(fa.bitrate, 0)) FROM file f JOIN file_audio fa ON fa.file_id = f.id WHERE f.version_id = v.id),
                (SELECT sum(coalesce(f.size, 0)) FROM file f WHERE f.version_id = v.id),
                (SELECT count(DISTINCT f.item_id) FROM file f WHERE f.version_id = v.id),
                NOT EXISTS (SELECT 1 FROM file f WHERE f.version_id = v.id
                  AND NOT EXISTS (SELECT 1 FROM local_copy lc WHERE lc.file_id = f.id AND lc.kind = 'saved' AND lc.main_id IS NULL AND lc.state = 'done'))
         FROM album_version v LEFT JOIN source s ON s.id = v.source_id
         WHERE v.album_id = ?1 AND EXISTS (SELECT 1 FROM file f WHERE f.version_id = v.id)
         ORDER BY v.id",
    )?;
    let rows = st.query_map([album], |r| {
        let title: String = r.get(1)?;
        let year: Option<i64> = r.get(2)?;
        let lossless = r.get::<_, Option<i64>>(10)?.unwrap_or(0) != 0;
        let (rate, depth): (i64, i64) = (
            r.get::<_, Option<i64>>(8)?.unwrap_or(0),
            r.get::<_, Option<i64>>(9)?.unwrap_or(0),
        );
        Ok(VersionInfo {
            id: r.get(0)?,
            edition: edition(&album_title, &title, album_year, year),
            source_id: r.get(3)?,
            source_name: r.get(4)?,
            source_kind: r.get(5)?,
            split: r.get::<_, i64>(6)? != 0,
            codec: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
            sample_rate: rate,
            bit_depth: depth,
            lossless,
            bitrate: if lossless {
                0
            } else {
                r.get::<_, Option<f64>>(11)?.unwrap_or(0.0).round() as i64
            },
            size: r.get::<_, Option<i64>>(12)?.unwrap_or(0),
            tracks: r.get(13)?,
            saved: r.get(14)?,
            hires: crate::music::hires(lossless, Some(depth), Some(rate)),
            online: true,
        })
    })?;
    let mut out: Vec<VersionInfo> = rows.collect::<rusqlite::Result<_>>()?;
    // Best first: lossless, bit depth, sample rate, bit rate; then the oldest.
    out.sort_by(|a, b| {
        (b.lossless, b.bit_depth, b.sample_rate, b.bitrate, a.id).cmp(&(
            a.lossless,
            a.bit_depth,
            a.sample_rate,
            a.bitrate,
            b.id,
        ))
    });
    Ok(out)
}

// ---- which one to play ----------------------------------------------------------------------

/// What preselection compares (a version, or one track's files).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Choice {
    pub id: i64,
    pub lossless: bool,
    pub bit_depth: i64,
    pub sample_rate: i64,
    pub bitrate: i64,
    pub size: i64,
    pub playable: bool,
}

impl From<&VersionInfo> for Choice {
    fn from(v: &VersionInfo) -> Self {
        Choice {
            id: v.id,
            lossless: v.lossless,
            bit_depth: v.bit_depth,
            sample_rate: v.sample_rate,
            bitrate: v.bitrate,
            size: v.size,
            playable: v.online || v.saved,
        }
    }
}

/// The one to play: the one played last when it can be played; else by `preference`:
/// "quality" (lossless first, then bit depth, then sample rate, then bit rate), "smallest" (the
/// smallest), "lossless" (the best lossless one, the best of all when none is). Ones that cannot
/// be played only when nothing else is left.
pub fn preselect(choices: &[Choice], last: Option<i64>, preference: &str) -> Option<i64> {
    if let Some(l) = last
        && choices.iter().any(|c| c.id == l && c.playable)
    {
        return Some(l);
    }
    let playable: Vec<&Choice> = choices.iter().filter(|c| c.playable).collect();
    let pool: Vec<&Choice> = if playable.is_empty() {
        choices.iter().collect()
    } else {
        playable
    };
    fn quality(c: &Choice) -> (bool, i64, i64, i64, i64) {
        (c.lossless, c.bit_depth, c.sample_rate, c.bitrate, c.size)
    }
    match preference {
        "smallest" => pool.iter().min_by_key(|c| (c.size, -c.id)).map(|c| c.id),
        "lossless" => {
            let lossless: Vec<&&Choice> = pool.iter().filter(|c| c.lossless).collect();
            if lossless.is_empty() {
                pool.iter().max_by_key(|c| quality(c)).map(|c| c.id)
            } else {
                lossless
                    .into_iter()
                    .max_by_key(|c| quality(c))
                    .map(|c| c.id)
            }
        }
        _ => pool.iter().max_by_key(|c| quality(c)).map(|c| c.id),
    }
}

/// The version last played of an album (its play state names a file of it).
pub fn last_version(c: &Connection, album: i64) -> Result<Option<i64>> {
    Ok(c.query_row(
        "SELECT f.version_id FROM playstate p JOIN file f ON f.id = p.last_file_id WHERE p.item_id = ?1 AND p.profile_id = 1",
        [album],
        |r| r.get(0),
    )
    .optional()?
    .flatten())
}

/// Remember the file played for its track and its album (the album's version, the track's file
/// when the version has two).
pub fn remember_played(c: &Connection, track: i64, file: i64, now: i64) -> Result<()> {
    let album: Option<i64> = c
        .query_row("SELECT parent_id FROM item WHERE id = ?1", [track], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    for item in std::iter::once(track).chain(album) {
        c.execute(
            "INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at, last_file_id)
             VALUES (?1, 1, 0, 0, 0, ?3, ?2)
             ON CONFLICT(item_id, profile_id) DO UPDATE SET last_file_id = excluded.last_file_id",
            params![item, file, now],
        )?;
    }
    Ok(())
}

/// One file of a track as the track's version menu shows it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackFile {
    pub id: i64,
    pub codec: String,
    pub sample_rate: i64,
    pub bit_depth: i64,
    pub lossless: bool,
    pub bitrate: i64,
    pub size: i64,
    pub duration_ms: i64,
    pub name: String,
}

/// A track's files in a version.
pub fn track_files(c: &Connection, track: i64, version: i64) -> Result<Vec<TrackFile>> {
    let mut st = c.prepare_cached(
        "SELECT f.id, coalesce(fa.codec, ''), coalesce(fa.sample_rate, 0), coalesce(fa.bit_depth, 0),
                coalesce(fa.lossless, 0), coalesce(fa.bitrate, 0), coalesce(f.size, 0), coalesce(fa.duration_ms, 0), f.path
         FROM file f LEFT JOIN file_audio fa ON fa.file_id = f.id
         WHERE f.item_id = ?1 AND f.version_id = ?2 ORDER BY f.id",
    )?;
    let rows = st.query_map(params![track, version], |r| {
        let lossless = r.get::<_, i64>(4)? != 0;
        let path: String = r.get(8)?;
        Ok(TrackFile {
            id: r.get(0)?,
            codec: r.get(1)?,
            sample_rate: r.get(2)?,
            bit_depth: r.get(3)?,
            lossless,
            bitrate: if lossless { 0 } else { r.get::<_, i64>(5)? },
            size: r.get(6)?,
            duration_ms: r.get(7)?,
            name: path.rsplit('/').next().unwrap_or(&path).to_string(),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The file to play of a track in a version: the one played last, else by preference.
pub fn track_file(
    c: &Connection,
    track: i64,
    version: i64,
    preference: &str,
) -> Result<Option<i64>> {
    let files = track_files(c, track, version)?;
    let last: Option<i64> = c
        .query_row(
            "SELECT last_file_id FROM playstate WHERE item_id = ?1 AND profile_id = 1",
            [track],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let choices: Vec<Choice> = files
        .iter()
        .map(|f| Choice {
            id: f.id,
            lossless: f.lossless,
            bit_depth: f.bit_depth,
            sample_rate: f.sample_rate,
            bitrate: f.bitrate,
            size: f.size,
            playable: true,
        })
        .collect();
    Ok(preselect(&choices, last, preference))
}

/// A version's tracks in album order with the file to play of each.
pub fn version_tracks(c: &Connection, version: i64, preference: &str) -> Result<Vec<(i64, i64)>> {
    let tracks: Vec<i64> = {
        let mut st = c.prepare_cached(
            "SELECT DISTINCT t.id, coalesce(t.season_no, 1), coalesce(t.episode_no, 0), t.sort_title
             FROM file f JOIN item t ON t.id = f.item_id
             WHERE f.version_id = ?1 AND t.kind = 'track'
             ORDER BY 2, 3, 4",
        )?;
        st.query_map([version], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut out = Vec::new();
    for t in tracks {
        if let Some(f) = track_file(c, t, version, preference)? {
            out.push((t, f));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2.0 library (schema 8) with the same album in two folders and on two sources, a
    /// remaster in the same release group, and an album of the same name with other songs: the
    /// migration merges the copies and the edition, keeps play counts (added up), the last play,
    /// the queue and saved copies, and leaves the other album alone.
    #[test]
    fn migration_9_merges_copies() {
        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        for m in &crate::MIGRATIONS[..8] {
            let tx = c.transaction().unwrap();
            match m {
                crate::Migration::Sql(sql) => tx.execute_batch(sql).unwrap(),
                crate::Migration::Rust(f)
                | crate::Migration::Rebuild(f)
                | crate::Migration::Merge(f) => f(&tx).unwrap(),
            }
            tx.commit().unwrap();
        }
        c.pragma_update(None, "user_version", 8).unwrap();
        c.execute_batch(
            "INSERT INTO source(id, kind, name, content) VALUES (2, 'local', 'NAS', 'music'), (3, 'smb', 'Laptop', 'music');
             INSERT INTO item(id, kind, title, sort_title) VALUES (1, 'artist', 'Band', 'band');
             -- 2.0 albums: one per folder.
             INSERT INTO item(id, kind, title, sort_title, year, parent_id, group_key, added_at) VALUES
               (10, 'album', 'Album', 'album', 2000, 1, 'dir:Band/Album (2000)|band|album|2000', 5),
               (11, 'album', 'Album', 'album', 2000, 1, 'dir:Band/Album MP3|band|album|2000', 3),
               (12, 'album', 'Album', 'album', 2000, 1, 'dir:Album|band|album|2000', 9),
               (13, 'album', 'Album', 'album', 2000, 1, 'dir:Band/Album live|band|album|2000', 1),
               (14, 'album', 'Other', 'other', 2001, 1, 'mb:rel-orig', 1),
               (15, 'album', 'Other (2011 Remaster)', 'other 2011 remaster', 2011, 1, 'mb:rel-remaster', 1);
             INSERT INTO external_id(item_id, provider, kind, value) VALUES
               (14, 'musicbrainz', 'release', 'rel-orig'), (14, 'musicbrainz', 'release_group', 'rg-other'),
               (15, 'musicbrainz', 'release', 'rel-remaster'), (15, 'musicbrainz', 'release_group', 'rg-other');
             INSERT INTO item(id, kind, title, sort_title, parent_id, season_no, episode_no, group_key) VALUES
               (100, 'track', 'Song 1', 'song 1', 10, 1, 1, 'file:1000'), (101, 'track', 'Song 2', 'song 2', 10, 1, 2, 'file:1001'),
               (110, 'track', 'Song 1', 'song 1', 11, 1, 1, 'file:1100'), (111, 'track', 'Song 2', 'song 2', 11, 1, 2, 'file:1101'),
               (120, 'track', 'Song 1', 'song 1', 12, 1, 1, 'file:1200'), (121, 'track', 'Song 2', 'song 2', 12, 1, 2, 'file:1201'),
               (130, 'track', 'Live 1', 'live 1', 13, 1, 1, 'file:1300'),
               (140, 'track', 'Other 1', 'other 1', 14, 1, 1, 'file:1400'),
               (150, 'track', 'Other 1', 'other 1', 15, 1, 1, 'file:1500'), (151, 'track', 'Other 2', 'other 2', 15, 1, 2, 'file:1501');
             INSERT INTO file(id, source_id, path, size, item_id) VALUES
               (1000, 2, 'Band/Album (2000)/01.flac', 10, 100), (1001, 2, 'Band/Album (2000)/02.flac', 10, 101),
               (1100, 2, 'Band/Album MP3/01.mp3', 5, 110), (1101, 2, 'Band/Album MP3/02.mp3', 5, 111),
               (1200, 3, 'Album/01.flac', 8, 120), (1201, 3, 'Album/02.flac', 8, 121),
               (1300, 2, 'Band/Album live/01.flac', 8, 130),
               (1400, 2, 'Band/Other/01.flac', 8, 140),
               (1500, 2, 'Band/Other 2011/01.flac', 8, 150), (1501, 2, 'Band/Other 2011/02.flac', 8, 151);
             INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at, play_count, last_played_at) VALUES
               (100, 1, 0, 0, 0, 1, 3, 50), (110, 1, 0, 0, 0, 1, 2, 70), (120, 1, 0, 0, 0, 1, 1, 60);
             INSERT INTO play_queue(position, item_id, file_id) VALUES (0, 111, 1101), (1, 130, 1300);
             INSERT INTO local_copy(item_id, file_id, kind, state, source_id, source_path, path, created_at)
               VALUES (121, 1201, 'saved', 'done', 3, 'Album/02.flac', '/x/02.flac', 1);",
        )
        .unwrap();
        crate::migrate(&mut c).unwrap();

        let q = |sql: &str| -> Vec<String> {
            c.prepare(sql)
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        // Copies: one album with three versions; the live album apart; the remaster, another
        // release of the group, an album of its own since migration 16 (issue #147).
        assert_eq!(
            q("SELECT a.id || ' ' || a.title || ' ' || (SELECT count(*) FROM album_version v WHERE v.album_id = a.id) || ' ' || a.added_at
               FROM item a WHERE a.kind = 'album' ORDER BY a.id"),
            [
                "10 Album 3 3",
                "13 Album 1 1",
                "14 Other 1 1",
                "152 Other (2011 Remaster) 1 1"
            ]
        );
        // One track per song, with every copy's file; play counts added up, the latest play kept.
        assert_eq!(
            q("SELECT t.title || ' ' || (SELECT count(*) FROM file f WHERE f.item_id = t.id) || ' ' || coalesce(p.play_count, 0) || ' ' || coalesce(p.last_played_at, 0)
               FROM item t LEFT JOIN playstate p ON p.item_id = t.id WHERE t.parent_id = 10 ORDER BY t.title"),
            ["Song 1 3 6 70", "Song 2 3 0 0"]
        );
        // The queue and the saved copy follow their tracks; the files stay the same.
        assert_eq!(
            q(
                "SELECT t.title || ' ' || q.file_id FROM play_queue q JOIN item t ON t.id = q.item_id ORDER BY q.position"
            ),
            ["Song 2 1101", "Live 1 1300"]
        );
        assert_eq!(
            q(
                "SELECT t.title || ' ' || t.parent_id FROM local_copy l JOIN item t ON t.id = l.item_id"
            ),
            ["Song 2 10"]
        );
        // The remaster keeps its own tracks and is linked as an edition (issue #147).
        let other: Vec<String> = q(
            "SELECT t.title || ' ' || count(f.id) FROM item t JOIN file f ON f.item_id = t.id WHERE t.parent_id = 152 GROUP BY t.id ORDER BY t.title",
        );
        assert_eq!(other, ["Other 1 1", "Other 2 1"]);
        assert_eq!(other_editions(&c, 14).unwrap()[0].id, 152);
        // Nothing points nowhere.
        let broken: i64 = c
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(broken, 0);
    }

    /// A library at schema 15 with one release group holding a standard and a deluxe release,
    /// a copy of the standard one, plays, a playlist entry and a saved copy on each.
    fn editions_at_15() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        for m in &crate::MIGRATIONS[..15] {
            let tx = c.transaction().unwrap();
            match m {
                crate::Migration::Sql(sql) => tx.execute_batch(sql).unwrap(),
                crate::Migration::Rust(f)
                | crate::Migration::Rebuild(f)
                | crate::Migration::Merge(f) => f(&tx).unwrap(),
            }
            tx.commit().unwrap();
        }
        c.pragma_update(None, "user_version", 15).unwrap();
        c.execute_batch(
            "INSERT INTO source(id, kind, name, content) VALUES (2, 'local', 'NAS', 'music');
             INSERT INTO item(id, kind, title, sort_title) VALUES (1, 'artist', 'Band', 'band');
             INSERT INTO item(id, kind, title, sort_title, year, parent_id, group_key, added_at)
               VALUES (10, 'album', 'Album', 'album', 2000, 1, 'rg:g1', 7);
             INSERT INTO external_id(item_id, provider, kind, value) VALUES
               (10, 'musicbrainz', 'release', 'std'), (10, 'musicbrainz', 'release', 'dlx'),
               (10, 'musicbrainz', 'release_group', 'g1');
             INSERT INTO album_version(id, album_id, key, source_id, title, year, release_id, release_group) VALUES
               (100, 10, 'v:2:mb:std', 2, 'Album', 2000, 'std', 'g1'),
               (101, 10, 'v:2:dir:Copy|band|album|2000', 2, 'Album', 2000, 'std', 'g1'),
               (102, 10, 'v:2:mb:dlx', 2, 'Album (Deluxe)', 2001, 'dlx', 'g1');
             INSERT INTO item(id, kind, title, sort_title, parent_id, group_key, season_no, episode_no, added_at) VALUES
               (20, 'track', 'One', 'one', 10, 't:1|one', 1, 1, 7),
               (21, 'track', 'Bonus', 'bonus', 10, 't:1|bonus', 1, 2, 7);
             INSERT INTO file(id, source_id, path, item_id, version_id) VALUES
               (200, 2, 's/1.flac', 20, 100), (201, 2, 'c/1.flac', 20, 101),
               (202, 2, 'd/1.flac', 20, 102), (203, 2, 'd/2.flac', 21, 102);
             INSERT INTO playstate(item_id, profile_id, updated_at, play_count, last_file_id) VALUES
               (20, 1, 1, 5, 202), (21, 1, 1, 2, 203);
             INSERT INTO playlist(id, name, created_at, updated_at) VALUES (1, 'P', 1, 1);
             INSERT INTO playlist_entry(playlist_id, position, item_id, file_id) VALUES
               (1, 0, 20, 202), (1, 1, 20, 200);
             INSERT INTO local_copy(id, file_id, item_id, source_id, source_path, path, state, created_at)
               VALUES (1, 202, 20, 2, 'd/1.flac', '/saved/1.flac', 'done', 1);",
        )
        .unwrap();
        c
    }

    #[test]
    fn migration_16_one_album_per_release() {
        let mut c = editions_at_15();
        let tx = c.transaction().unwrap();
        migration_16(&tx).unwrap();
        tx.commit().unwrap();
        let album_of = |v: i64| -> i64 {
            c.query_row(
                "SELECT album_id FROM album_version WHERE id = ?1",
                [v],
                |r| r.get(0),
            )
            .unwrap()
        };
        // Two copies of the standard release: one album. The deluxe release: another.
        assert_eq!(album_of(100), 10);
        assert_eq!(album_of(101), 10);
        let dlx = album_of(102);
        assert_ne!(dlx, 10);
        let key = |a: i64| -> String {
            c.query_row("SELECT group_key FROM item WHERE id = ?1", [a], |r| {
                r.get(0)
            })
            .unwrap()
        };
        assert_eq!(key(10), "rel:std");
        assert_eq!(key(dlx), "rel:dlx");
        // Linked as editions.
        assert_eq!(other_editions(&c, 10).unwrap()[0].id, dlx);
        assert_eq!(other_editions(&c, dlx).unwrap()[0].id, 10);
        // Plays are neither lost nor doubled, and follow the file last played.
        let total: i64 = c
            .query_row("SELECT sum(play_count) FROM playstate", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 7);
        let track_202: i64 = c
            .query_row("SELECT item_id FROM file WHERE id = 202", [], |r| r.get(0))
            .unwrap();
        assert_ne!(track_202, 20);
        let plays: i64 = c
            .query_row(
                "SELECT play_count FROM playstate WHERE item_id = ?1",
                [track_202],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(plays, 5);
        // The playlist entry and the saved copy go with their file; the other entry stays.
        let entry = |pos: i64| -> i64 {
            c.query_row(
                "SELECT item_id FROM playlist_entry WHERE position = ?1",
                [pos],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(entry(0), track_202);
        assert_eq!(entry(1), 20);
        let saved: i64 = c
            .query_row("SELECT item_id FROM local_copy WHERE id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(saved, track_202);
        let broken: i64 = c
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(broken, 0);
        // Running it again changes nothing.
        let albums = |c: &Connection| -> i64 {
            c.query_row("SELECT count(*) FROM item WHERE kind = 'album'", [], |r| {
                r.get(0)
            })
            .unwrap()
        };
        let before = albums(&c);
        reconcile_releases(&c, 9).unwrap();
        assert_eq!(albums(&c), before);
    }

    #[test]
    fn a_new_match_is_not_undone_by_the_next_scan() {
        let mut c = editions_at_15();
        let tx = c.transaction().unwrap();
        migration_16(&tx).unwrap();
        tx.commit().unwrap();
        // Match again: the standard album is really another release.
        crate::music::replace_release(
            &mut c,
            10,
            &crate::music::ReleaseFacts {
                id: "other".into(),
                group_id: Some("g2".into()),
                ..Default::default()
            },
        )
        .unwrap();
        merge_same_release(&c, 10).unwrap();
        let albums = |c: &Connection| -> i64 {
            c.query_row("SELECT count(*) FROM item WHERE kind = 'album'", [], |r| {
                r.get(0)
            })
            .unwrap()
        };
        let before = albums(&c);
        reconcile_releases(&c, 9).unwrap();
        assert_eq!(albums(&c), before);
        assert_eq!(
            external(&c, 10, "release").unwrap().as_deref(),
            Some("other")
        );
        let key: String = c
            .query_row("SELECT group_key FROM item WHERE id = 10", [], |r| r.get(0))
            .unwrap();
        assert_eq!(key, "rel:other");
    }

    #[test]
    fn locked_split_stays_and_no_id_waits_for_its_match() {
        let c = editions_at_15();
        c.execute_batch(
            "UPDATE item SET locked = 1 WHERE id = 10;
             INSERT INTO item(id, kind, title, sort_title, parent_id, group_key, added_at) VALUES
               (30, 'album', 'LUX', 'lux', 1, 'tag:band|lux|2025', 1),
               (31, 'album', 'Lux', 'lux', 1, 'rel:lux-r', 1);
             INSERT INTO external_id(item_id, provider, kind, value) VALUES (31, 'musicbrainz', 'release', 'lux-r');",
        )
        .unwrap();
        reconcile_releases(&c, 9).unwrap();
        // A locked album is not split.
        let n: i64 = c
            .query_row(
                "SELECT count(*) FROM album_version WHERE album_id = 10",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 3);
        // A title that differs only in case does not join.
        let both: i64 = c
            .query_row("SELECT count(*) FROM item WHERE id IN (30, 31)", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(both, 2);
        // Its match does.
        c.execute(
            "INSERT INTO external_id(item_id, provider, kind, value) VALUES (30, 'musicbrainz', 'release', 'lux-r')",
            [],
        )
        .unwrap();
        assert_eq!(merge_same_release(&c, 31).unwrap(), 30);
    }

    #[test]
    fn identities() {
        assert_eq!(identity_9(Some("G"), Some("r"), "dir:x|a|b|1"), "rg:g");
        assert_eq!(album_identity(Some("R"), "dir:x|a|b|1"), "rel:r");
        assert_eq!(album_identity(None, "mb:abc"), "rel:abc");
        assert_eq!(
            album_identity(None, "dir:Band/Album (2000)|band|album|2000"),
            "tag:band|album|2000"
        );
        assert_eq!(album_identity(None, "dir:|band|album|"), "tag:band|album|");
        assert_eq!(track_key(1, "Song One!", Some(1)), "t:1|song one");
        assert_eq!(track_key(2, "", Some(4)), "t:2|#4");
    }

    #[test]
    fn editions() {
        assert_eq!(
            edition(
                "Parachutes",
                "Parachutes (2011 Remaster)",
                Some(2000),
                Some(2011)
            ),
            "2011 Remaster"
        );
        assert_eq!(
            edition("Parachutes", "Parachutes - Deluxe Edition", None, None),
            "Deluxe Edition"
        );
        assert_eq!(
            edition("Parachutes", "parachutes", Some(2000), Some(2000)),
            ""
        );
        assert_eq!(
            edition("Parachutes", "Parachutes", Some(2000), Some(2011)),
            "2011"
        );
        assert_eq!(edition("A", "Something else", None, None), "Something else");
    }

    fn ch(
        id: i64,
        lossless: bool,
        depth: i64,
        rate: i64,
        bitrate: i64,
        size: i64,
        playable: bool,
    ) -> Choice {
        Choice {
            id,
            lossless,
            bit_depth: depth,
            sample_rate: rate,
            bitrate,
            size,
            playable,
        }
    }

    #[test]
    fn preselection() {
        let flac96 = ch(1, true, 24, 96_000, 0, 1_100, true);
        let flac44 = ch(2, true, 16, 44_100, 0, 400, true);
        let mp3 = ch(3, false, 0, 44_100, 320, 140, true);
        let all = [mp3, flac44, flac96];
        assert_eq!(preselect(&all, None, "quality"), Some(1));
        assert_eq!(preselect(&all, None, "smallest"), Some(3));
        assert_eq!(preselect(&all, None, "lossless"), Some(1));
        // The one played last wins, when it can be played.
        assert_eq!(preselect(&all, Some(3), "quality"), Some(3));
        let offline96 = ch(1, true, 24, 96_000, 0, 1_100, false);
        assert_eq!(
            preselect(&[mp3, flac44, offline96], Some(1), "quality"),
            Some(2)
        );
        // Lossless only with no lossless version: the best there is.
        let mp3_128 = ch(4, false, 0, 44_100, 128, 60, true);
        assert_eq!(preselect(&[mp3, mp3_128], None, "lossless"), Some(3));
        // Nothing playable: still one (to show), the best.
        let off = [
            ch(5, false, 0, 44_100, 128, 60, false),
            ch(6, true, 16, 44_100, 0, 300, false),
        ];
        assert_eq!(preselect(&off, None, "quality"), Some(6));
        assert_eq!(preselect(&[], None, "quality"), None);
    }
}
