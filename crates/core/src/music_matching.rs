//! Matching music again (2.7, issues #101 to #107): songs get a MusicBrainz recording of their
//! own, artists a match of their own (chosen among artists who share a name by the user's
//! albums), and an artist without a match takes the one its album's release or song's
//! recording names, then brings its other unmatched albums along.

use std::sync::Arc;

use huehuecoyotl_library::{DbError, jobs, music, rusqlite};
use huehuecoyotl_metadata::MetaError;
use huehuecoyotl_metadata::ids::{self, Ref};
use huehuecoyotl_metadata::music::{Artist, Recording, norm};
use serde_json::{Value, json};

use crate::matching::Outcome;
use crate::{CResult, CoreError, ErrorKind, Event, Inner};

fn db(e: DbError) -> MetaError {
    MetaError::Other(e.to_string())
}

/// A length difference of at most this many milliseconds counts as the same song.
const SAME_LENGTH_MS: u64 = 3_000;

fn time(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// After a merge kept another album, "matched through" points at it.
pub(crate) fn move_via(c: &rusqlite::Connection, from: i64, to: i64) -> Result<(), DbError> {
    if from != to {
        c.execute(
            "UPDATE external_id SET value = ?2 WHERE provider = 'coyotl' AND kind = 'matched_via' AND value = ?1",
            [format!("album:{from}"), format!("album:{to}")],
        )?;
    }
    Ok(())
}

/// Issue #147: one album per release after a scan or a match. Albums joined away hand over the
/// artists that took their match from them.
pub(crate) async fn reconcile_releases(inner: &Arc<Inner>) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let done = inner
        .lib
        .write(move |c| {
            let tx = c.transaction()?;
            for (gone, kept) in huehuecoyotl_library::versions::reconcile_releases(&tx, now)? {
                move_via(&tx, gone, kept)?;
            }
            tx.commit()?;
            Ok(())
        })
        .await;
    if let Err(e) = done {
        tracing::warn!("joining albums by release failed: {e}");
    }
}

/// Issue #146: join the artists that are the same (one MusicBrainz artist ID, or one name without
/// case and accents). Returns each artist joined away with the one it joined.
pub(crate) async fn reconcile_artists(inner: &Arc<Inner>) -> Vec<(i64, i64)> {
    let pairs = inner
        .lib
        .write(|c| huehuecoyotl_library::artists::reconcile(c))
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("joining artists failed: {e}");
            Vec::new()
        });
    if !pairs.is_empty() {
        for (gone, kept) in &pairs {
            tracing::info!(gone, kept, "artist joined");
            inner.sink.post(Event::ItemChanged { item_id: *kept });
        }
        inner.mark_changed();
    }
    pairs
}

/// The artist that stands for `artist` after joining.
pub(crate) fn kept(pairs: &[(i64, i64)], artist: i64) -> i64 {
    let mut a = artist;
    while let Some((_, k)) = pairs.iter().find(|(g, _)| *g == a) {
        a = *k;
    }
    a
}

pub(crate) async fn after_artists(inner: &Arc<Inner>, artists: &[i64]) {
    if artists.is_empty() {
        return;
    }
    let artists = artists.to_vec();
    let queued = inner
        .lib
        .write(move |c| {
            if !music::online(c)? {
                return Ok(false);
            }
            for a in &artists {
                for id in music::unmatched_albums_of(c, *a)? {
                    jobs::enqueue(
                        c,
                        "music_match",
                        &json!({ "item_id": id }).to_string(),
                        jobs::priority::NORMAL,
                        Some(&format!("music_match:{id}")),
                    )?;
                }
            }
            Ok(true)
        })
        .await;
    crate::extras::enqueue_artists(inner, &[]).await;
    if matches!(queued, Ok(true)) {
        inner.wake.notify_waiters();
    }
}

