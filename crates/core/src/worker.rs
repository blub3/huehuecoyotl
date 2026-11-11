//! Job workers (SPEC 7): one for scans, one for metadata. Jobs come from the persisted queue;
//! failures back off, rate limits wait for the server's Retry-After, and metadata jobs wait
//! while no API key is configured.

use std::sync::Arc;
use std::time::Duration;

use huehuecoyotl_library::{catalog, jobs, queries};
use huehuecoyotl_metadata::matching::{Decision, decide};
use huehuecoyotl_metadata::{MediaType, MetaError, MetadataProvider, artwork};
use huehuecoyotl_scanner::scan::{ScanError, scan_source};
use huehuecoyotl_vfs::VPath;
use serde_json::json;

use crate::{Event, Inner};

pub const SCAN_KINDS: &[&str] = &["scan"];
pub const META_KINDS: &[&str] = &["match", "details", "artwork"];

enum Outcome {
    Done,
    Retry(String, Option<i64>),
    /// Not now, and it does not count as an attempt.
    Defer(i64),
}

pub fn spawn_workers(inner: &Arc<Inner>) {
    for (kinds, needs_provider) in [(SCAN_KINDS, false), (META_KINDS, true)] {
        let inner = inner.clone();
        tokio::spawn(async move {
            loop {
                let ready = !needs_provider || inner.provider().is_some();
                let job = if ready {
                    inner
                        .lib
                        .write(move |c| jobs::claim(c, kinds))
                        .await
                        .unwrap_or_else(|e| {
                            tracing::error!("claiming a job failed: {e}");
                            None
                        })
                } else {
                    None
                };
                let Some(job) = job else {
                    // Sleep until new work is queued, a key arrives, or a backoff expires.
                    let _ =
                        tokio::time::timeout(Duration::from_secs(20), inner.wake.notified()).await;
                    continue;
                };
                let id = job.id;
                let outcome = run(&inner, &job.kind, &job.payload).await;
                let res = match outcome {
                    Outcome::Done => inner.lib.write(move |c| jobs::complete(c, id)).await,
                    Outcome::Retry(err, after) => {
                        tracing::warn!(kind = %job.kind, attempt = job.attempts, "job failed: {err}");
                        inner
                            .lib
                            .write(move |c| jobs::fail(c, id, &err, after))
                            .await
                    }
                    Outcome::Defer(s) => inner.lib.write(move |c| jobs::defer(c, id, s)).await,
                };
                if let Err(e) = res {
                    tracing::error!("updating job {id} failed: {e}");
                }
                inner.mark_changed();
            }
        });
    }
}

fn item_of(payload: &str) -> i64 {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|v| v["item_id"].as_i64())
        .unwrap_or(0)
}

