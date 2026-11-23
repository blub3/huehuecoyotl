//! Issue #146: one artist per MusicBrainz artist ID. Artists are keyed by their name folded
//! without case and accents (`artist:<fold>`); an artist ID from the tags or a match identifies
//! one. Two artists with the same ID become one, and so do artists whose folded names agree as
//! long as they do not carry different IDs. Joining keeps everything: albums, credits, playlists
//! and play history (which hang on the tracks), artwork, IDs and the match the user chose.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;
use crate::text::fold;

/// MusicBrainz's special artist for compilations; never joined into a real artist by ID.
pub const VARIOUS_MBID: &str = "89ad4ac3-39f7-470e-963a-56509c546377";

/// The grouping key of an artist with this name.
pub fn key(name: &str) -> String {
    format!("artist:{}", fold(name))
}

/// The artist's MusicBrainz artist IDs (normally one).
fn ids(c: &Connection, artist: i64) -> Result<Vec<String>> {
    let mut st = c.prepare(
        "SELECT value FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = 'artist' ORDER BY value",
    )?;
    Ok(st
        .query_map([artist], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

/// Which of these artists stays: the one whose match the user chose, then the one with an ID,
/// then the one with more albums, then the oldest.
fn keeper(c: &Connection, artists: &[i64]) -> Result<i64> {
    let mut best: Option<(i64, (i64, i64, i64, i64))> = None;
    for &a in artists {
        let (locked, has_id, albums): (i64, i64, i64) = c.query_row(
            "SELECT i.locked,
                    EXISTS (SELECT 1 FROM external_id e WHERE e.item_id = i.id AND e.provider = 'musicbrainz' AND e.kind = 'artist'),
                    (SELECT count(*) FROM item al WHERE al.parent_id = i.id AND al.kind = 'album')
             FROM item i WHERE i.id = ?1",
            [a],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let rank = (locked, has_id, albums, -a);
        if best.is_none_or(|(_, b)| rank > b) {
            best = Some((a, rank));
        }
    }
    Ok(best.map(|(a, _)| a).unwrap_or(artists[0]))
}

/// Join artist `drop` into `keep`. Its albums move, its credits point at `keep`, IDs and
/// artwork `keep` lacks move over, a match on either side is kept, and its key leads to `keep`
/// from now on (`group_alias`), so a rescan finds `keep`.
pub fn merge(c: &Connection, keep: i64, drop: i64) -> Result<()> {
    if keep == drop {
        return Ok(());
    }
    // A track or album credited to both keeps one credit per position.
    c.execute(
        "UPDATE OR IGNORE credit SET artist_id = ?1 WHERE artist_id = ?2",
        params![keep, drop],
    )?;
    c.execute("DELETE FROM credit WHERE artist_id = ?1", [drop])?;
    // IDs: the kept artist's own win; kinds it lacks come over.
    c.execute(
        "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value)
         SELECT ?1, provider, kind, value FROM external_id d
         WHERE d.item_id = ?2 AND NOT EXISTS (
           SELECT 1 FROM external_id k WHERE k.item_id = ?1 AND k.provider = d.provider AND k.kind = d.kind)",
        params![keep, drop],
    )?;
    c.execute(
        "UPDATE item SET
           match_state = CASE WHEN match_state = 'matched' THEN match_state
                              ELSE (SELECT match_state FROM item WHERE id = ?2) END,
           meta_json = coalesce(meta_json, (SELECT meta_json FROM item WHERE id = ?2)),
           meta_fetched_at = coalesce(meta_fetched_at, (SELECT meta_fetched_at FROM item WHERE id = ?2))
         WHERE id = ?1",
        params![keep, drop],
    )?;
    // The names (migration 15; migration 14 joins before they exist).
    let names = has_names(c)?;
    if names {
        c.execute(
            "UPDATE item SET
               files_title = coalesce(files_title, (SELECT files_title FROM item WHERE id = ?2)),
               mb_title = coalesce(mb_title, (SELECT mb_title FROM item WHERE id = ?2)),
               own_title = coalesce(own_title, (SELECT own_title FROM item WHERE id = ?2)),
               name_choice = coalesce(name_choice, (SELECT name_choice FROM item WHERE id = ?2))
             WHERE id = ?1",
            params![keep, drop],
        )?;
    }
    c.execute("DELETE FROM match_candidate WHERE item_id = ?1", [drop])?;
    // Albums, artwork, the lock, aliases, queued jobs and the row itself.
    crate::merge::merge_into(c, keep, drop)?;
    if names {
        apply_name(c, keep)?;
    }
    Ok(())
}

fn has_names(c: &Connection) -> Result<bool> {
    Ok(c.query_row(
        "SELECT count(*) > 0 FROM pragma_table_info('item') WHERE name = 'mb_title'",
        [],
        |r| r.get(0),
    )?)
}

/// Join the artists that are the same: those with the same MusicBrainz artist ID, then those
/// whose keys (folded names) agree and that carry no different IDs. Returns each artist joined
/// away with the one it joined (`(gone, kept)`).
pub fn reconcile(c: &Connection) -> Result<Vec<(i64, i64)>> {
    let mut merged = Vec::new();
    // 1. The same artist ID.
    let shared: Vec<String> = {
        let mut st = c.prepare(
            "SELECT e.value FROM external_id e JOIN item i ON i.id = e.item_id
             WHERE e.provider = 'musicbrainz' AND e.kind = 'artist' AND i.kind = 'artist'
             GROUP BY e.value HAVING count(DISTINCT e.item_id) > 1",
        )?;
        st.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for id in shared {
        let artists: Vec<i64> = {
            let mut st = c.prepare(
                "SELECT DISTINCT e.item_id FROM external_id e JOIN item i ON i.id = e.item_id
                 WHERE e.provider = 'musicbrainz' AND e.kind = 'artist' AND e.value = ?1 AND i.kind = 'artist'
                 ORDER BY e.item_id",
            )?;
            st.query_map([&id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        if artists.len() < 2 {
            continue;
        }
        let keep = keeper(c, &artists)?;
        for a in artists.into_iter().filter(|a| *a != keep) {
            // An artist the user merged elsewhere may already be gone.
            if exists(c, a)? {
                merge(c, keep, a)?;
                merged.push((a, keep));
            }
        }
    }
    // 2. The same folded name, unless they carry different IDs.
    let keys: Vec<String> = {
        let mut st = c.prepare(
            "SELECT group_key FROM item WHERE kind = 'artist' AND group_key IS NOT NULL
             GROUP BY group_key HAVING count(*) > 1",
        )?;
        st.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for k in keys {
        let artists: Vec<i64> = {
            let mut st = c.prepare(
                "SELECT id FROM item WHERE kind = 'artist' AND group_key = ?1 ORDER BY id",
            )?;
            st.query_map([&k], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let mut distinct: Vec<String> = Vec::new();
        for a in &artists {
            for v in ids(c, *a)? {
                if !distinct.contains(&v) {
                    distinct.push(v);
                }
            }
        }
        if distinct.len() > 1 {
            continue;
        }
        let keep = keeper(c, &artists)?;
        for a in artists.into_iter().filter(|a| *a != keep) {
            merge(c, keep, a)?;
            merged.push((a, keep));
        }
    }
    Ok(merged)
}

// ---- the name shown (issue #148) ---------------------------------------------------------------

/// How the user chose to name an artist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameChoice {
    /// MusicBrainz's spelling when matched, else the files' (the default).
    Auto,
    /// As the files spell it, kept through every scan and match.
    Files,
    /// The user's own name.
    Own,
}

impl NameChoice {
    pub fn parse(s: &str) -> Self {
        match s {
            "files" => Self::Files,
            "own" => Self::Own,
            _ => Self::Auto,
        }
    }
    fn stored(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Files => Some("files"),
            Self::Own => Some("own"),
        }
    }
}

/// An artist's names: the one shown, where it comes from ("musicbrainz", "files" or "own"), the
/// files' and MusicBrainz's spellings, and the user's choice and own name.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Names {
    pub shown: String,
    pub source: String,
    pub files: String,
    pub musicbrainz: String,
    pub choice: String,
    pub own: String,
}

/// title, files_title, mb_title, name_choice, own_title.
type NameRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

pub fn names(c: &Connection, artist: i64) -> Result<Option<Names>> {
    let row: Option<NameRow> = c
        .query_row(
            "SELECT coalesce(title, ''), files_title, mb_title, name_choice, own_title FROM item WHERE id = ?1 AND kind = 'artist'",
            [artist],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((title, files, mb, choice, own)) = row else {
        return Ok(None);
    };
    let files = files.unwrap_or_else(|| title.clone());
    let mb = mb.unwrap_or_default();
    let own = own.unwrap_or_default();
    let choice = NameChoice::parse(choice.as_deref().unwrap_or(""));
    let (shown, source) = match choice {
        NameChoice::Own if !own.trim().is_empty() => (own.trim().to_string(), "own"),
        NameChoice::Files => (files.clone(), "files"),
        _ if !mb.is_empty() => (mb.clone(), "musicbrainz"),
        _ => (files.clone(), "files"),
    };
    Ok(Some(Names {
        shown,
        source: source.into(),
        files,
        musicbrainz: mb,
        choice: choice.stored().unwrap_or("auto").into(),
        own,
    }))
}

/// Show the name the rule picks: `title`, its sort title and the search index follow it, so
/// every list, search and credit shows it. Returns whether it changed. The files are never
/// touched.
pub fn apply_name(c: &Connection, artist: i64) -> Result<bool> {
    let Some(n) = names(c, artist)? else {
        return Ok(false);
    };
    let changed = c.execute(
        "UPDATE item SET title = ?2, sort_title = ?3 WHERE id = ?1 AND title IS NOT ?2",
        params![artist, n.shown, crate::text::sort_title(&n.shown)],
    )? > 0;
    if changed {
        c.execute("DELETE FROM item_fts WHERE rowid = ?1", [artist])?;
        c.execute(
            "INSERT INTO item_fts(rowid, title) VALUES (?1, ?2)",
            params![artist, n.shown],
        )?;
    }
    Ok(changed)
}

/// MusicBrainz's spelling of a matched artist (from its artist lookup).
pub fn set_mb_title(c: &Connection, artist: i64, name: &str) -> Result<bool> {
    c.execute(
        "UPDATE item SET mb_title = ?2 WHERE id = ?1",
        params![artist, name.trim()],
    )?;
    apply_name(c, artist)
}

/// The user's choice: MusicBrainz's spelling, the files', or their own name.
pub fn set_name_choice(c: &Connection, artist: i64, choice: NameChoice, own: &str) -> Result<bool> {
    c.execute(
        "UPDATE item SET name_choice = ?2, own_title = CASE WHEN ?2 = 'own' THEN ?3 ELSE own_title END WHERE id = ?1",
        params![artist, choice.stored(), own.trim()],
    )?;
    apply_name(c, artist)
}

/// Matched artists whose MusicBrainz spelling is not known yet.
pub fn names_due(c: &Connection) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT i.id FROM item i JOIN external_id e ON e.item_id = i.id
         WHERE i.kind = 'artist' AND i.mb_title IS NULL AND e.provider = 'musicbrainz' AND e.kind = 'artist'
           AND e.value <> ?1 ORDER BY i.id",
    )?;
    Ok(st
        .query_map([VARIOUS_MBID], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

// ---- Merge with… (issue #149) -----------------------------------------------------------------

/// Why two artists cannot be merged by hand, if they cannot.
pub fn merge_refusal(c: &Connection, a: i64, b: i64) -> Result<Option<String>> {
    if a == b {
        return Ok(Some("same".into()));
    }
    let kinds: i64 = c.query_row(
        "SELECT count(*) FROM item WHERE id IN (?1, ?2) AND kind = 'artist'",
        params![a, b],
        |r| r.get(0),
    )?;
    if kinds != 2 {
        return Ok(Some("not artists".into()));
    }
    let (ia, ib) = (ids(c, a)?, ids(c, b)?);
    if !ia.is_empty() && !ib.is_empty() && !ia.iter().any(|x| ib.contains(x)) {
        return Ok(Some("different MusicBrainz IDs".into()));
    }
    Ok(None)
}

/// An artist in the merge dialog.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: i64,
    pub name: String,
    pub albums: i64,
    pub songs: i64,
    pub matched: bool,
    /// Why it is likely the same ("same MusicBrainz ID", "similar name"), else "".
    pub why: String,
    /// Merging is refused (different MusicBrainz IDs).
    pub blocked: bool,
}

fn candidate(c: &Connection, id: i64, why: &str, other: i64) -> Result<Candidate> {
    let (name, albums, songs): (String, i64, i64) = c.query_row(
        "SELECT coalesce(i.title, ''),
                (SELECT count(*) FROM item a WHERE a.parent_id = i.id AND a.kind = 'album'),
                (SELECT count(DISTINCT t.id) FROM item t WHERE t.kind = 'track' AND (
                   t.parent_id IN (SELECT a.id FROM item a WHERE a.parent_id = i.id AND a.kind = 'album')
                   OR t.id IN (SELECT cr.item_id FROM credit cr WHERE cr.artist_id = i.id)))
         FROM item i WHERE i.id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok(Candidate {
        id,
        name,
        albums,
        songs,
        matched: !ids(c, id)?.is_empty(),
        why: why.into(),
        blocked: merge_refusal(c, id, other)?.is_some(),
    })
}

/// The artists to merge `artist` with: likely ones first (the same MusicBrainz ID, or a name
/// that contains the other: "Beyoncé" and "Beyoncé Knowles"), then the artists whose names
/// contain `query` (without case and accents), at most 50.
pub fn candidates(
    c: &Connection,
    artist: i64,
    query: &str,
) -> Result<(Vec<Candidate>, Vec<Candidate>)> {
    let me: String = c.query_row(
        "SELECT coalesce(title, '') FROM item WHERE id = ?1",
        [artist],
        |r| r.get(0),
    )?;
    let me_ids = ids(c, artist)?;
    let me_f = fold(&me);
    let all: Vec<(i64, String)> = {
        let mut st = c.prepare(
            "SELECT id, coalesce(title, '') FROM item WHERE kind = 'artist' AND id <> ?1 ORDER BY sort_title, id",
        )?;
        st.query_map([artist], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let q = fold(query);
    let (mut likely, mut others) = (Vec::new(), Vec::new());
    for (id, name) in all {
        let f = fold(&name);
        let same_id = !me_ids.is_empty() && ids(c, id)?.iter().any(|x| me_ids.contains(x));
        let similar = me_f.len() >= 3 && f.len() >= 3 && (f.contains(&me_f) || me_f.contains(&f));
        if same_id || similar {
            likely.push(candidate(
                c,
                id,
                if same_id {
                    "same MusicBrainz ID"
                } else {
                    "similar name"
                },
                artist,
            )?);
        } else if !q.is_empty() && f.contains(&q) && others.len() < 50 {
            others.push(candidate(c, id, "", artist)?);
        }
    }
    likely.truncate(10);
    Ok((likely, others))
}

/// What a merge of `drop` into `keep` would hold.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub keep: Candidate,
    pub drop: Candidate,
    pub albums: i64,
    pub songs: i64,
    pub playlist_entries: i64,
    pub plays: i64,
    pub refusal: Option<String>,
}

pub fn preview(c: &Connection, keep: i64, drop: i64) -> Result<Preview> {
    let (k, d) = (candidate(c, keep, "", drop)?, candidate(c, drop, "", keep)?);
    let tracks = "SELECT t.id FROM item t WHERE t.kind = 'track' AND (
                    t.parent_id IN (SELECT a.id FROM item a WHERE a.parent_id IN (?1, ?2) AND a.kind = 'album')
                    OR t.id IN (SELECT cr.item_id FROM credit cr WHERE cr.artist_id IN (?1, ?2)))";
    let songs: i64 = c.query_row(
        &format!("SELECT count(*) FROM ({tracks})"),
        params![keep, drop],
        |r| r.get(0),
    )?;
    let playlist_entries: i64 = c.query_row(
        &format!("SELECT count(*) FROM playlist_entry WHERE item_id IN ({tracks})"),
        params![keep, drop],
        |r| r.get(0),
    )?;
    let plays: i64 = c.query_row(
        &format!("SELECT coalesce(sum(play_count), 0) FROM playstate WHERE item_id IN ({tracks})"),
        params![keep, drop],
        |r| r.get(0),
    )?;
    Ok(Preview {
        albums: k.albums + d.albums,
        songs,
        playlist_entries,
        plays,
        refusal: merge_refusal(c, keep, drop)?,
        keep: k,
        drop: d,
    })
}

/// What a merge by hand changed, to undo it (kept in memory for a short while).
#[derive(Clone, Debug)]
pub struct Undo {
    pub keep: i64,
    pub drop: i64,
    keep_row: Vec<(String, rusqlite::types::Value)>,
    drop_row: Vec<(String, rusqlite::types::Value)>,
    albums: Vec<i64>,
    credits: Vec<(i64, String, i64)>,
    keep_ids: Vec<(String, String, String)>,
    drop_ids: Vec<(String, String, String)>,
    drop_art: Vec<(i64, Vec<rusqlite::types::Value>)>,
    drop_aliases: Vec<(String, String)>,
}

fn row(c: &Connection, id: i64) -> Result<Vec<(String, rusqlite::types::Value)>> {
    let mut st = c.prepare("SELECT * FROM item WHERE id = ?1")?;
    let names: Vec<String> = st.column_names().map(|s| s.to_string()).collect();
    Ok(st.query_row([id], |r| {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| Ok((n.clone(), r.get::<_, rusqlite::types::Value>(i)?)))
            .collect::<rusqlite::Result<Vec<_>>>()
    })?)
}

fn all_ids(c: &Connection, item: i64) -> Result<Vec<(String, String, String)>> {
    let mut st = c.prepare("SELECT provider, kind, value FROM external_id WHERE item_id = ?1")?;
    Ok(st
        .query_map([item], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

/// Merge `drop` into `keep` by hand: as an automatic join, remembered through `group_alias` so
/// a rescan does not split them; refused for artists with different MusicBrainz IDs. Returns
/// what [`undo`] needs.
pub fn merge_by_hand(c: &Connection, keep: i64, drop: i64) -> Result<Undo> {
    if let Some(r) = merge_refusal(c, keep, drop)? {
        return Err(crate::DbError::Other(format!("cannot merge: {r}")));
    }
    let art_cols = "role, remote_path, local_path, width, height, color";
    let drop_art: Vec<(i64, Vec<rusqlite::types::Value>)> = {
        let mut st = c.prepare(&format!(
            "SELECT rowid, {art_cols} FROM artwork WHERE item_id = ?1"
        ))?;
        st.query_map([drop], |r| {
            Ok((
                r.get(0)?,
                (1..=6)
                    .map(|i| r.get(i))
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    let undo = Undo {
        keep,
        drop,
        keep_row: row(c, keep)?,
        drop_row: row(c, drop)?,
        albums: {
            let mut st = c.prepare("SELECT id FROM item WHERE parent_id = ?1")?;
            st.query_map([drop], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        },
        credits: {
            let mut st =
                c.prepare("SELECT item_id, role, position FROM credit WHERE artist_id = ?1")?;
            st.query_map([drop], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<_>>()?
        },
        keep_ids: all_ids(c, keep)?,
        drop_ids: all_ids(c, drop)?,
        drop_art,
        drop_aliases: {
            let mut st = c.prepare("SELECT kind, key FROM group_alias WHERE item_id = ?1")?;
            st.query_map([drop], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        },
    };
    merge(c, keep, drop)?;
    Ok(undo)
}

/// Undo a merge by hand: the artist comes back with its albums, credits, IDs, artwork and keys,
/// and the kept artist is as it was.
pub fn undo(c: &Connection, u: &Undo) -> Result<()> {
    if exists(c, u.drop)? {
        return Ok(());
    }
    let cols: Vec<&str> = u.drop_row.iter().map(|(n, _)| n.as_str()).collect();
    let marks: Vec<String> = (1..=cols.len()).map(|i| format!("?{i}")).collect();
    let vals: Vec<&rusqlite::types::Value> = u.drop_row.iter().map(|(_, v)| v).collect();
    c.execute(
        &format!(
            "INSERT INTO item({}) VALUES ({})",
            cols.join(", "),
            marks.join(", ")
        ),
        rusqlite::params_from_iter(vals),
    )?;
    let title = u
        .drop_row
        .iter()
        .find(|(n, _)| n == "title")
        .and_then(|(_, v)| match v {
            rusqlite::types::Value::Text(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap_or_default();
    c.execute(
        "INSERT INTO item_fts(rowid, title) VALUES (?1, ?2)",
        params![u.drop, title],
    )?;
    for a in &u.albums {
        c.execute(
            "UPDATE item SET parent_id = ?1 WHERE id = ?2",
            params![u.drop, a],
        )?;
    }
    for (item, role, pos) in &u.credits {
        c.execute(
            "UPDATE credit SET artist_id = ?1 WHERE item_id = ?2 AND role = ?3 AND position = ?4",
            params![u.drop, item, role, pos],
        )?;
    }
    c.execute(
        "DELETE FROM external_id WHERE item_id IN (?1, ?2)",
        params![u.keep, u.drop],
    )?;
    for (item, list) in [(u.keep, &u.keep_ids), (u.drop, &u.drop_ids)] {
        for (p, k, v) in list {
            c.execute(
                "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value) VALUES (?1, ?2, ?3, ?4)",
                params![item, p, k, v],
            )?;
        }
    }
    for (rowid, vals) in &u.drop_art {
        c.execute("DELETE FROM artwork WHERE rowid = ?1", [rowid])?;
        c.execute(
            "INSERT INTO artwork(item_id, role, remote_path, local_path, width, height, color) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![u.drop, vals[0], vals[1], vals[2], vals[3], vals[4], vals[5]],
        )?;
    }
    let drop_key = u
        .drop_row
        .iter()
        .find(|(n, _)| n == "group_key")
        .map(|(_, v)| v.clone());
    if let Some(rusqlite::types::Value::Text(k)) = drop_key {
        c.execute(
            "DELETE FROM group_alias WHERE kind = 'artist' AND key = ?1 AND item_id = ?2",
            params![k, u.keep],
        )?;
    }
    for (kind, key) in &u.drop_aliases {
        c.execute(
            "UPDATE group_alias SET item_id = ?1 WHERE kind = ?2 AND key = ?3",
            params![u.drop, kind, key],
        )?;
    }
    // The kept artist as it was (its name, lock, match and facts).
    let pairs: Vec<&(String, rusqlite::types::Value)> =
        u.keep_row.iter().filter(|(n, _)| n != "id").collect();
    let sets: Vec<String> = pairs
        .iter()
        .enumerate()
        .map(|(i, (n, _))| format!("{n} = ?{}", i + 2))
        .collect();
    let mut vals: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::Integer(u.keep)];
    vals.extend(pairs.iter().map(|(_, v)| v.clone()));
    c.execute(
        &format!("UPDATE item SET {} WHERE id = ?1", sets.join(", ")),
        rusqlite::params_from_iter(vals),
    )?;
    apply_name(c, u.keep)?;
    c.execute("DELETE FROM item_fts WHERE rowid = ?1", [u.keep])?;
    let keep_title: String = c.query_row(
        "SELECT coalesce(title, '') FROM item WHERE id = ?1",
        [u.keep],
        |r| r.get(0),
    )?;
    c.execute(
        "INSERT INTO item_fts(rowid, title) VALUES (?1, ?2)",
        params![u.keep, keep_title],
    )?;
    Ok(())
}

fn exists(c: &Connection, item: i64) -> Result<bool> {
    Ok(
        c.query_row("SELECT 1 FROM item WHERE id = ?1", [item], |_| Ok(()))
            .optional()?
            .is_some(),
    )
}

/// Migration 14 (2.9.1, issue #146): artists keyed by their folded names, then joined by ID and
/// by name.
pub(crate) fn migration_14(tx: &rusqlite::Transaction) -> Result<()> {
    let rows: Vec<(i64, String)> = {
        let mut st =
            tx.prepare("SELECT id, coalesce(title, '') FROM item WHERE kind = 'artist'")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (id, name) in rows {
        // Keys of split or merged artists ("split:", aliases) are left as they are.
        tx.execute(
            "UPDATE item SET group_key = ?2 WHERE id = ?1 AND group_key LIKE 'artist:%'",
            params![id, key(&name)],
        )?;
    }
    let merged = reconcile(tx)?.len();
    tracing::info!(merged, "artists joined by MusicBrainz ID and name");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::migrate(&mut c).unwrap();
        c
    }

    fn artist(c: &Connection, id: i64, name: &str, mbid: Option<&str>, locked: bool) {
        c.execute(
            "INSERT INTO item(id, kind, title, sort_title, group_key, locked, match_state) VALUES (?1, 'artist', ?2, ?2, ?3, ?4, ?5)",
            params![id, name, key(name), locked as i64, if mbid.is_some() { "matched" } else { "unmatched" }],
        )
        .unwrap();
        if let Some(m) = mbid {
            c.execute(
                "INSERT INTO external_id(item_id, provider, kind, value) VALUES (?1, 'musicbrainz', 'artist', ?2)",
                params![id, m],
            )
            .unwrap();
        }
    }

    /// An album with one track, credited to `artist`, with a play count of `plays`.
    fn album(c: &Connection, id: i64, artist: i64, title: &str, plays: i64) {
        c.execute(
            "INSERT INTO item(id, kind, parent_id, title, sort_title) VALUES (?1, 'album', ?2, ?3, ?3)",
            params![id, artist, title],
        )
        .unwrap();
        c.execute(
            "INSERT INTO item(id, kind, parent_id, title, sort_title) VALUES (?1, 'track', ?2, 'Song', 'song')",
            params![id + 1, id],
        )
        .unwrap();
        c.execute(
            "INSERT INTO credit(item_id, artist_id, role, position) VALUES (?1, ?2, 'artist', 0), (?3, ?2, 'album_artist', 0)",
            params![id + 1, artist, id],
        )
        .unwrap();
        c.execute(
            "INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at, play_count) VALUES (?1, 1, 0, 0, 0, 1, ?2)",
            params![id + 1, plays],
        )
        .unwrap();
    }

    fn artists(c: &Connection) -> Vec<(i64, String)> {
        let mut st = c
            .prepare("SELECT id, title FROM item WHERE kind = 'artist' ORDER BY id")
            .unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    const ROSALIA: &str = "25f3abd9-63b5-471a-bd25-feb9672dfa11";

    #[test]
    fn names_fold_without_case_and_accents() {
        assert_eq!(fold("Rosalía"), fold("ROSALIA"));
        assert_eq!(key("Beyoncé"), "artist:beyonce");
        assert_ne!(fold("Rosalía"), fold("Rosana"));
    }

    #[test]
    fn the_same_artist_id_joins_and_keeps_everything() {
        let c = db();
        artist(&c, 1, "Rosalía", Some(ROSALIA), false);
        artist(&c, 2, "Rosalia (live)", Some(ROSALIA), true);
        album(&c, 10, 1, "Lux", 3);
        album(&c, 20, 2, "LUX", 5);
        c.execute_batch(
            "INSERT INTO playlist(id, name, created_at, updated_at) VALUES (1, 'Mine', 0, 0);
             INSERT INTO playlist_entry(playlist_id, position, item_id) VALUES (1, 0, 11), (1, 1, 21);",
        )
        .unwrap();
        let pairs = reconcile(&c).unwrap();
        assert_eq!(pairs, [(1, 2)], "the one whose match was chosen stays");
        assert_eq!(artists(&c).len(), 1);
        let albums: i64 = c
            .query_row(
                "SELECT count(*) FROM item WHERE kind = 'album' AND parent_id = 2",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(albums, 2);
        let credits: i64 = c
            .query_row("SELECT count(*) FROM credit WHERE artist_id = 2", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(credits, 4);
        let entries: i64 = c
            .query_row("SELECT count(*) FROM playlist_entry", [], |r| r.get(0))
            .unwrap();
        assert_eq!(entries, 2);
        let plays: i64 = c
            .query_row("SELECT sum(play_count) FROM playstate", [], |r| r.get(0))
            .unwrap();
        assert_eq!(plays, 8);
        let fk: i64 = c
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(fk, 0);
    }

    #[test]
    fn names_join_unless_the_ids_differ() {
        let c = db();
        artist(&c, 1, "Rosalia", None, false);
        artist(&c, 2, "ROSALÍA", None, false);
        artist(
            &c,
            3,
            "Nirvana",
            Some("5b11f4ce-a62d-471e-81fc-a69a8278c7da"),
            false,
        );
        artist(
            &c,
            4,
            "Nirvana",
            Some("c7b7e0e5-0000-4000-8000-000000000001"),
            false,
        );
        let pairs = reconcile(&c).unwrap();
        assert_eq!(pairs.len(), 1, "{pairs:?}");
        let left: Vec<i64> = artists(&c).into_iter().map(|(id, _)| id).collect();
        assert_eq!(left.len(), 3, "one Rosalía, two different Nirvanas");
        assert!(left.contains(&3) && left.contains(&4));
    }

    #[test]
    fn a_name_without_id_joins_the_one_with_its_id() {
        let c = db();
        artist(&c, 1, "Rosalia", None, false);
        artist(&c, 2, "Rosalía", Some(ROSALIA), false);
        album(&c, 10, 1, "LUX", 1);
        assert_eq!(reconcile(&c).unwrap(), [(1, 2)]);
        let id: i64 = c
            .query_row(
                "SELECT item_id FROM external_id WHERE kind = 'artist'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(id, 2);
        assert!(reconcile(&c).unwrap().is_empty(), "nothing left to join");
    }

    fn title(c: &Connection, id: i64) -> String {
        c.query_row("SELECT title FROM item WHERE id = ?1", [id], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn the_name_shown_follows_the_rule() {
        let c = db();
        artist(&c, 1, "Rosalia", Some(ROSALIA), true);
        c.execute("UPDATE item SET files_title = 'Rosalia' WHERE id = 1", [])
            .unwrap();
        // Matched, even with a match the user chose: MusicBrainz's spelling.
        assert!(set_mb_title(&c, 1, "Rosalía").unwrap());
        assert_eq!(title(&c, 1), "Rosalía");
        let n = names(&c, 1).unwrap().unwrap();
        assert_eq!(
            (n.source.as_str(), n.files.as_str()),
            ("musicbrainz", "Rosalia")
        );
        // The user's own name wins, also over a later MusicBrainz spelling.
        set_name_choice(&c, 1, NameChoice::Own, "ROSALÍA").unwrap();
        set_mb_title(&c, 1, "Rosalía Vila").unwrap();
        assert_eq!(title(&c, 1), "ROSALÍA");
        // Kept as in the files.
        set_name_choice(&c, 1, NameChoice::Files, "").unwrap();
        assert_eq!(title(&c, 1), "Rosalia");
        // Back to automatic.
        set_name_choice(&c, 1, NameChoice::Auto, "").unwrap();
        assert_eq!(title(&c, 1), "Rosalía Vila");
        let found: i64 = c
            .query_row(
                "SELECT rowid FROM item_fts WHERE item_fts MATCH 'vila'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(found, 1, "search follows the name");
        // A new match forgets the old spelling until the lookup brings the new one.
        crate::music::set_artist_match(&c, 1, "00000000-0000-4000-8000-000000000001", true)
            .unwrap();
        assert_eq!(title(&c, 1), "Rosalia");
    }

    #[test]
    fn a_join_keeps_the_names() {
        let c = db();
        artist(&c, 1, "Rosalia", None, false);
        artist(&c, 2, "Rosalía", Some(ROSALIA), false);
        c.execute("UPDATE item SET files_title = title", [])
            .unwrap();
        set_name_choice(&c, 1, NameChoice::Own, "ROSALÍA").unwrap();
        set_mb_title(&c, 2, "Rosalía").unwrap();
        assert_eq!(reconcile(&c).unwrap(), [(1, 2)]);
        assert_eq!(title(&c, 2), "ROSALÍA", "the user's name came along");
    }

    fn snapshot(c: &Connection) -> Vec<String> {
        let mut out = Vec::new();
        for sql in [
            "SELECT id || '|' || coalesce(parent_id, '') || '|' || coalesce(title, '') || '|' || locked FROM item ORDER BY id",
            "SELECT item_id || '|' || artist_id || '|' || role || '|' || position FROM credit ORDER BY 1",
            "SELECT item_id || '|' || kind || '|' || value FROM external_id ORDER BY 1",
            "SELECT item_id || '|' || role || '|' || coalesce(local_path, '') FROM artwork ORDER BY 1",
            "SELECT kind || '|' || key || '|' || item_id FROM group_alias ORDER BY 1",
        ] {
            let mut st = c.prepare(sql).unwrap();
            out.extend(
                st.query_map([], |r| r.get::<_, String>(0))
                    .unwrap()
                    .map(|r| r.unwrap()),
            );
        }
        out
    }

    #[test]
    fn merge_by_hand_and_undo() {
        let c = db();
        artist(
            &c,
            1,
            "Beyoncé",
            Some("859d0860-d480-4efd-970c-c05d5f1776b8"),
            false,
        );
        artist(&c, 2, "Beyonce Knowles", None, true);
        album(&c, 10, 1, "Lemonade", 2);
        album(&c, 20, 2, "Dangerously in Love", 3);
        c.execute(
            "INSERT INTO artwork(item_id, role, local_path) VALUES (2, 'poster', '/p2.jpg'), (1, 'poster', '/p1.jpg')",
            [],
        )
        .unwrap();
        let before = snapshot(&c);
        let (likely, _) = candidates(&c, 1, "").unwrap();
        assert_eq!(
            likely.iter().map(|a| a.id).collect::<Vec<_>>(),
            [2],
            "a name containing the other"
        );
        let p = preview(&c, 1, 2).unwrap();
        assert_eq!(
            (p.albums, p.songs, p.plays, p.refusal.clone()),
            (2, 2, 5, None)
        );
        let u = merge_by_hand(&c, 1, 2).unwrap();
        assert_eq!(artists(&c).len(), 1);
        let alias: i64 = c
            .query_row("SELECT item_id FROM group_alias WHERE kind = 'artist' AND key = 'artist:beyonce knowles'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(alias, 1, "a rescan of the old name finds the kept artist");
        undo(&c, &u).unwrap();
        assert_eq!(snapshot(&c), before, "everything as it was");
        let fk: i64 = c
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(fk, 0);
    }

    #[test]
    fn different_ids_cannot_be_merged() {
        let c = db();
        artist(
            &c,
            1,
            "Beyoncé",
            Some("859d0860-d480-4efd-970c-c05d5f1776b8"),
            false,
        );
        artist(
            &c,
            2,
            "Destiny's Child",
            Some("a0b6b5a2-0000-4000-8000-000000000002"),
            false,
        );
        assert!(merge_refusal(&c, 1, 2).unwrap().is_some());
        assert!(merge_by_hand(&c, 1, 2).is_err());
        assert_eq!(artists(&c).len(), 2);
        let (_, others) = candidates(&c, 1, "destiny").unwrap();
        assert!(others[0].blocked);
    }
}