fn credits(r: &Recording) -> Vec<(String, String)> {
    r.artists
        .iter()
        .map(|a| (a.name.clone(), a.id.clone()))
        .collect()
}

/// Issue #103: the recording becomes the song's; issue #107: its unmatched track artists follow.
async fn apply_recording(
    inner: &Arc<Inner>,
    track: i64,
    r: &Recording,
    lock: bool,
) -> Result<(), MetaError> {
    let (id, isrcs, credits) = (r.id.clone(), r.isrcs.clone(), credits(r));
    let followed = inner
        .lib
        .write(move |c| {
            music::set_recording(c, track, &id, &isrcs, lock)?;
            // Only the song's own artists: the album artist is not changed by a song.
            let followed =
                music::follow_artists_of_track(c, track, &credits, &format!("track:{track}"))?;
            Ok(followed)
        })
        .await
        .map_err(db)?;
    after_artists(inner, &followed).await;
    inner.sink.post(Event::ItemChanged { item_id: track });
    Ok(())
}

fn artist_agrees(r: &Recording, artist: &str) -> bool {
    artist.trim().is_empty()
        || r.artists.iter().any(|a| norm(&a.name) == norm(artist))
        || norm(
            &r.artists
                .iter()
                .map(|a| format!("{}{}", a.name, a.join))
                .collect::<String>(),
        ) == norm(artist)
}

fn length_agrees(r: &Recording, length: Option<u64>) -> bool {
    match (r.length_ms, length) {
        (Some(a), Some(b)) => a.abs_diff(b) <= SAME_LENGTH_MS,
        _ => true,
    }
}

/// Issue #103: match a song again: within its album's release when the album is matched, else
/// by title, artist and length; never a guess.
pub(crate) async fn rematch_track(
    inner: &Arc<Inner>,
    track: i64,
) -> Result<(Outcome, i64), MetaError> {
    let t = inner
        .lib
        .read(move |c| {
            Ok(if music::online(c)? {
                music::track_target(c, track)?
            } else {
                None
            })
        })
        .await
        .map_err(db)?
        .ok_or(MetaError::Auth)?;
    let p = inner.music();
    let mut found: Option<String> = None;
    if let Some(rel) = &t.release {
        let r = p.release(rel).await?;
        let all: Vec<(u32, &huehuecoyotl_metadata::music::Track)> = r
            .media
            .iter()
            .flat_map(|m| m.tracks.iter().map(move |x| (m.position, x)))
            .collect();
        let same = |x: &huehuecoyotl_metadata::music::Track| norm(&x.title) == norm(&t.title);
        found = all
            .iter()
            .find(|(d, x)| *d == t.disc && x.position == t.position && same(x))
            .or_else(|| all.iter().find(|(_, x)| same(x)))
            .map(|(_, x)| x.recording_id.clone())
            .filter(|id| !id.is_empty());
    }
    if found.is_none() {
        let hits = p.search_recordings(&t.title, &t.artist).await?;
        let good: Vec<&Recording> = hits
            .iter()
            .filter(|r| {
                r.score >= 90
                    && norm(&r.title) == norm(&t.title)
                    && artist_agrees(r, &t.artist)
                    && length_agrees(r, t.length_ms)
            })
            .collect();
        match good.as_slice() {
            [] => return Ok((Outcome::NotFound, track)),
            [one] => found = Some(one.id.clone()),
            many => {
                // Several recordings fit: the one on this album, if exactly one is.
                let on_album: Vec<&&Recording> = many
                    .iter()
                    .filter(|r| {
                        r.releases
                            .iter()
                            .any(|(_, title, _)| norm(title) == norm(&t.album_title))
                    })
                    .collect();
                match on_album.as_slice() {
                    [one] => found = Some(one.id.clone()),
                    _ => return Ok((Outcome::Review, track)),
                }
            }
        }
    }
    let Some(id) = found else {
        return Ok((Outcome::NotFound, track));
    };
    if t.recording.as_deref() == Some(id.as_str()) {
        return Ok((Outcome::Unchanged, track));
    }
    let r = p.recording(&id).await?;
    apply_recording(inner, track, &r, false).await?;
    Ok((
        if t.recording.is_some() {
            Outcome::Changed
        } else {
            Outcome::Newly
        },
        track,
    ))
}