async fn run(inner: &Arc<Inner>, kind: &str, payload: &str) -> Outcome {
    let v: serde_json::Value = serde_json::from_str(payload).unwrap_or_default();
    let res = match kind {
        "scan" => scan(inner, v["source_id"].as_i64().unwrap_or(0)).await,
        "match" | "details" | "artwork" => {
            let Some(p) = inner.provider() else {
                return Outcome::Defer(300);
            };
            let item = item_of(payload);
            let r = match kind {
                "match" => match_item(inner, &*p, item).await,
                "details" => details(inner, &*p, item, v["tmdb_id"].as_i64().unwrap_or(0)).await,
                _ => fetch_artwork(inner, &*p, item).await,
            };
            match r {
                Ok(()) => Ok(()),
                Err(MetaError::RateLimited(s)) => {
                    return Outcome::Retry("rate limited".into(), Some(s as i64));
                }
                Err(MetaError::Auth) => {
                    inner.key_rejected();
                    return Outcome::Defer(600);
                }
                Err(MetaError::NotFound) => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        }
        other => Err(format!("unknown job kind {other}")),
    };
    match res {
        Ok(()) => Outcome::Done,
        Err(e) => Outcome::Retry(e, None),
    }
}

async fn scan(inner: &Arc<Inner>, source_id: i64) -> Result<(), String> {
    let root: Option<String> = inner
        .lib
        .read(move |c| {
            use huehuecoyotl_library::rusqlite::OptionalExtension;
            Ok(
                c.query_row("SELECT root FROM source WHERE id = ?1", [source_id], |r| {
                    r.get(0)
                })
                .optional()?
                .flatten(),
            )
        })
        .await
        .map_err(|e| e.to_string())?;
    let Some(root) = root else { return Ok(()) }; // library was removed meanwhile
    inner.scanning.lock().unwrap().insert(source_id);
    inner.sink.post(Event::ScanState {
        source_id,
        running: true,
        message: String::new(),
    });
    let res = scan_source(&inner.lib, &inner.local, source_id, &VPath::new(&root)).await;
    inner.scanning.lock().unwrap().remove(&source_id);
    let (message, out) = match res {
        Ok(r) => {
            let new = r.new_items.clone();
            let queued = inner
                .lib
                .write(move |c| {
                    // New items, plus older ones the matcher may now get right.
                    let mut ids = new;
                    for id in catalog::due_for_matching(c)? {
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                    for id in ids {
                        jobs::enqueue(
                            c,
                            "match",
                            &json!({ "item_id": id }).to_string(),
                            jobs::priority::NORMAL,
                            Some(&format!("match:{id}")),
                        )?;
                    }
                    Ok(())
                })
                .await;
            inner.wake.notify_waiters();
            (
                format!(
                    "{} files, {} new, {} removed",
                    r.files_seen, r.added, r.removed
                ),
                queued.map_err(|e| e.to_string()),
            )
        }
        // An unplugged drive is not a failure to retry quickly; report it and keep the data.
        Err(ScanError::LooksUnreachable | ScanError::Vfs(_)) => {
            ("The folder cannot be read right now.".to_string(), Ok(()))
        }
        Err(e) => (e.to_string(), Err(e.to_string())),
    };
    inner.sink.post(Event::ScanState {
        source_id,
        running: false,
        message,
    });
    out
}

fn media_type(kind: &str) -> Option<MediaType> {
    match kind {
        "movie" => Some(MediaType::Movie),
        "show" => Some(MediaType::Show),
        _ => None,
    }
}

async fn match_item(
    inner: &Arc<Inner>,
    p: &dyn MetadataProvider,
    item: i64,
) -> Result<(), MetaError> {
    let target = inner
        .lib
        .read(move |c| catalog::match_target(c, item))
        .await
        .map_err(|e| MetaError::Other(e.to_string()))?;
    let Some(t) = target else { return Ok(()) };
    let Some(ty) = media_type(&t.kind) else {
        return Ok(());
    };
    if t.locked {
        if let Some(id) = t.tmdb_id {
            enqueue_details(inner, item, id, jobs::priority::NORMAL).await;
        }
        return Ok(());
    }
    let mut cands = p.search(ty, &t.title, t.year).await?;
    let (decision, scores) = decide(&mut cands, &t.title, t.year);
    tracing::info!(item, title = %t.title, ?decision, "matched");
    let rows: Vec<catalog::CandidateRow> = cands
        .iter()
        .zip(scores)
        .take(8)
        .map(|(c, s)| catalog::CandidateRow {
            provider_id: c.id,
            title: c.title.clone(),
            year: c.year,
            score: s,
            poster_path: c.poster_path.clone(),
            overview: c.overview.clone(),
        })
        .collect();
    let best = cands.first().map(|c| c.id);
    let state = match decision {
        Decision::Matched => "matched",
        Decision::NeedsReview => "needs_review",
        Decision::Unmatched => "unmatched",
    };
    let keep = if decision == Decision::NeedsReview {
        rows
    } else {
        vec![]
    };
    inner
        .lib
        .write(move |c| catalog::set_match_state(c, item, state, &keep))
        .await
        .map_err(|e| MetaError::Other(e.to_string()))?;
    if let (Decision::Matched, Some(id)) = (decision, best) {
        enqueue_details(inner, item, id, jobs::priority::NORMAL).await;
    }
    inner.sink.post(Event::ItemChanged { item_id: item });
    Ok(())
}

pub(crate) async fn enqueue_details(inner: &Arc<Inner>, item: i64, tmdb_id: i64, prio: i64) {
    let payload = json!({ "item_id": item, "tmdb_id": tmdb_id }).to_string();
    let _ = inner
        .lib
        .write(move |c| {
            jobs::enqueue(
                c,
                "details",
                &payload,
                prio,
                Some(&format!("details:{item}")),
            )
        })
        .await;
    inner.wake.notify_waiters();
}

async fn details(
    inner: &Arc<Inner>,
    p: &dyn MetadataProvider,
    item: i64,
    tmdb_id: i64,
) -> Result<(), MetaError> {
    let db = |e: huehuecoyotl_library::DbError| MetaError::Other(e.to_string());
    let Some(t) = inner
        .lib
        .read(move |c| catalog::match_target(c, item))
        .await
        .map_err(db)?
    else {
        return Ok(());
    };
    let Some(ty) = media_type(&t.kind) else {
        return Ok(());
    };
    let d = p.details(ty, tmdb_id).await?;
    let meta = json!({ "genres": d.genres, "cast": d.cast }).to_string();
    let update = catalog::MetaUpdate {
        tmdb_id: Some(tmdb_id),
        title: Some(d.title.clone()),
        original_title: Some(d.original_title.clone()),
        year: d.year,
        overview: Some(d.overview.clone()),
        rating: d.rating,
        runtime_min: d.runtime_min,
        imdb_id: d.imdb_id.clone(),
        meta_json: Some(meta),
        match_state: Some("matched"),
    };
    let (poster, backdrop) = (d.poster_path.clone(), d.backdrop_path.clone());
    inner
        .lib
        .write(move |c| {
            catalog::apply_meta(c, item, &update)?;
            catalog::set_artwork(c, item, "poster", poster.as_deref())?;
            catalog::set_artwork(c, item, "poster_grid", poster.as_deref())?;
            catalog::set_artwork(c, item, "backdrop", backdrop.as_deref())?;
            Ok(())
        })
        .await
        .map_err(db)?;

    if ty == MediaType::Show {
        let structure = inner
            .lib
            .read(move |c| catalog::show_structure(c, item))
            .await
            .map_err(db)?;
        for (season_item, season_no, episodes) in structure {
            let s = match p.season(tmdb_id, season_no as u16).await {
                Ok(s) => s,
                Err(MetaError::NotFound) => continue,
                Err(e) => return Err(e),
            };
            let season_poster = s.poster_path.clone();
            let season_update = catalog::MetaUpdate {
                title: Some(s.name.clone()).filter(|n| !n.is_empty()),
                overview: Some(s.overview.clone()),
                ..Default::default()
            };
            let eps: Vec<(i64, catalog::MetaUpdate, Option<String>)> = episodes
                .iter()
                .filter_map(|(eid, no)| {
                    let e = s.episodes.iter().find(|e| i64::from(e.episode_no) == *no)?;
                    Some((
                        *eid,
                        catalog::MetaUpdate {
                            title: Some(e.title.clone()).filter(|t| !t.is_empty()),
                            overview: Some(e.overview.clone()),
                            year: e.year,
                            rating: e.rating,
                            runtime_min: e.runtime_min,
                            match_state: Some("matched"),
                            ..Default::default()
                        },
                        e.still_path.clone(),
                    ))
                })
                .collect();
            inner
                .lib
                .write(move |c| {
                    catalog::apply_meta(c, season_item, &season_update)?;
                    catalog::set_artwork(c, season_item, "poster_grid", season_poster.as_deref())?;
                    for (eid, u, still) in &eps {
                        catalog::apply_meta(c, *eid, u)?;
                        catalog::set_artwork(c, *eid, "still", still.as_deref())?;
                    }
                    Ok(())
                })
                .await
                .map_err(db)?;
        }
    }
    let payload = json!({ "item_id": item }).to_string();
    inner
        .lib
        .write(move |c| {
            jobs::enqueue(
                c,
                "artwork",
                &payload,
                jobs::priority::NORMAL,
                Some(&format!("artwork:{item}")),
            )
        })
        .await
        .map_err(db)?;
    inner.sink.post(Event::ItemChanged { item_id: item });
    Ok(())
}

/// (provider size, max stored width) per artwork role.
fn sizes(role: &str) -> (&'static str, u32) {
    match role {
        "poster" => ("w780", 780),
        "poster_grid" => ("w342", artwork::GRID_WIDTH),
        "backdrop" => ("w1280", 1280),
        _ => ("w300", 400), // stills
    }
}

async fn fetch_artwork(
    inner: &Arc<Inner>,
    p: &dyn MetadataProvider,
    item: i64,
) -> Result<(), MetaError> {
    let db = |e: huehuecoyotl_library::DbError| MetaError::Other(e.to_string());
    let pending = inner
        .lib
        .read(move |c| catalog::pending_artwork(c, item))
        .await
        .map_err(db)?;
    for (owner, role, remote) in pending {
        let (size, width) = sizes(&role);
        let bytes = p.image(&remote, size).await?;
        let scaled = tokio::task::spawn_blocking(move || artwork::scale_to_jpeg(&bytes, width))
            .await
            .map_err(|e| MetaError::Other(e.to_string()))?;
        let Ok((jpg, w, h)) = scaled else {
            tracing::warn!(owner, %role, "image could not be decoded");
            continue;
        };
        let dir = inner.data_dir.join("artwork").join(owner.to_string());
        let file = dir.join(format!("{role}.jpg"));
        let write = {
            let (dir, file) = (dir.clone(), file.clone());
            tokio::task::spawn_blocking(move || {
                std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, jpg))
            })
        };
        write
            .await
            .map_err(|e| MetaError::Other(e.to_string()))?
            .map_err(|e| MetaError::Other(e.to_string()))?;
        let local = VPath::new(file.to_string_lossy()).to_string();
        inner
            .lib
            .write(move |c| catalog::set_artwork_local(c, owner, &role, &local, w, h))
            .await
            .map_err(db)?;
    }
    inner.sink.post(Event::ItemChanged { item_id: item });
    Ok(())
}

/// Mark a whole show or season (or one item) watched or unwatched.
pub async fn set_watched_tree(inner: &Arc<Inner>, item: i64, watched: bool) -> Result<(), String> {
    inner
        .lib
        .write(move |c| {
            let ids: Vec<i64> = {
                let mut st = c.prepare(
                    "SELECT id FROM item WHERE id = ?1 AND kind IN ('movie','episode','video')
                     UNION SELECT id FROM item WHERE parent_id = ?1 AND kind = 'episode'
                     UNION SELECT e.id FROM item e JOIN item s ON e.parent_id = s.id WHERE s.parent_id = ?1",
                )?;
                st.query_map([item], |r| r.get(0))?.collect::<Result<_, _>>()?
            };
            for id in ids {
                queries::set_watched(c, id, watched)?;
            }
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())
}
