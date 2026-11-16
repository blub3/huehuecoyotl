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
pub(crate) async fn match_album(inner: &Arc<Inner>, mut album: i64) -> Result<(), MetaError> {
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
            // Issue #63: other copies of this release or editions of its group join this album
            // (the oldest of them remains).
            album = inner
                .lib
                .write(move |c| {
                    music::apply_release(c, album, &f)?;
                    huehuecoyotl_library::versions::merge_same_release(c, album)
                })
                .await
                .map_err(db)?;
            release = Some(r);
            // Issue #12: the album's artists now have their MusicBrainz ids.
            crate::extras::enqueue_artists(inner, &[]).await;
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

    /// Issue #10: an album's page: its tracks by disc, durations, format and Hi-Res. Issue #63:
    /// for a version (`version` 0: the one played last, else by `preference`: quality, smallest
    /// or lossless), with every version and whether it can be played.
    pub fn album_detail(&self, request_id: u64, album: i64, version: i64, preference: &str) {
        let preference = preference.to_string();
        self.spawn(request_id, move |inner| async move {
            let offline = inner.offline.lock().unwrap().clone();
            let d = inner
                .lib
                .read(move |c| {
                    music::album_detail(
                        c,
                        album,
                        (version > 0).then_some(version),
                        &preference,
                        &offline,
                    )
                })
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

    /// Issue #63: "This is a different album": the version becomes an album of its own, which
    /// scans and lookups leave alone. Answers with the new album's page.
    pub fn split_album_version(&self, request_id: u64, version: i64) {
        self.spawn(request_id, move |inner| async move {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as i64);
            let album = inner
                .lib
                .write(move |c| huehuecoyotl_library::versions::split_version(c, version, now))
                .await?;
            inner.mark_changed();
            let offline = inner.offline.lock().unwrap().clone();
            let d = inner
                .lib
                .read(move |c| music::album_detail(c, album, None, "quality", &offline))
                .await?
                .ok_or_else(|| {
                    crate::CoreError::new(crate::ErrorKind::NotFound, format!("album {album}"))
                })?;
            Ok(Event::MusicDetail {
                request_id,
                kind: "album".into(),
                json: serde_json::to_string(&d).unwrap_or_default(),
            })
        });
    }

    /// Issue #63: play this file of a track (one of its files in a version), from now on.
    pub fn set_track_file(&self, track: i64, file: i64) {
        let inner = self.inner.clone();
        let fut = self.inner.lib.submit(move |c| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as i64);
            huehuecoyotl_library::versions::remember_played(c, track, file, now)
        });
        self.rt.spawn(async move {
            match fut.await {
                Ok(()) => inner.sink.post(Event::ItemChanged { item_id: track }),
                Err(e) => tracing::error!(track, "choosing the file failed: {e}"),
            }
        });
    }

    /// Issue #65: a book's page (answers with `MusicDetail`, kind "book": the book with its
    /// people, series, chapters, files and progress), a series' or a person's books ("series",
    /// "person").
    pub fn book_detail(&self, request_id: u64, id: i64) {
        self.spawn(request_id, move |inner| async move {
            let (kind, json) = inner
                .lib
                .read(move |c| {
                    let kind: Option<String> = c
                        .query_row("SELECT kind FROM item WHERE id = ?1", [id], |r| r.get(0))
                        .map(Some)
                        .or_else(|e| match e {
                            huehuecoyotl_library::rusqlite::Error::QueryReturnedNoRows => Ok(None),
                            e => Err(e),
                        })?;
                    use huehuecoyotl_library::books;
                    Ok(match kind.as_deref() {
                        Some("audiobook") => (
                            "book",
                            serde_json::to_string(&books::detail(c, id)?).unwrap_or_default(),
                        ),
                        Some(k @ ("book_series" | "person")) => {
                            let title: String = c.query_row(
                                "SELECT coalesce(title, '') FROM item WHERE id = ?1",
                                [id],
                                |r| r.get(0),
                            )?;
                            let list = if k == "person" {
                                books::person_books(c, id)?
                            } else {
                                books::series_books(c, id)?
                            };
                            (
                                if k == "person" { "person" } else { "series" },
                                serde_json::json!({ "id": id, "title": title, "books": list })
                                    .to_string(),
                            )
                        }
                        _ => ("none", "null".to_string()),
                    })
                })
                .await?;
            if kind == "none" {
                return Err(crate::CoreError::new(
                    crate::ErrorKind::NotFound,
                    format!("book {id}"),
                ));
            }
            Ok(Event::MusicDetail {
                request_id,
                kind: kind.into(),
                json,
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
                crate::books::enqueue_lookups(&inner, &[]).await;
            }
            Ok(Event::MusicOnline {
                request_id,
                online: on,
            })
        });
    }
}