/// How many of the user's albums are this artist's at MusicBrainz.
async fn fit(inner: &Arc<Inner>, a: &Artist, albums: &[String]) -> Result<usize, MetaError> {
    if albums.is_empty() {
        return Ok(0);
    }
    let theirs = inner.music().artist_groups(&a.id).await?;
    Ok(music::albums_in_common(albums, &theirs))
}

/// At most this many artists of one name are checked against the user's albums.
const MAX_FIT_CHECKS: usize = 3;

/// Issue #105: match an artist again: by name; when several artists share it, the one that has
/// the user's albums, if exactly one does.
pub(crate) async fn rematch_artist(
    inner: &Arc<Inner>,
    artist: i64,
) -> Result<(Outcome, i64), MetaError> {
    let t = inner
        .lib
        .read(move |c| {
            Ok(if music::online(c)? {
                music::artist_match_target(c, artist)?
            } else {
                None
            })
        })
        .await
        .map_err(db)?
        .ok_or(MetaError::Auth)?;
    if norm(&t.name) == "variousartists" {
        return Ok((Outcome::Unchanged, artist));
    }
    let hits = inner.music().search_artists(&t.name).await?;
    let same: Vec<&Artist> = hits
        .iter()
        .filter(|a| norm(&a.name) == norm(&t.name))
        .collect();
    let chosen = match same.as_slice() {
        [] => return Ok((Outcome::NotFound, artist)),
        [one] => one.id.clone(),
        many => {
            let mut best: Vec<(&Artist, usize)> = Vec::new();
            for a in many.iter().take(MAX_FIT_CHECKS) {
                best.push((a, fit(inner, a, &t.albums).await?));
            }
            best.sort_by_key(|x| std::cmp::Reverse(x.1));
            match best.as_slice() {
                [(a, n), rest @ ..] if *n > 0 && rest.iter().all(|(_, m)| m < n) => a.id.clone(),
                _ => {
                    // The current match among them: it stays.
                    if t.mbid
                        .as_ref()
                        .is_some_and(|m| many.iter().any(|a| &a.id == m))
                    {
                        return Ok((Outcome::Unchanged, artist));
                    }
                    return Ok((Outcome::Review, artist));
                }
            }
        }
    };
    if t.mbid.as_deref() == Some(chosen.as_str()) {
        return Ok((Outcome::Unchanged, artist));
    }
    let was = t.mbid.is_some();
    inner
        .lib
        .write(move |c| music::set_artist_match(c, artist, &chosen, false))
        .await
        .map_err(db)?;
    after_artists(inner, &[artist]).await;
    let artist = kept(&reconcile_artists(inner).await, artist);
    enqueue_artist_names(inner).await;
    inner.sink.post(Event::ItemChanged { item_id: artist });
    Ok((
        if was {
            Outcome::Changed
        } else {
            Outcome::Newly
        },
        artist,
    ))
}

