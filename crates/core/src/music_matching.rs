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

/// Issues #105 and #106: matched artists are looked up at the optional providers, and their
/// albums without a match are looked up again.
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
        inner.sink.post(Event::ItemChanged { item_id: item });
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
