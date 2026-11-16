//! Music information online (issue #11, SPEC 7 "Providers"): albums are looked up at MusicBrainz
//! (one request for an album tagged with its release id, a search and a lookup otherwise), and an
//! album without a cover gets the Cover Art Archive's. With music information limited to file
//! tags, nothing is sent.

use std::sync::Arc;

use huehuecoyotl_library::{jobs, music};
use huehuecoyotl_metadata::MetaError;
use huehuecoyotl_metadata::music::{Decision, MusicProvider, Release, decide};
use huehuecoyotl_scanner::music::Cover;
use serde_json::json;

use crate::{Core, Event, Inner};

impl Inner {
    pub(crate) fn music(&self) -> Arc<dyn MusicProvider> {
        self.music.read().unwrap().clone()
    }
}

/// Queue lookups for these albums and every album due for one, unless music information is
/// limited to file tags.
pub(crate) async fn enqueue_lookups(inner: &Arc<Inner>, albums: &[i64]) {
    let albums = albums.to_vec();
    let queued = inner
        .lib
        .write(move |c| {
            if !music::online(c)? {
                return Ok(false);
            }
            let mut ids = albums;
            for id in music::due(c)? {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            for id in ids {
                jobs::enqueue(
                    c,
                    "music_match",
                    &json!({ "item_id": id }).to_string(),
                    jobs::priority::NORMAL,
                    Some(&format!("music_match:{id}")),
                )?;
            }
            Ok(true)
        })
        .await;
    if matches!(queued, Ok(true)) {
        inner.wake.notify_waiters();
    }
}

fn facts(r: &Release) -> music::ReleaseFacts {
    music::ReleaseFacts {
        id: r.id.clone(),
        group_id: r.group_id.clone(),
        release_type: r.release_type.clone(),
        compilation: r.compilation,
        date: r.date.clone(),
        artists: r
            .artists
            .iter()
            .map(|a| (a.name.clone(), a.id.clone()))
            .collect(),
        tracks: r
            .media
            .iter()
            .flat_map(|m| {
                m.tracks.iter().map(move |t| {
                    (
                        m.position,
                        t.position,
                        t.title.clone(),
                        t.recording_id.clone(),
                    )
                })
            })
            .collect(),
    }
}

fn db(e: huehuecoyotl_library::DbError) -> MetaError {
    MetaError::Other(e.to_string())
}

/// Look one album up, and fetch its cover when it has none.
pub(crate) async fn match_album(inner: &Arc<Inner>, album: i64) -> Result<(), MetaError> {
    let Some(t) = inner
        .lib
        .read(move |c| {
            Ok(if music::online(c)? {
                music::album_target(c, album)?
            } else {
                None
            })
        })
        .await
        .map_err(db)?
    else {
        return Ok(()); // offline by setting, or the album is gone
    };
    let p = inner.music();
    let mut release: Option<Release> = None;
    if !t.locked && t.match_state != "matched" {
        let id = match &t.release_id {
            // Tagged with its release id: one request.
            Some(id) => Some(id.clone()),
            None => {
                let hits = p
                    .search_releases(&t.artist, &t.title, (t.tracks > 0).then_some(t.tracks))
                    .await?;
                match decide(
                    &hits,
                    &t.artist,
                    &t.title,
                    (t.tracks > 0).then_some(t.tracks),
                ) {
                    Decision::Matched(id) => Some(id),
                    Decision::Review(hits) => {
                        let cands: Vec<music::Candidate> = hits
                            .iter()
                            .map(|h| music::Candidate {
                                id: h.id.clone(),
                                title: h.title.clone(),
                                artist: h.artist.clone(),
                                year: h
                                    .date
                                    .as_deref()
                                    .and_then(|d| d.get(..4))
                                    .and_then(|y| y.parse().ok()),
                                score: f64::from(h.score) / 100.0,
                                tracks: h.track_count,
                            })
                            .collect();
                        inner
                            .lib
                            .write(move |c| music::set_review(c, album, &cands))
                            .await
                            .map_err(db)?;
                        None
                    }
                    Decision::None => {
                        inner
                            .lib
                            .write(move |c| music::set_unmatched(c, album))
                            .await
                            .map_err(db)?;
                        None
                    }
                }
            }
        };
        if let Some(id) = id {
            let r = p.release(&id).await?;
            let f = facts(&r);
            inner
                .lib
                .write(move |c| music::apply_release(c, album, &f))
                .await
                .map_err(db)?;
            release = Some(r);
        }
    }
    if !t.has_cover {
        let (rel, group) = match &release {
            Some(r) => (Some(r.id.clone()), r.group_id.clone()),
            None => (t.release_id.clone(), t.group_id.clone()),
        };
        if let Some(rel) = rel
            && let Some(img) = p.front_cover(&rel, group.as_deref()).await?
        {
            crate::worker::write_cover(inner, &inner.local, album, Cover::Embedded(img.to_vec()))
                .await;
        }
    }
    inner.sink.post(Event::ItemChanged { item_id: album });
    Ok(())
}

impl Core {
    /// Use this music provider instead of MusicBrainz (tests).
    pub fn set_music_provider(&self, p: Arc<dyn MusicProvider>) {
        *self.inner.music.write().unwrap() = p;
    }

    /// Issue #10: an album's page: its tracks by disc, durations, format and Hi-Res.
    pub fn album_detail(&self, request_id: u64, album: i64) {
        self.spawn(request_id, move |inner| async move {
            let d = inner
                .lib
                .read(move |c| music::album_detail(c, album))
                .await?
                .ok_or_else(|| {
                    crate::CoreError::new(crate::ErrorKind::NotFound, format!("album {album}"))
                })?;
            drop(inner.lib.submit(move |c| jobs::bump_items(c, &[album])));
            Ok(Event::MusicDetail {
                request_id,
                kind: "album".into(),
                json: serde_json::to_string(&d).unwrap_or_default(),
            })
        });
    }

    /// Issue #10: an artist's page: albums by release type and year, and "appears on".
    pub fn artist_detail(&self, request_id: u64, artist: i64) {
        self.spawn(request_id, move |inner| async move {
            let d = inner
                .lib
                .read(move |c| music::artist_detail(c, artist))
                .await?
                .ok_or_else(|| {
                    crate::CoreError::new(crate::ErrorKind::NotFound, format!("artist {artist}"))
                })?;
            Ok(Event::MusicDetail {
                request_id,
                kind: "artist".into(),
                json: serde_json::to_string(&d).unwrap_or_default(),
            })
        });
    }

    /// Whether music information is looked up online.
    pub fn music_online(&self, request_id: u64) {
        self.spawn(request_id, move |inner| async move {
            let online = inner.lib.read(music::online).await?;
            Ok(Event::MusicOnline { request_id, online })
        });
    }

    /// Turn online music information on or off (Settings → Metadata). Turning it on queues the
    /// albums that were never looked up.
    pub fn set_music_online(&self, request_id: u64, on: bool) {
        self.spawn(request_id, move |inner| async move {
            inner.lib.write(move |c| music::set_online(c, on)).await?;
            if on {
                enqueue_lookups(&inner, &[]).await;
            }
            Ok(Event::MusicOnline {
                request_id,
                online: on,
            })
        });
    }
}