fn recording_candidate(
    r: &Recording,
    title: &str,
    artist: &str,
    length: Option<u64>,
    current: Option<&str>,
) -> Value {
    let same_artist = artist_agrees(r, artist);
    let mut compare = vec![json!({
        "label": format!("{title} → {}", r.title),
        "ok": norm(title) == norm(&r.title),
    })];
    let fit = match (length, r.length_ms) {
        (Some(a), Some(b)) => {
            compare.push(json!({ "label": format!("{} → {}", time(a), time(b)), "ok": a.abs_diff(b) <= SAME_LENGTH_MS }));
            if !same_artist {
                "otherArtist".to_string()
            } else if a.abs_diff(b) <= SAME_LENGTH_MS {
                format!("lengthAgrees:{} → {}", time(a), time(b))
            } else {
                format!("lengthDiffers:{}", time(a.abs_diff(b)))
            }
        }
        _ if !same_artist => "otherArtist".to_string(),
        _ => String::new(),
    };
    let artists: String = r
        .artists
        .iter()
        .map(|a| format!("{}{}", a.name, a.join))
        .collect();
    compare.push(json!({ "label": format!("{artist} → {artists}"), "ok": same_artist }));
    if let Some((_, album, _)) = r.releases.first() {
        compare.push(json!({ "label": album, "ok": true }));
    }
    let good = same_artist && length_agrees(r, length) && norm(title) == norm(&r.title);
    json!({
        "provider": "musicbrainz",
        "id": r.id,
        "kind": "recording",
        "title": r.title,
        "artist": artists,
        "lengthMs": r.length_ms,
        "release": r.releases.first().map(|x| x.1.clone()),
        "year": r.releases.first().and_then(|x| x.2.as_deref()).and_then(|d| d.get(..4)).and_then(|y| y.parse::<u16>().ok()),
        "fit": fit,
        "good": good,
        "compare": compare,
        "score": f64::from(r.score) / 100.0,
        "current": current == Some(r.id.as_str()),
    })
}

/// Issues #104 and #105: candidates for a song (recordings) or an artist.
pub(crate) async fn search(
    inner: &Arc<Inner>,
    item: i64,
    kind: &str,
    text: &str,
    q: &Value,
) -> CResult<Vec<Value>> {
    let online = inner.lib.read(music::online).await?;
    if !online {
        return Err(CoreError::new(
            ErrorKind::Unsupported,
            "Music and book information online is off.".to_string(),
        ));
    }
    let p = inner.music();
    let reference = ids::parse(text);
    if kind == "track" {
        let t = inner
            .lib
            .read(move |c| music::track_target(c, item))
            .await?
            .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("song {item}")))?;
        let hits = match reference {
            Some(Ref::MbRecording(id) | Ref::Mbid(id)) => {
                vec![p.recording(&id).await.map_err(crate::books::meta_err)?]
            }
            _ => {
                let title = if text.is_empty() {
                    t.title.clone()
                } else {
                    text.to_string()
                };
                let artist = q["artist"]
                    .as_str()
                    .map(str::trim)
                    .filter(|a| !a.is_empty())
                    .map_or_else(|| t.artist.clone(), str::to_string);
                p.search_recordings(&title, &artist)
                    .await
                    .map_err(crate::books::meta_err)?
            }
        };
        return Ok(hits
            .iter()
            .map(|r| {
                recording_candidate(r, &t.title, &t.artist, t.length_ms, t.recording.as_deref())
            })
            .collect());
    }
    let t = inner
        .lib
        .read(move |c| music::artist_match_target(c, item))
        .await?
        .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("artist {item}")))?;
    let hits = match reference {
        Some(Ref::MbArtist(id) | Ref::Mbid(id)) => {
            vec![p.artist(&id).await.map_err(crate::books::meta_err)?]
        }
        _ => {
            let name = if text.is_empty() {
                t.name.clone()
            } else {
                text.to_string()
            };
            p.search_artists(&name)
                .await
                .map_err(crate::books::meta_err)?
        }
    };
    let mut out = Vec::new();
    for (i, a) in hits.iter().enumerate() {
        // The user's albums are compared for the first few (one request each).
        let n = if i < MAX_FIT_CHECKS {
            Some(
                fit(inner, a, &t.albums)
                    .await
                    .map_err(crate::books::meta_err)?,
            )
        } else {
            None
        };
        out.push(json!({
            "provider": "musicbrainz",
            "id": a.id,
            "kind": "artist",
            "title": a.name,
            "artist": a.disambiguation,
            "type": a.kind,
            "country": a.country,
            "begin": a.begin,
            "end": a.end,
            "albums": n,
            "ofAlbums": t.albums.len(),
            "good": n.is_some_and(|n| n > 0),
            "score": f64::from(a.score) / 100.0,
            "current": t.mbid.as_deref() == Some(a.id.as_str()),
        }));
    }
    Ok(out)
}

