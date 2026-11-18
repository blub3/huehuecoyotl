//! Music matching in the database (issue #11, SPEC 7 "Providers"): what an album looks like to a
//! provider, and what a matched release adds. A release fills in what the tags lack (ids, release
//! type, date) and never overwrites a tag; a locked album is left alone.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// The `setting` key that turns online music information off ("0"); on by default (v1.9).
pub const ONLINE_SETTING: &str = "music_online";
/// An album without a match is tried again after a week, like video.
const RETRY_AFTER_MS: i64 = 7 * 24 * 3600 * 1000;

pub fn online(c: &Connection) -> Result<bool> {
    Ok(crate::copies::setting(c, ONLINE_SETTING)?.as_deref() != Some("0"))
}

pub fn set_online(c: &Connection, on: bool) -> Result<()> {
    crate::copies::set_setting(c, ONLINE_SETTING, Some(if on { "1" } else { "0" }))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlbumTarget {
    pub id: i64,
    pub title: String,
    /// The album artist's name ("Various Artists" for compilations).
    pub artist: String,
    pub tracks: u32,
    pub release_id: Option<String>,
    pub group_id: Option<String>,
    pub locked: bool,
    pub match_state: String,
    /// It has a cover in the grid tier already.
    pub has_cover: bool,
}

fn external(c: &Connection, item: i64, kind: &str) -> Result<Option<String>> {
    Ok(c
        .query_row(
            "SELECT value FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = ?2",
            params![item, kind],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn album_target(c: &Connection, album: i64) -> Result<Option<AlbumTarget>> {
    let row = c
        .query_row(
            "SELECT a.title, coalesce(p.title, ''), a.locked, coalesce(a.match_state, 'unmatched'),
                    (SELECT count(*) FROM item t WHERE t.parent_id = a.id AND t.kind = 'track'),
                    EXISTS (SELECT 1 FROM artwork w WHERE w.item_id = a.id AND w.role = 'poster_grid' AND w.local_path IS NOT NULL)
             FROM item a LEFT JOIN item p ON p.id = a.parent_id WHERE a.id = ?1 AND a.kind = 'album'",
            [album],
            |r| {
                Ok(AlbumTarget {
                    id: album,
                    title: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    artist: r.get(1)?,
                    locked: r.get::<_, i64>(2)? != 0,
                    match_state: r.get(3)?,
                    tracks: r.get::<_, i64>(4)? as u32,
                    has_cover: r.get(5)?,
                    ..Default::default()
                })
            },
        )
        .optional()?;
    let Some(mut t) = row else {
        return Ok(None);
    };
    t.release_id = external(c, album, "release")?;
    t.group_id = external(c, album, "release_group")?;
    Ok(Some(t))
}

/// Albums to look up: not matched yet (or for review) and not tried for a week, not locked.
pub fn due(c: &Connection) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT id FROM item WHERE kind = 'album' AND locked = 0
           AND coalesce(match_state, 'unmatched') IN ('unmatched', 'needs_review')
           AND (meta_fetched_at IS NULL OR meta_fetched_at < ?1)",
    )?;
    let rows = st.query_map([now_ms() - RETRY_AFTER_MS], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// What a matched release says, without the provider's types.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReleaseFacts {
    pub id: String,
    pub group_id: Option<String>,
    pub release_type: Option<String>,
    pub compilation: bool,
    pub date: Option<String>,
    /// (name, id) of the release's artists.
    pub artists: Vec<(String, String)>,
    /// (disc, position, title, recording id)
    pub tracks: Vec<(u32, u32, String, String)>,
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn set_external(c: &Connection, item: i64, kind: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Ok(());
    }
    let has: bool = c.query_row(
        "SELECT EXISTS (SELECT 1 FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = ?2)",
        params![item, kind],
        |r| r.get(0),
    )?;
    if !has {
        c.execute(
            "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value) VALUES (?1, 'musicbrainz', ?2, ?3)",
            params![item, kind, value.to_lowercase()],
        )?;
    }
    Ok(())
}

/// Issue #102: another release replaces the album's match: its old release, release group and
/// recording ids go first, then the new release applies as usual.
pub fn replace_release(c: &mut Connection, album: i64, r: &ReleaseFacts) -> Result<()> {
    c.execute(
        "DELETE FROM external_id WHERE provider = 'musicbrainz' AND (
           (item_id = ?1 AND kind IN ('release', 'release_group'))
           OR (kind = 'recording' AND item_id IN (SELECT id FROM item WHERE parent_id = ?1 AND kind = 'track')))",
        [album],
    )?;
    apply_release(c, album, r)
}

/// A matched release: ids, release type and date where the tags had none; the album artists'
/// and the tracks' ids (tracks by disc and position, or by title when the counts differ).
pub fn apply_release(c: &mut Connection, album: i64, r: &ReleaseFacts) -> Result<()> {
    let tx = c.transaction()?;
    set_external(&tx, album, "release", &r.id)?;
    if let Some(g) = &r.group_id {
        set_external(&tx, album, "release_group", g)?;
    }
    tx.execute(
        "INSERT INTO music(item_id, release_type, compilation, release_date) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(item_id) DO UPDATE SET
           release_type = coalesce(music.release_type, excluded.release_type),
           compilation = max(music.compilation, excluded.compilation),
           release_date = coalesce(music.release_date, excluded.release_date)",
        params![album, r.release_type, r.compilation, r.date],
    )?;
    let year: Option<i64> = r
        .date
        .as_deref()
        .and_then(|d| d.get(..4))
        .and_then(|y| y.parse().ok());
    tx.execute(
        "UPDATE item SET year = coalesce(year, ?2) WHERE id = ?1",
        params![album, year],
    )?;
    // Album artists by name.
    let artists: Vec<(i64, String)> = {
        let mut st = tx.prepare(
            "SELECT a.id, a.title FROM credit c JOIN item a ON a.id = c.artist_id WHERE c.item_id = ?1
             UNION SELECT p.id, p.title FROM item al JOIN item p ON p.id = al.parent_id WHERE al.id = ?1",
        )?;
        st.query_map([album], |row| {
            Ok((
                row.get(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    for (id, name) in &artists {
        if let Some((_, mb)) = r.artists.iter().find(|(n, _)| norm(n) == norm(name)) {
            set_external(&tx, *id, "artist", mb)?;
        }
    }
    // Tracks.
    let local: Vec<(i64, u32, u32, String)> = {
        let mut st = tx.prepare(
            "SELECT id, coalesce(season_no, 1), coalesce(episode_no, 0), coalesce(title, '') FROM item WHERE parent_id = ?1 AND kind = 'track'",
        )?;
        st.query_map([album], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    let by_position = local.len() == r.tracks.len();
    for (id, disc, pos, title) in &local {
        let hit = r.tracks.iter().find(|(d, p, t, _)| {
            if by_position {
                d == disc && p == pos
            } else {
                norm(t) == norm(title)
            }
        });
        if let Some((_, _, _, rec)) = hit {
            set_external(&tx, *id, "recording", rec)?;
        }
    }
    tx.execute(
        "UPDATE item SET match_state = 'matched', meta_fetched_at = ?2 WHERE id = ?1",
        params![album, now_ms()],
    )?;
    tx.commit()?;
    Ok(())
}

// ---- artist extras (issue #12) ---------------------------------------------------------------

/// An artist to look up at TheAudioDB and Fanart.tv: its MusicBrainz id, and its albums with
/// their release groups and whether they have a cover and disc art already.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArtistTarget {
    pub id: i64,
    pub mbid: String,
    pub albums: Vec<AlbumArtTarget>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlbumArtTarget {
    pub id: i64,
    pub group: Option<String>,
    pub has_cover: bool,
    pub has_disc: bool,
}

pub fn artist_target(c: &Connection, artist: i64) -> Result<Option<ArtistTarget>> {
    let Some(mbid) = external(c, artist, "artist")? else {
        return Ok(None);
    };
    let mut st = c.prepare(
        "SELECT a.id,
                (SELECT value FROM external_id e WHERE e.item_id = a.id AND e.provider = 'musicbrainz' AND e.kind = 'release_group'),
                EXISTS (SELECT 1 FROM artwork w WHERE w.item_id = a.id AND w.role = 'poster' AND w.local_path IS NOT NULL),
                EXISTS (SELECT 1 FROM artwork w WHERE w.item_id = a.id AND w.role = 'disc' AND w.local_path IS NOT NULL)
         FROM item a WHERE a.kind = 'album' AND a.parent_id = ?1 ORDER BY a.id",
    )?;
    let albums = st
        .query_map([artist], |r| {
            Ok(AlbumArtTarget {
                id: r.get(0)?,
                group: r.get(1)?,
                has_cover: r.get(2)?,
                has_disc: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(ArtistTarget {
        id: artist,
        mbid,
        albums,
    }))
}

/// Artists with a MusicBrainz id that were not looked up at the optional providers yet.
pub fn artists_due(c: &Connection) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT i.id FROM item i WHERE i.kind = 'artist' AND i.meta_fetched_at IS NULL
           AND EXISTS (SELECT 1 FROM external_id e WHERE e.item_id = i.id AND e.provider = 'musicbrainz' AND e.kind = 'artist')
         ORDER BY i.id",
    )?;
    Ok(st
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

/// A new key: every artist is looked up again.
pub fn reset_artists(c: &Connection) -> Result<()> {
    c.execute(
        "UPDATE item SET meta_fetched_at = NULL WHERE kind = 'artist'",
        [],
    )?;
    Ok(())
}

/// The artist was looked up; keep the biography when there is one.
pub fn set_artist_looked_up(c: &Connection, artist: i64, biography: Option<&str>) -> Result<()> {
    c.execute(
        "UPDATE item SET overview = coalesce(?2, overview), meta_fetched_at = ?3 WHERE id = ?1",
        params![artist, biography, now_ms()],
    )?;
    Ok(())
}

/// One candidate for "Review matches": the release id, its title, artist, year, score and track
/// count.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Candidate {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<u16>,
    pub score: f64,
    pub tracks: Option<u32>,
}

/// Not certain: the album waits for the user's choice, with these candidates. Its tags stay.
pub fn set_review(c: &mut Connection, album: i64, cands: &[Candidate]) -> Result<()> {
    let tx = c.transaction()?;
    tx.execute(
        "UPDATE item SET match_state = 'needs_review', meta_fetched_at = ?2 WHERE id = ?1",
        params![album, now_ms()],
    )?;
    tx.execute("DELETE FROM match_candidate WHERE item_id = ?1", [album])?;
    for k in cands {
        let about = match k.tracks {
            Some(n) => format!("{} · {n} tracks", k.artist),
            None => k.artist.clone(),
        };
        tx.execute(
            "INSERT INTO match_candidate(item_id, provider_id, title, year, score, overview) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![album, k.id, k.title, k.year, k.score, about],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Nothing found: tried again after a week.
pub fn set_unmatched(c: &Connection, album: i64) -> Result<()> {
    c.execute(
        "UPDATE item SET match_state = 'unmatched', meta_fetched_at = ?2 WHERE id = ?1",
        params![album, now_ms()],
    )?;
    Ok(())
}

// ---- the album and artist pages (issue #10) -------------------------------------------------

/// Lossless with more than 16 bits or above 48 kHz (SPEC 2, "Music in 2.0").
pub fn hires(lossless: bool, depth: Option<i64>, rate: Option<i64>) -> bool {
    lossless && (depth.unwrap_or(0) > 16 || rate.unwrap_or(0) > 48_000)
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRow {
    pub id: i64,
    pub disc: i64,
    pub number: i64,
    pub title: String,
    /// "A feat. B", from the credits.
    pub artists: String,
    pub duration_ms: i64,
    pub codec: String,
    pub sample_rate: i64,
    pub bit_depth: i64,
    pub lossless: bool,
    pub hires: bool,
    pub play_count: i64,
    pub saved: bool,
    /// Issue #63: the file that plays in the selected version, and the track's files there when
    /// it has more than one (a replaced file); empty otherwise.
    pub file_id: i64,
    pub files: Vec<crate::versions::TrackFile>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumDetail {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub artist_id: i64,
    pub year: Option<i64>,
    pub release_type: String,
    pub compilation: bool,
    pub genre: String,
    pub poster: String,
    pub poster_full: String,
    /// Issue #12: disc art from an optional provider (a local path), else "".
    pub disc: String,
    pub duration_ms: i64,
    /// Every track's codec, sample rate and bit depth, when they are all the same; else "".
    pub codec: String,
    pub sample_rate: i64,
    pub bit_depth: i64,
    pub hires: bool,
    pub discs: i64,
    pub tracks: Vec<TrackRow>,
    /// Issue #63: the album's versions (best first), the one selected, whether it can be played,
    /// and whether no version can (every source offline, no saved copy).
    pub versions: Vec<crate::versions::VersionInfo>,
    pub version_id: i64,
    pub playable: bool,
    pub all_offline: bool,
}

pub(crate) fn art(c: &Connection, item: i64, role: &str) -> Result<String> {
    Ok(c.query_row(
        "SELECT local_path FROM artwork WHERE item_id = ?1 AND role = ?2 AND local_path IS NOT NULL",
        params![item, role],
        |r| r.get(0),
    )
    .optional()?
    .unwrap_or_default())
}

pub(crate) fn credit_line(c: &Connection, item: i64, role: &str) -> Result<String> {
    let mut st = c.prepare_cached(
        "SELECT a.title, cr.join_phrase FROM credit cr JOIN item a ON a.id = cr.artist_id
         WHERE cr.item_id = ?1 AND cr.role = ?2 ORDER BY cr.position",
    )?;
    let parts = st
        .query_map(params![item, role], |r| {
            Ok(format!(
                "{}{}",
                r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                r.get::<_, String>(1)?
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(parts.concat())
}

/// An album's page (issue #10) for a version (issue #63): `want` (else the one played last, else
/// by `preference`); `offline` names sources that cannot be reached.
pub fn album_detail(
    c: &Connection,
    album: i64,
    want: Option<i64>,
    preference: &str,
    offline: &std::collections::HashSet<i64>,
) -> Result<Option<AlbumDetail>> {
    let head = c
        .query_row(
            "SELECT a.title, coalesce(p.title, ''), coalesce(a.parent_id, 0), a.year,
                    coalesce(m.release_type, ''), coalesce(m.compilation, 0), coalesce(m.genre, '')
             FROM item a LEFT JOIN item p ON p.id = a.parent_id LEFT JOIN music m ON m.item_id = a.id
             WHERE a.id = ?1 AND a.kind = 'album'",
            [album],
            |r| {
                Ok(AlbumDetail {
                    id: album,
                    title: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    artist: r.get(1)?,
                    artist_id: r.get(2)?,
                    year: r.get(3)?,
                    release_type: r.get(4)?,
                    compilation: r.get::<_, i64>(5)? != 0,
                    genre: r.get(6)?,
                    ..Default::default()
                })
            },
        )
        .optional()?;
    let Some(mut d) = head else {
        return Ok(None);
    };
    d.poster = art(c, album, "poster_grid")?;
    d.poster_full = art(c, album, "poster")?;
    d.disc = art(c, album, "disc")?;
    // Issue #63: the versions, and the selected one's tracks.
    let mut versions = crate::versions::versions(c, album)?;
    for v in &mut versions {
        v.online = !offline.contains(&v.source_id);
    }
    if !versions.is_empty() {
        let choices: Vec<crate::versions::Choice> =
            versions.iter().map(crate::versions::Choice::from).collect();
        let selected = want
            .filter(|w| versions.iter().any(|v| v.id == *w))
            .or(crate::versions::preselect(
                &choices,
                crate::versions::last_version(c, album)?,
                preference,
            ))
            .unwrap_or(versions[0].id);
        let sel = versions
            .iter()
            .find(|v| v.id == selected)
            .cloned()
            .unwrap_or_default();
        d.version_id = selected;
        d.playable = sel.online || sel.saved;
        d.all_offline = versions.iter().all(|v| !(v.online || v.saved));
        for (t, file) in crate::versions::version_tracks(c, selected, preference)? {
            let mut row = c.query_row(
                "SELECT coalesce(t.season_no, 1), coalesce(t.episode_no, 0), coalesce(t.title, ''),
                        coalesce(fa.duration_ms, 0), coalesce(fa.codec, ''), coalesce(fa.sample_rate, 0),
                        coalesce(fa.bit_depth, 0), coalesce(fa.lossless, 0), coalesce(p.play_count, 0),
                        EXISTS (SELECT 1 FROM local_copy lc WHERE lc.file_id = ?2 AND lc.kind = 'saved' AND lc.main_id IS NULL AND lc.state = 'done')
                 FROM item t LEFT JOIN file_audio fa ON fa.file_id = ?2
                 LEFT JOIN playstate p ON p.item_id = t.id AND p.profile_id = 1
                 WHERE t.id = ?1",
                params![t, file],
                |r| {
                    let lossless = r.get::<_, i64>(7)? != 0;
                    let (depth, rate): (i64, i64) = (r.get(6)?, r.get(5)?);
                    Ok(TrackRow {
                        id: t,
                        disc: r.get(0)?,
                        number: r.get(1)?,
                        title: r.get(2)?,
                        duration_ms: r.get(3)?,
                        codec: r.get(4)?,
                        sample_rate: rate,
                        bit_depth: depth,
                        lossless,
                        hires: hires(lossless, Some(depth), Some(rate)),
                        play_count: r.get(8)?,
                        saved: r.get(9)?,
                        file_id: file,
                        ..Default::default()
                    })
                },
            )?;
            let files = crate::versions::track_files(c, t, selected)?;
            if files.len() > 1 {
                row.files = files;
            }
            d.tracks.push(row);
        }
        d.versions = versions;
    }
    let mut st = c.prepare(
        "SELECT t.id, coalesce(t.season_no, 1), coalesce(t.episode_no, 0), coalesce(t.title, ''),
                coalesce(fa.duration_ms, 0), coalesce(fa.codec, ''), coalesce(fa.sample_rate, 0),
                coalesce(fa.bit_depth, 0), coalesce(fa.lossless, 0), coalesce(p.play_count, 0),
                EXISTS (SELECT 1 FROM local_copy lc WHERE lc.item_id = t.id AND lc.kind = 'saved' AND lc.main_id IS NULL AND lc.state = 'done')
         FROM item t
         LEFT JOIN file f ON f.id = (SELECT id FROM file WHERE item_id = t.id ORDER BY size DESC LIMIT 1)
         LEFT JOIN file_audio fa ON fa.file_id = f.id
         LEFT JOIN playstate p ON p.item_id = t.id AND p.profile_id = 1
         WHERE t.parent_id = ?1 AND t.kind = 'track'
         ORDER BY 2, 3, t.sort_title",
    )?;
    let rows = st.query_map([album], |r| {
        let lossless = r.get::<_, i64>(8)? != 0;
        let (depth, rate): (i64, i64) = (r.get(7)?, r.get(6)?);
        Ok(TrackRow {
            id: r.get(0)?,
            disc: r.get(1)?,
            number: r.get(2)?,
            title: r.get(3)?,
            duration_ms: r.get(4)?,
            codec: r.get(5)?,
            sample_rate: rate,
            bit_depth: depth,
            lossless,
            hires: hires(lossless, Some(depth), Some(rate)),
            play_count: r.get(9)?,
            saved: r.get(10)?,
            ..Default::default()
        })
    })?;
    // Without versions (a library before migration 9 in tests): every track, its largest file.
    if d.versions.is_empty() {
        d.tracks = rows.collect::<rusqlite::Result<_>>()?;
        d.playable = !d.tracks.is_empty();
    } else {
        drop(rows);
    }
    for t in &mut d.tracks {
        t.artists = credit_line(c, t.id, "artist")?;
    }
    d.duration_ms = d.tracks.iter().map(|t| t.duration_ms).sum();
    d.discs = d.tracks.iter().map(|t| t.disc).max().unwrap_or(1);
    d.hires = !d.tracks.is_empty() && d.tracks.iter().all(|t| t.hires);
    if let Some(f) = d.tracks.first()
        && d.tracks.iter().all(|t| {
            t.codec == f.codec && t.sample_rate == f.sample_rate && t.bit_depth == f.bit_depth
        })
    {
        (d.codec, d.sample_rate, d.bit_depth) = (f.codec.clone(), f.sample_rate, f.bit_depth);
    }
    Ok(Some(d))
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumCard {
    pub id: i64,
    pub title: String,
    pub year: Option<i64>,
    pub release_type: String,
    pub poster: String,
    pub poster_large: String,
    pub hires: bool,
    /// Issue #63: how many versions the album has.
    pub versions: i64,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistDetail {
    pub id: i64,
    pub title: String,
    /// From an optional provider (issue #12); "" otherwise: the artist's image, a background
    /// and a logo (local paths), and the biography in the UI language or English.
    pub image: String,
    pub background: String,
    pub logo: String,
    pub biography: String,
    /// Grouped in this order: album, ep, single, compilation, other; each by year.
    pub albums: Vec<AlbumCard>,
    /// Albums by others with a track credited to this artist ("Appears on").
    pub appears_on: Vec<AlbumCard>,
}

/// The release type an album is listed under on its artist's page.
pub fn group_of(release_type: &str, compilation: bool) -> &'static str {
    if compilation {
        return "compilation";
    }
    match release_type {
        "album" => "album",
        "ep" => "ep",
        "single" => "single",
        "compilation" => "compilation",
        "" => "album",
        _ => "other",
    }
}

fn cards(c: &Connection, sql: &str, artist: i64) -> Result<Vec<AlbumCard>> {
    let mut st = c.prepare(sql)?;
    let rows = st.query_map([artist], |r| {
        let rt: String = r.get(3)?;
        let comp = r.get::<_, i64>(4)? != 0;
        Ok(AlbumCard {
            id: r.get(0)?,
            title: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            year: r.get(2)?,
            release_type: group_of(&rt, comp).to_string(),
            hires: r.get::<_, i64>(5)? == 0 && r.get::<_, i64>(6)? > 0,
            ..Default::default()
        })
    })?;
    let mut out: Vec<AlbumCard> = rows.collect::<rusqlite::Result<_>>()?;
    for a in &mut out {
        a.poster = art(c, a.id, "poster_grid")?;
        a.poster_large = art(c, a.id, "poster_grid_l")?;
        a.versions = c.query_row(
            "SELECT count(*) FROM album_version WHERE album_id = ?1",
            [a.id],
            |r| r.get(0),
        )?;
    }
    Ok(out)
}

/// Album columns for a card: id, title, year, release type, compilation, tracks that are not
/// Hi-Res, tracks.
const CARD_COLS: &str = "a.id, a.title, a.year, coalesce(m.release_type, ''), coalesce(m.compilation, 0),
    (SELECT count(*) FROM item t JOIN file f ON f.item_id = t.id LEFT JOIN file_audio fa ON fa.file_id = f.id
      WHERE t.parent_id = a.id AND NOT (coalesce(fa.lossless, 0) = 1 AND (coalesce(fa.bit_depth, 0) > 16 OR coalesce(fa.sample_rate, 0) > 48000))),
    (SELECT count(*) FROM item t WHERE t.parent_id = a.id)";

pub fn artist_detail(c: &Connection, artist: i64) -> Result<Option<ArtistDetail>> {
    let title: Option<String> = c
        .query_row(
            "SELECT coalesce(title, '') FROM item WHERE id = ?1 AND kind = 'artist'",
            [artist],
            |r| r.get(0),
        )
        .optional()?;
    let Some(title) = title else {
        return Ok(None);
    };
    let order = ["album", "ep", "single", "compilation", "other"];
    let mut albums = cards(
        c,
        &format!(
            "SELECT {CARD_COLS} FROM item a LEFT JOIN music m ON m.item_id = a.id
             WHERE a.kind = 'album' AND a.parent_id = ?1 ORDER BY coalesce(a.year, 9999), a.sort_title"
        ),
        artist,
    )?;
    albums.sort_by_key(|a| order.iter().position(|o| *o == a.release_type).unwrap_or(9));
    let appears_on = cards(
        c,
        &format!(
            "SELECT DISTINCT {CARD_COLS} FROM item a LEFT JOIN music m ON m.item_id = a.id
             WHERE a.kind = 'album' AND coalesce(a.parent_id, 0) != ?1
               AND EXISTS (SELECT 1 FROM item t JOIN credit cr ON cr.item_id = t.id WHERE t.parent_id = a.id AND cr.artist_id = ?1)
             ORDER BY coalesce(a.year, 9999), a.sort_title"
        ),
        artist,
    )?;
    let biography: String = c.query_row(
        "SELECT coalesce(overview, '') FROM item WHERE id = ?1",
        [artist],
        |r| r.get(0),
    )?;
    Ok(Some(ArtistDetail {
        id: artist,
        title,
        image: art(c, artist, "poster")?,
        background: art(c, artist, "backdrop")?,
        logo: art(c, artist, "logo")?,
        biography,
        albums,
        appears_on,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrate(&mut c).unwrap();
        c.execute_batch(
            "INSERT INTO source(id, kind, name, root, content) VALUES (2, 'local', 'Music', '/m', 'music');
             INSERT INTO item(id, kind, title, sort_title) VALUES (1, 'artist', 'Aurora', 'aurora'), (2, 'artist', 'Band B', 'band b'),
               (3, 'artist', 'Various Artists', 'various artists');
             INSERT INTO item(id, kind, title, sort_title, year, parent_id) VALUES
               (10, 'album', 'Skies', 'skies', 2019, 1), (11, 'album', 'Light', 'light', 2021, 1), (12, 'album', 'Mix', 'mix', 2020, 3);
             INSERT INTO music(item_id, release_type) VALUES (10, 'album'), (11, 'single');
             INSERT INTO music(item_id, compilation) VALUES (12, 1);
             INSERT INTO item(id, kind, title, sort_title, parent_id, season_no, episode_no) VALUES
               (20, 'track', 'Glow', 'glow', 10, 1, 1), (21, 'track', 'Drift', 'drift', 10, 2, 1),
               (22, 'track', 'Light', 'light', 11, 1, 1), (23, 'track', 'Summer', 'summer', 12, 1, 1);
             INSERT INTO file(id, source_id, path, size, item_id) VALUES (30, 2, 'a.flac', 1, 20), (31, 2, 'b.flac', 1, 21), (32, 2, 'c.mp3', 1, 22), (33, 2, 'd.m4a', 1, 23);
             INSERT INTO file_audio(file_id, codec, sample_rate, bit_depth, lossless, duration_ms) VALUES
               (30, 'flac', 96000, 24, 1, 200000), (31, 'flac', 96000, 24, 1, 100000), (32, 'mp3', 44100, NULL, 0, 180000), (33, 'aac', 44100, NULL, 0, 150000);
             INSERT INTO credit(item_id, artist_id, role, position, join_phrase) VALUES
               (20, 1, 'artist', 0, ''), (21, 1, 'artist', 0, ''), (22, 1, 'artist', 0, ''),
               (23, 2, 'artist', 0, ' feat. '), (23, 1, 'artist', 1, '');",
        )
        .unwrap();
        c
    }

    #[test]
    fn the_album_page() {
        let c = db();
        let d = album_detail(&c, 10, None, "quality", &Default::default())
            .unwrap()
            .unwrap();
        assert_eq!(
            (d.title.as_str(), d.artist.as_str(), d.artist_id, d.year),
            ("Skies", "Aurora", 1, Some(2019))
        );
        assert_eq!(
            d.tracks
                .iter()
                .map(|t| (t.disc, t.number, t.title.as_str()))
                .collect::<Vec<_>>(),
            [(1, 1, "Glow"), (2, 1, "Drift")]
        );
        assert_eq!((d.discs, d.duration_ms), (2, 300_000));
        assert_eq!(
            (d.codec.as_str(), d.sample_rate, d.bit_depth, d.hires),
            ("flac", 96000, 24, true)
        );
        let mix = album_detail(&c, 12, None, "quality", &Default::default())
            .unwrap()
            .unwrap();
        assert!(!mix.hires);
        assert_eq!(mix.tracks[0].artists, "Band B feat. Aurora");
        assert!(
            album_detail(&c, 20, None, "quality", &Default::default())
                .unwrap()
                .is_none(),
            "a track is not an album"
        );
    }

    #[test]
    fn the_artist_page() {
        let c = db();
        let a = artist_detail(&c, 1).unwrap().unwrap();
        assert_eq!(
            a.albums
                .iter()
                .map(|x| (x.title.as_str(), x.release_type.as_str(), x.hires))
                .collect::<Vec<_>>(),
            [("Skies", "album", true), ("Light", "single", false)]
        );
        // A compilation with a track credited to the artist.
        assert_eq!(
            a.appears_on
                .iter()
                .map(|x| x.title.as_str())
                .collect::<Vec<_>>(),
            ["Mix"]
        );
        assert_eq!(group_of("", true), "compilation");
        assert_eq!(group_of("broadcast", false), "other");
    }

    #[test]
    fn hires_needs_lossless_and_more_than_cd_quality() {
        assert!(hires(true, Some(24), Some(44_100)));
        assert!(hires(true, Some(16), Some(96_000)));
        assert!(!hires(true, Some(16), Some(48_000)));
        assert!(!hires(false, Some(24), Some(96_000)));
    }
}

// ---- Now Playing (issue #82) ---------------------------------------------------------------

/// What Now Playing shows for a song besides its tags: the album cover's stored background
/// colour (or, when there is none yet, the cover file to compute it from) and the stored
/// waveform of the file that plays.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Visuals {
    pub album: i64,
    pub file: i64,
    pub color: Option<String>,
    pub cover_path: Option<String>,
    pub waveform: Option<Vec<u8>>,
}

/// `file` 0: the song's best file.
pub fn visuals(c: &Connection, track: i64, file: i64) -> Result<Visuals> {
    let album: i64 = c
        .query_row(
            "SELECT coalesce(parent_id, 0) FROM item WHERE id = ?1",
            [track],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);
    let file = if file > 0 {
        file
    } else {
        c.query_row(
            "SELECT f.id FROM file f LEFT JOIN file_audio fa ON fa.file_id = f.id WHERE f.item_id = ?1
             ORDER BY coalesce(fa.lossless, 0) DESC, coalesce(fa.bit_depth, 0) DESC, coalesce(fa.sample_rate, 0) DESC LIMIT 1",
            [track],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0)
    };
    let (color, cover_path): (Option<String>, Option<String>) = c
        .query_row(
            "SELECT color, local_path FROM artwork WHERE item_id = ?1 AND role = 'poster_grid' AND local_path IS NOT NULL",
            [album],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .unwrap_or((None, None));
    let waveform: Option<Vec<u8>> = c
        .query_row(
            "SELECT waveform FROM file_audio WHERE file_id = ?1",
            [file],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(Visuals {
        album,
        file,
        color,
        cover_path,
        waveform,
    })
}

pub fn set_cover_color(c: &Connection, album: i64, color: &str) -> Result<()> {
    c.execute(
        "UPDATE artwork SET color = ?2 WHERE item_id = ?1 AND role = 'poster_grid'",
        params![album, color],
    )?;
    Ok(())
}

/// A file's waveform; files without audio details get a row for it.
pub fn set_waveform(c: &Connection, file: i64, peaks: &[u8]) -> Result<()> {
    c.execute(
        "INSERT INTO file_audio(file_id, waveform) VALUES (?1, ?2)
         ON CONFLICT(file_id) DO UPDATE SET waveform = excluded.waveform",
        params![file, peaks],
    )?;
    Ok(())
}

/// Where a music file is, for the waveform job: (source, source kind, path, size, track).
pub type FileTarget = (i64, String, String, u64, i64);

pub fn file_target(c: &Connection, file: i64) -> Result<Option<FileTarget>> {
    Ok(c
        .query_row(
            &format!(
                "SELECT f.source_id, s.kind, {}, coalesce(f.size, 0), f.item_id
                 FROM file f JOIN source s ON s.id = f.source_id WHERE f.id = ?1 AND f.item_id IS NOT NULL",
                crate::catalog::ABS_PATH_SQL
            ),
            [file],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)?.max(0) as u64, r.get(4)?)),
        )
        .optional()?)
}
