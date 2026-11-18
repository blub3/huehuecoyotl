//! Playlists (2.6, issue #81): the Playlists screen's lists, an own playlist's changes, playing
//! one and saving its songs. Answers come as `MusicDetail` events: "playlists" (every playlist)
//! and "playlist" (one, with its songs).

use std::sync::Arc;

use huehuecoyotl_library::{playlists, queue};
use serde_json::json;

use crate::playqueue::Op;
use crate::{CResult, Core, CoreError, ErrorKind, Event, Inner};

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// How many songs a playlist answer carries (a page shows them all; the preview a few).
const MAX_ENTRIES: usize = 5000;

async fn list_event(inner: &Arc<Inner>, request_id: u64) -> CResult<Event> {
    let (smart, mine) = inner
        .lib
        .read(move |c| playlists::list(c, now_ms()))
        .await?;
    Ok(Event::MusicDetail {
        request_id,
        kind: "playlists".into(),
        json: json!({ "smart": smart, "mine": mine }).to_string(),
    })
}

async fn detail_event(
    inner: &Arc<Inner>,
    request_id: u64,
    key: String,
    limit: usize,
) -> CResult<Event> {
    let k = key.clone();
    let (smart, mine, entries) = inner
        .lib
        .read(move |c| {
            let (s, m) = playlists::list(c, now_ms())?;
            let e = playlists::entries(c, &k, now_ms(), limit)?;
            Ok((s, m, e))
        })
        .await?;
    let card = smart
        .into_iter()
        .chain(mine)
        .find(|c| c.key == key)
        .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("playlist {key}")))?;
    Ok(Event::MusicDetail {
        request_id,
        kind: "playlist".into(),
        json: json!({ "card": card, "entries": entries }).to_string(),
    })
}

/// Albums, artists or songs as the songs they stand for, each with its file (`version` picks
/// an album version; 0: the one played last, else the preferred one).
async fn expand(inner: &Arc<Inner>, items: Vec<i64>, version: i64) -> CResult<Vec<queue::Track>> {
    let offline = inner.offline.lock().unwrap().clone();
    Ok(inner
        .lib
        .read(move |c| {
            queue::expand(
                c,
                &items,
                (version > 0).then_some(version),
                "quality",
                &offline,
            )
        })
        .await?)
}

impl Core {
    /// Every playlist (MusicDetail "playlists": `smart` and `mine`, cards with key, id, name,
    /// smart key, count, durationMs and covers).
    pub fn playlists(&self, request_id: u64) {
        self.spawn(request_id, move |inner| async move {
            list_event(&inner, request_id).await
        });
    }

    /// One playlist with its songs ("smart:<key>" or an id; MusicDetail "playlist").
    pub fn playlist_detail(&self, request_id: u64, key: &str, limit: u32) {
        let key = key.to_string();
        self.spawn(request_id, move |inner| async move {
            detail_event(
                &inner,
                request_id,
                key,
                (limit as usize).clamp(1, MAX_ENTRIES),
            )
            .await
        });
    }

    /// A new playlist, with these items' songs when given (answers with it).
    pub fn playlist_create(&self, request_id: u64, name: &str, items: Vec<i64>, version: i64) {
        let name = name.to_string();
        self.spawn(request_id, move |inner| async move {
            let tracks = if items.is_empty() {
                vec![]
            } else {
                expand(&inner, items, version).await?
            };
            let id = inner
                .lib
                .write(move |c| {
                    let id = playlists::create(c, &name, now_ms())?;
                    playlists::add(c, id, &tracks, now_ms())?;
                    Ok(id)
                })
                .await?;
            detail_event(&inner, request_id, id.to_string(), MAX_ENTRIES).await
        });
    }

    /// Add albums, artists or songs to an own playlist (answers with it).
    pub fn playlist_add(&self, request_id: u64, id: i64, items: Vec<i64>, version: i64) {
        self.spawn(request_id, move |inner| async move {
            let tracks = expand(&inner, items, version).await?;
            if tracks.is_empty() {
                return Err(CoreError::new(ErrorKind::NotFound, "Nothing to add."));
            }
            inner
                .lib
                .write(move |c| playlists::add(c, id, &tracks, now_ms()))
                .await?;
            detail_event(&inner, request_id, id.to_string(), MAX_ENTRIES).await
        });
    }

    pub fn playlist_rename(&self, request_id: u64, id: i64, name: &str) {
        let name = name.to_string();
        self.spawn(request_id, move |inner| async move {
            inner
                .lib
                .write(move |c| playlists::rename(c, id, &name, now_ms()))
                .await?;
            detail_event(&inner, request_id, id.to_string(), MAX_ENTRIES).await
        });
    }

    /// Delete an own playlist (answers with every playlist).
    pub fn playlist_delete(&self, request_id: u64, id: i64) {
        self.spawn(request_id, move |inner| async move {
            inner.lib.write(move |c| playlists::delete(c, id)).await?;
            list_event(&inner, request_id).await
        });
    }

    /// Remove the song at `index`, or move it to `to` (`to` < 0: remove).
    pub fn playlist_move(&self, request_id: u64, id: i64, index: u32, to: i32) {
        self.spawn(request_id, move |inner| async move {
            inner
                .lib
                .write(move |c| {
                    if to < 0 {
                        playlists::remove(c, id, index as usize, now_ms())
                    } else {
                        playlists::move_entry(c, id, index as usize, to as usize, now_ms())
                    }
                })
                .await?;
            detail_event(&inner, request_id, id.to_string(), MAX_ENTRIES).await
        });
    }

    /// Play a playlist from song `start` (an index; with shuffle and 0, a random one).
    pub fn queue_play_playlist(&self, request_id: u64, key: &str, start: u32, shuffle: bool) {
        self.queue_op(
            request_id,
            Op::PlayPlaylist {
                key: key.to_string(),
                start: start as usize,
                shuffle,
            },
        );
    }

    /// Save a playlist's songs on this computer (each with its file; MusicDetail
    /// "playlistSaved" with how many copies were started).
    pub fn playlist_save(&self, request_id: u64, key: &str) {
        let key = key.to_string();
        self.spawn(request_id, move |inner| async move {
            let tracks = inner
                .lib
                .read(move |c| playlists::resolve(c, &key, now_ms()))
                .await?;
            let mut started = 0usize;
            for (item, file) in tracks {
                match crate::download::save(&inner, item, file.unwrap_or(0)).await {
                    Ok(made) => started += made.len(),
                    // Songs on this computer need no copy.
                    Err(e) if e.kind == ErrorKind::Unsupported => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(Event::MusicDetail {
                request_id,
                kind: "playlistSaved".into(),
                json: json!({ "started": started }).to_string(),
            })
        });
    }
}