/// Issues #104 and #105: the user's choice for a song or an artist; it is locked.
pub(crate) async fn choose(inner: &Arc<Inner>, item: i64, kind: &str, id: &str) -> CResult<i64> {
    let p = inner.music();
    if kind == "track" {
        let r = p.recording(id).await.map_err(crate::books::meta_err)?;
        apply_recording(inner, item, &r, true)
            .await
            .map_err(crate::books::meta_err)?;
    } else {
        let id = id.to_string();
        inner
            .lib
            .write(move |c| music::set_artist_match(c, item, &id, true))
            .await?;
        after_artists(inner, &[item]).await;
        let joined = kept(&reconcile_artists(inner).await, item);
        enqueue_artist_names(inner).await;
        inner.sink.post(Event::ItemChanged { item_id: joined });
        inner.mark_changed();
        return Ok(joined);
    }
    inner.mark_changed();
    Ok(item)
}

/// Issue #102: an album against a release, track by track (number, title and length; your file →
/// MusicBrainz), as a `matchPreview`.
pub(crate) async fn preview_album(inner: &Arc<Inner>, album: i64, release: &str) -> CResult<Value> {
    let r = inner
        .music()
        .release(release)
        .await
        .map_err(crate::books::meta_err)?;
    let local: Vec<(u32, u32, String, Option<u64>)> = inner
        .lib
        .read(move |c| {
            let mut st = c.prepare(
                "SELECT coalesce(t.season_no, 1), coalesce(t.episode_no, 0), coalesce(t.title, ''),
                        (SELECT max(fa.duration_ms) FROM file f JOIN file_audio fa ON fa.file_id = f.id WHERE f.item_id = t.id)
                 FROM item t WHERE t.parent_id = ?1 AND t.kind = 'track' ORDER BY 1, 2",
            )?;
            Ok(st
                .query_map([album], |x| {
                    Ok((
                        x.get::<_, i64>(0)? as u32,
                        x.get::<_, i64>(1)? as u32,
                        x.get(2)?,
                        x.get::<_, Option<i64>>(3)?.filter(|l| *l > 0).map(|l| l as u64),
                    ))
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await?;
    let remote: Vec<(u32, u32, String, Option<u64>)> = r
        .media
        .iter()
        .flat_map(|m| {
            m.tracks
                .iter()
                .map(move |t| (m.position, t.position, t.title.clone(), t.length_ms))
        })
        .collect();
    let mut rows = Vec::new();
    let mut agree = 0;
    for (d, p, title, len) in &local {
        let other = remote
            .iter()
            .find(|(rd, rp, _, _)| rd == d && rp == p)
            .or_else(|| remote.iter().find(|(_, _, t, _)| norm(t) == norm(title)));
        let ok = other.is_some_and(|(_, _, t, l)| {
            norm(t) == norm(title)
                && match (len, l) {
                    (Some(a), Some(b)) => a.abs_diff(*b) <= SAME_LENGTH_MS,
                    _ => true,
                }
        });
        if ok {
            agree += 1;
        }
        rows.push(json!({
            "position": p,
            "disc": d,
            "title": title,
            "length": len.map(time),
            "theirs": other.map(|o| o.2.clone()),
            "theirLength": other.and_then(|o| o.3).map(time),
            "ok": ok,
        }));
    }
    let extra = remote
        .iter()
        .filter(|(rd, rp, t, _)| {
            !local
                .iter()
                .any(|(d, p, title, _)| (d == rd && p == rp) || norm(title) == norm(t))
        })
        .count();
    Ok(json!({
        "itemId": album,
        "change": "none",
        "tracks": rows,
        "agree": agree,
        "total": local.len(),
        "extra": extra,
    }))
}

// ---- the name shown (issue #148) ---------------------------------------------------------------

/// Queue the MusicBrainz artist lookups that bring matched artists' spellings (one request a
/// second with the music limiter, like every MusicBrainz call).
pub(crate) async fn enqueue_artist_names(inner: &Arc<Inner>) {
    let queued = inner
        .lib
        .write(|c| {
            if !music::online(c)? {
                return Ok(0);
            }
            let due = huehuecoyotl_library::artists::names_due(c)?;
            for id in &due {
                jobs::enqueue(
                    c,
                    "artist_name",
                    &json!({ "item_id": id }).to_string(),
                    jobs::priority::BACKGROUND,
                    Some(&format!("artist_name:{id}")),
                )?;
            }
            Ok(due.len())
        })
        .await
        .unwrap_or(0);
    if queued > 0 {
        inner.wake.notify_waiters();
    }
}

/// One matched artist's spelling at MusicBrainz; the name shown follows it unless the user chose
/// another.
pub(crate) async fn fetch_artist_name(inner: &Arc<Inner>, artist: i64) -> Result<(), MetaError> {
    let Some(mbid) = inner
        .lib
        .read(move |c| {
            Ok(if music::online(c)? {
                huehuecoyotl_library::matching::external(c, artist, "musicbrainz", "artist")?
            } else {
                None
            })
        })
        .await
        .map_err(db)?
    else {
        return Ok(());
    };
    let found = inner.music().artist(&mbid).await?;
    if found.name.trim().is_empty() {
        return Ok(());
    }
    let name = found.name;
    let changed = inner
        .lib
        .write(move |c| huehuecoyotl_library::artists::set_mb_title(c, artist, &name))
        .await
        .map_err(db)?;
    if changed {
        inner.sink.post(Event::ItemChanged { item_id: artist });
        inner.mark_changed();
    }
    Ok(())
}

impl crate::Core {
    /// Issue #148: how an artist is named: `choice` "auto" (MusicBrainz's spelling when matched),
    /// "files" or "own" (with `own`). Answers with the artist's page (`MusicDetail` "artist").
    pub fn set_artist_name(&self, request_id: u64, artist: i64, choice: &str, own: &str) {
        let (choice, own) = (choice.to_string(), own.to_string());
        self.spawn(request_id, move |inner| async move {
            let c = huehuecoyotl_library::artists::NameChoice::parse(&choice);
            if c == huehuecoyotl_library::artists::NameChoice::Own && own.trim().is_empty() {
                return Err(CoreError::new(
                    ErrorKind::Unsupported,
                    "Please enter a name.",
                ));
            }
            inner
                .lib
                .write(move |db| {
                    huehuecoyotl_library::artists::set_name_choice(db, artist, c, &own)
                })
                .await?;
            inner.mark_changed();
            let d = inner
                .lib
                .read(move |db| music::artist_detail(db, artist))
                .await?
                .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("artist {artist}")))?;
            Ok(Event::MusicDetail {
                request_id,
                kind: "artist".into(),
                json: serde_json::to_string(&d).unwrap_or_default(),
            })
        });
    }
}

// ---- Merge with… (issue #149) ------------------------------------------------------------------

/// How long a merge by hand can be undone.
const UNDO_FOR: std::time::Duration = std::time::Duration::from_secs(30);

impl crate::Core {
    /// Issue #149: the artists to merge `artist` with (`MusicDetail` "mergeCandidates":
    /// `likely`, `others`, each `[{id, name, albums, songs, matched, why, blocked}]`).
    pub fn artist_merge_candidates(&self, request_id: u64, artist: i64, query: &str) {
        let query = query.to_string();
        self.spawn(request_id, move |inner| async move {
            let (likely, others) = inner
                .lib
                .read(move |c| huehuecoyotl_library::artists::candidates(c, artist, &query))
                .await?;
            Ok(Event::MusicDetail {
                request_id,
                kind: "mergeCandidates".into(),
                json: json!({ "likely": likely, "others": others }).to_string(),
            })
        });
    }

    /// Issue #149: what merging `drop` into `keep` would hold (`MusicDetail` "mergePreview").
    pub fn artist_merge_preview(&self, request_id: u64, keep: i64, drop: i64) {
        self.spawn(request_id, move |inner| async move {
            let p = inner
                .lib
                .read(move |c| huehuecoyotl_library::artists::preview(c, keep, drop))
                .await?;
            Ok(Event::MusicDetail {
                request_id,
                kind: "mergePreview".into(),
                json: serde_json::to_string(&p).unwrap_or_default(),
            })
        });
    }

    /// Issue #149: merge `drop` into `keep` by hand (`MusicDetail` "artistMerged": `keep`,
    /// `drop`, `keepName`, `dropName`); [`Core::artist_merge_undo`] reverts it for 30 seconds.
    pub fn artist_merge(&self, request_id: u64, keep: i64, drop: i64) {
        self.spawn(request_id, move |inner| async move {
            let drop_name: String = inner
                .lib
                .read(move |c| {
                    Ok(c.query_row(
                        "SELECT coalesce(title, '') FROM item WHERE id = ?1",
                        [drop],
                        |r| r.get(0),
                    )?)
                })
                .await?;
            if let Some(why) = inner
                .lib
                .read(move |c| huehuecoyotl_library::artists::merge_refusal(c, keep, drop))
                .await?
            {
                return Err(CoreError::new(
                    ErrorKind::Unsupported,
                    if why.contains("MusicBrainz") {
                        "These are two different artists at MusicBrainz, so they cannot be merged."
                            .to_string()
                    } else {
                        format!("These cannot be merged: {why}.")
                    },
                ));
            }
            let undo = inner
                .lib
                .write(move |c| huehuecoyotl_library::artists::merge_by_hand(c, keep, drop))
                .await?;
            *inner.merge_undo.lock().unwrap() = Some((undo, std::time::Instant::now()));
            // The kept artist's name after the merge (#148 may choose another spelling).
            let keep_name: String = inner
                .lib
                .read(move |c| {
                    Ok(c.query_row(
                        "SELECT coalesce(title, '') FROM item WHERE id = ?1",
                        [keep],
                        |r| r.get(0),
                    )?)
                })
                .await?;
            inner.sink.post(Event::ItemChanged { item_id: keep });
            inner.mark_changed();
            Ok(Event::MusicDetail {
                request_id,
                kind: "artistMerged".into(),
                json:
                    json!({ "keep": keep, "drop": drop, "keepName": keep_name, "dropName": drop_name })
                        .to_string(),
            })
        });
    }

    /// Issue #149: undo the last merge by hand within 30 seconds (`MusicDetail`
    /// "artistMergeUndone": `keep`, `drop`).
    pub fn artist_merge_undo(&self, request_id: u64) {
        self.spawn(request_id, move |inner| async move {
            let last = inner.merge_undo.lock().unwrap().take();
            let Some((undo, _)) = last.filter(|(_, at)| at.elapsed() < UNDO_FOR) else {
                return Err(CoreError::new(
                    ErrorKind::NotFound,
                    "There is no merge to undo.",
                ));
            };
            let (keep, drop) = (undo.keep, undo.drop);
            inner
                .lib
                .write(move |c| huehuecoyotl_library::artists::undo(c, &undo))
                .await?;
            inner.sink.post(Event::ItemChanged { item_id: keep });
            inner.mark_changed();
            Ok(Event::MusicDetail {
                request_id,
                kind: "artistMergeUndone".into(),
                json: json!({ "keep": keep, "drop": drop }).to_string(),
            })
        });
    }
}
