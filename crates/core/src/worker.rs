//! Job workers (SPEC 7): one for scans, one for metadata. Jobs come from the persisted queue;
//! failures back off, rate limits wait for the server's Retry-After, and metadata jobs wait
//! while no API key is configured.

use std::sync::Arc;
use std::time::Duration;

use huehuecoyotl_library::{catalog, jobs, merge, queries};
use huehuecoyotl_metadata::matching::{
    Decision, PLAUSIBLE, decide, filter_country, fits_structure, search_queries, split_country,
};
use huehuecoyotl_metadata::{Candidate, MediaType, MetaError, MetadataProvider, artwork};
use huehuecoyotl_scanner::music::{Cover, scan_music_source};
use huehuecoyotl_scanner::scan::{ScanError, regroup_shows, scan_source};
use huehuecoyotl_vfs::VPath;
use serde_json::json;

use crate::{Event, Inner};

pub const SCAN_KINDS: &[&str] = &["scan", "tiers", "regroup"];
pub const META_KINDS: &[&str] = &["match", "details", "artwork", "rematch_video"];
/// Issue #11: music lookups have their own worker; MusicBrainz's pace never holds up TMDB.
pub const MUSIC_KINDS: &[&str] = &["music_match", "music_artist", "rematch_music"];
/// Issue #67: book lookups (Open Library's pace) have their own worker too.
pub const BOOK_KINDS: &[&str] = &["book_match", "rematch_book"];

enum Outcome {
    Done,
    Retry(String, Option<i64>),
    /// Not now, and it does not count as an attempt.
    Defer(i64),
}

pub fn spawn_workers(inner: &Arc<Inner>) {
    // Probes have their own worker, so analysing files never holds up scans or metadata.
    for (kinds, needs_provider) in [
        (SCAN_KINDS, false),
        (META_KINDS, true),
        (crate::probe::PROBE_KINDS, false),
        (MUSIC_KINDS, false),
        (BOOK_KINDS, false),
    ] {
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
        "tiers" => large_tiers(inner).await,
        "regroup" => regroup(inner).await,
        "probe" => crate::probe::run(inner, v["file_id"].as_i64().unwrap_or(0)).await,
        "waveform" => crate::probe::waveform(inner, v["file_id"].as_i64().unwrap_or(0)).await,
        "music_match" => match crate::music::match_album(inner, item_of(payload)).await {
            Ok(()) | Err(MetaError::NotFound) => Ok(()),
            Err(MetaError::RateLimited(s)) => {
                return Outcome::Retry("MusicBrainz is busy".into(), Some(s as i64));
            }
            Err(e) => Err(e.to_string()),
        },
        "book_match" => match crate::books::match_book(inner, item_of(payload)).await {
            Ok(()) | Err(MetaError::NotFound) => Ok(()),
            Err(MetaError::RateLimited(s)) => {
                return Outcome::Retry("Open Library is busy".into(), Some(s as i64));
            }
            Err(e) => Err(e.to_string()),
        },
        // Issue #12: TheAudioDB and Fanart.tv.
        "music_artist" => match crate::extras::lookup_artist(inner, item_of(payload)).await {
            Ok(()) | Err(MetaError::NotFound) | Err(MetaError::Auth) => Ok(()),
            Err(MetaError::RateLimited(s)) => {
                return Outcome::Retry("rate limited".into(), Some(s as i64));
            }
            Err(e) => Err(e.to_string()),
        },
        // Issue #92: one item of a library rematch.
        "rematch_video" | "rematch_music" | "rematch_book" => {
            match crate::matching::run_job(inner, payload).await {
                Ok(()) => Ok(()),
                Err(MetaError::RateLimited(s)) => {
                    return Outcome::Retry("rate limited".into(), Some(s as i64));
                }
                Err(MetaError::Auth) => return Outcome::Defer(600),
                Err(e) => Err(e.to_string()),
            }
        }
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
    // Issue #146: a scan or a music match may have given two artists one ID or one name.
    if res.is_ok() && matches!(kind, "scan" | "music_match" | "rematch_music") {
        crate::music_matching::reconcile_artists(inner).await;
    }
    match res {
        Ok(()) => Outcome::Done,
        Err(e) => Outcome::Retry(e, None),
    }
}

async fn scan(inner: &Arc<Inner>, source_id: i64) -> Result<(), String> {
    let row = inner
        .lib
        .read(move |c| catalog::source(c, source_id))
        .await
        .map_err(|e| e.to_string())?;
    // The library was removed meanwhile.
    let Some((kind, Some(root), content)) = row.map(|r| (r.kind, r.root, r.content)) else {
        return Ok(());
    };
    let remote = if kind == "local" {
        None
    } else {
        match inner.remote_source(source_id).await {
            Ok(s) => Some(s),
            // Offline or a changed password: report it, keep the data, try again later.
            Err(e) => {
                inner.sink.post(Event::ScanState {
                    source_id,
                    running: false,
                    message: e.message,
                });
                return Ok(());
            }
        }
    };
    inner.scanning.lock().unwrap().insert(source_id);
    inner.sink.post(Event::ScanState {
        source_id,
        running: true,
        message: String::new(),
    });
    let root = match &remote {
        Some(_) if root.is_empty() => "/".to_string(),
        _ => root,
    };
    let src: &dyn huehuecoyotl_vfs::Source = match &remote {
        None => &inner.local,
        Some(s) => &**s,
    };
    if content == "music" {
        return scan_music(inner, src, source_id, &VPath::new(root)).await;
    }
    if content == "audiobooks" {
        return scan_books(inner, src, source_id, &VPath::new(root)).await;
    }
    let res = scan_source(&inner.lib, src, source_id, &VPath::new(root)).await;
    if let Err(ScanError::Vfs(huehuecoyotl_vfs::VfsError::Unreachable(_))) = &res {
        inner.set_online(source_id, false);
    }
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
            // New and changed files get their technical details in the background.
            crate::probe::enqueue_unprobed(inner).await;
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

/// A music folder (issue #7): tags, albums and artists, then the albums' covers.
async fn scan_music(
    inner: &Arc<Inner>,
    src: &dyn huehuecoyotl_vfs::Source,
    source_id: i64,
    root: &VPath,
) -> Result<(), String> {
    let res = scan_music_source(&inner.lib, src, source_id, root).await;
    if let Err(ScanError::Vfs(huehuecoyotl_vfs::VfsError::Unreachable(_))) = &res {
        inner.set_online(source_id, false);
    }
    inner.scanning.lock().unwrap().remove(&source_id);
    let (message, out) = match res {
        Ok(r) => {
            for (album, cover) in r.covers {
                write_cover(inner, src, album, cover).await;
            }
            crate::music::enqueue_lookups(inner, &r.new_albums).await;
            inner.mark_changed();
            (
                format!(
                    "{} files, {} new, {} removed",
                    r.files_seen, r.added, r.removed
                ),
                Ok(()),
            )
        }
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

/// Issue #65: an audiobook folder.
async fn scan_books(
    inner: &Arc<Inner>,
    src: &dyn huehuecoyotl_vfs::Source,
    source_id: i64,
    root: &VPath,
) -> Result<(), String> {
    let res =
        huehuecoyotl_scanner::audiobooks::scan_audiobook_source(&inner.lib, src, source_id, root)
            .await;
    if let Err(ScanError::Vfs(huehuecoyotl_vfs::VfsError::Unreachable(_))) = &res {
        inner.set_online(source_id, false);
    }
    inner.scanning.lock().unwrap().remove(&source_id);
    let (message, out) = match res {
        Ok(r) => {
            for (book, cover) in r.covers {
                write_cover(inner, src, book, cover).await;
                crate::books::keep_ratio(inner, book).await;
            }
            // Embedded chapters come from the probe (single files, M4B).
            let books = r.probe.clone();
            let _ = inner
                .lib
                .write(move |c| {
                    for b in &books {
                        let files: Vec<i64> = {
                            let mut st =
                                c.prepare("SELECT file_id FROM book_file WHERE book_id = ?1")?;
                            st.query_map([b], |r| r.get(0))?
                                .collect::<huehuecoyotl_library::rusqlite::Result<_>>()?
                        };
                        for f in files {
                            huehuecoyotl_library::jobs::enqueue(
                                c,
                                "probe",
                                &serde_json::json!({ "file_id": f }).to_string(),
                                huehuecoyotl_library::jobs::priority::NORMAL,
                                Some(&format!("probe:{f}")),
                            )?;
                        }
                    }
                    Ok(())
                })
                .await;
            inner.wake.notify_waiters();
            // Issue #67: new books (and any due) are looked up online.
            crate::books::enqueue_lookups(inner, &r.new_books).await;
            inner.mark_changed();
            (
                format!(
                    "{} files, {} books, {} read again, {} removed",
                    r.files_seen, r.books, r.read, r.removed
                ),
                Ok(()),
            )
        }
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

/// An album's cover from its tags or a sidecar image, in the tiers posters have: the full image
/// (`poster`, at most 1000 px) and the two grid tiers. Failures are logged and skipped.
pub(crate) async fn write_cover(
    inner: &Arc<Inner>,
    src: &dyn huehuecoyotl_vfs::Source,
    album: i64,
    cover: Cover,
) {
    let data = match cover {
        Cover::Embedded(b) => b,
        Cover::Sidecar(path) => {
            let size = match src.stat(&path).await {
                Ok(e) => e.size,
                Err(e) => {
                    tracing::warn!(album, "cover not readable: {e}");
                    return;
                }
            };
            if size > 32 << 20 {
                tracing::warn!(album, size, "cover too large, skipped");
                return;
            }
            let mut data = Vec::with_capacity(size as usize);
            while (data.len() as u64) < size {
                let want = (size - data.len() as u64).min(4 << 20) as u32;
                match src.read_at(&path, data.len() as u64, want).await {
                    Ok(b) if !b.is_empty() => data.extend_from_slice(&b),
                    Ok(_) => break,
                    Err(e) => {
                        tracing::warn!(album, "cover not readable: {e}");
                        return;
                    }
                }
            }
            data
        }
    };
    let dir = inner.data_dir.join("artwork").join(album.to_string());
    let data = Arc::new(data);
    for (role, width) in [
        ("poster", 1000),
        ("poster_grid", artwork::GRID_WIDTH),
        ("poster_grid_l", artwork::GRID_LARGE_WIDTH),
    ] {
        let (d, dir) = (data.clone(), dir.clone());
        let made = tokio::task::spawn_blocking(move || -> Result<(String, u32, u32), String> {
            let (jpg, w, h) = artwork::scale_to_jpeg(&d, width)?;
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let file = dir.join(format!("{role}.jpg"));
            std::fs::write(&file, jpg).map_err(|e| e.to_string())?;
            Ok((VPath::new(file.to_string_lossy()).to_string(), w, h))
        })
        .await;
        match made {
            Ok(Ok((local, w, h))) => {
                let _ = inner
                    .lib
                    .write(move |c| catalog::set_artwork_tier(c, album, role, &local, w, h))
                    .await;
            }
            Ok(Err(e)) => {
                tracing::warn!(album, "cover could not be decoded: {e}");
                return;
            }
            Err(e) => {
                tracing::warn!(album, "cover not written: {e}");
                return;
            }
        }
    }
    inner.sink.post(Event::ItemChanged { item_id: album });
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
            return Ok(());
        }
        // Locked without a match: a version split off by the user (issue #34). Offer the
        // candidates in the review list, never match it on its own.
        let mut cands = p.search(ty, &t.title, t.year).await?;
        let (_, scores) = decide(&mut cands, &t.title, t.year);
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
        inner
            .lib
            .write(move |c| catalog::set_match_state(c, item, "needs_review", &rows))
            .await
            .map_err(|e| MetaError::Other(e.to_string()))?;
        inner.sink.post(Event::ItemChanged { item_id: item });
        return Ok(());
    }
    let on_disk = if ty == MediaType::Show {
        inner
            .lib
            .read(move |c| catalog::show_structure(c, item, true))
            .await
            .map_err(|e| MetaError::Other(e.to_string()))?
            .into_iter()
            .map(|(_, season, eps)| {
                let max = eps.iter().map(|(_, e)| *e).max().unwrap_or(0);
                (season as u16, max as u16)
            })
            .collect()
    } else {
        vec![]
    };
    let (decision, cands, scores) = find_match(p, ty, &t.title, t.year, &on_disk).await?;
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

/// At most this many search queries per item (the title, then shorter parts of it).
const MAX_QUERIES: usize = 3;
/// At most this many candidates' details are fetched to settle an ambiguous show.
const MAX_STRUCTURE_CHECKS: usize = 3;

/// Search the provider for an item and decide. For shows (issue #32):
/// - a country suffix ("Example Show US") becomes an origin-country filter;
/// - when the title finds nothing usable, shorter leading parts are tried, still scored against
///   the full title, so punctuation differences ("Show The Return" / "Show: The Return") match;
/// - when several candidates fit the title, the seasons and episodes on disk (`on_disk`:
///   season, highest episode) decide; if exactly one candidate can hold them, it is the match,
///   otherwise the show goes to the review list.
pub async fn find_match(
    p: &dyn MetadataProvider,
    ty: MediaType,
    title: &str,
    year: Option<u16>,
    on_disk: &[(u16, u16)],
) -> Result<(Decision, Vec<Candidate>, Vec<f64>), MetaError> {
    if ty == MediaType::Movie {
        let mut cands = p.search(ty, title, year).await?;
        let (d, scores) = decide(&mut cands, title, year);
        return Ok((d, cands, scores));
    }
    let (base, country) = split_country(title);
    let mut best = (Decision::Unmatched, Vec::new(), Vec::new());
    for q in search_queries(&base).into_iter().take(MAX_QUERIES) {
        let mut cands = p.search(ty, &q, year).await?;
        filter_country(&mut cands, country);
        let (d, scores) = decide(&mut cands, &base, year);
        let found = d != Decision::Unmatched;
        if found || best.1.is_empty() {
            best = (d, cands, scores);
        }
        if found {
            break;
        }
    }
    let (mut decision, mut cands, mut scores) = best;
    if decision == Decision::NeedsReview && on_disk.iter().any(|(s, _)| *s > 0) {
        let mut fitting = Vec::new();
        for (i, c) in cands.iter().enumerate().take(MAX_STRUCTURE_CHECKS) {
            if scores[i] < PLAUSIBLE {
                break;
            }
            let d = p.details(ty, c.id).await?;
            if fits_structure(&d.seasons, on_disk) {
                fitting.push(i);
            }
        }
        if let [only] = fitting[..] {
            cands.swap(0, only);
            scores.swap(0, only);
            decision = Decision::Matched;
        }
    }
    Ok((decision, cands, scores))
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
    // The TMDB id is the identity (issue #32): if another movie or show already has it, the
    // two become one item, and the details go to the survivor.
    let item = inner
        .lib
        .write(move |c| merge::claim_tmdb(c, item, tmdb_id))
        .await
        .map_err(db)?;
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
        // Every season TMDB lists, also the ones not on disk: the show page shows them dimmed.
        let tmdb_seasons: Vec<(i64, String)> = d
            .seasons
            .iter()
            .map(|s| (i64::from(s.season_no), s.name.clone()))
            .collect();
        let structure = inner
            .lib
            .write(move |c| {
                for (no, name) in &tmdb_seasons {
                    let title = if name.is_empty() {
                        format!("Season {no}")
                    } else {
                        name.clone()
                    };
                    catalog::ensure_season(c, item, *no, &title)?;
                }
                catalog::show_structure(c, item, false)
            })
            .await
            .map_err(db)?;
        for (season_item, season_no, _) in structure {
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
            // Every episode TMDB lists; the ones not on disk become virtual items.
            let eps: Vec<(i64, String, catalog::MetaUpdate, Option<String>)> = s
                .episodes
                .iter()
                .map(|e| {
                    (
                        i64::from(e.episode_no),
                        e.title.clone(),
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
                    )
                })
                .collect();
            inner
                .lib
                .write(move |c| {
                    catalog::apply_meta(c, season_item, &season_update)?;
                    catalog::set_artwork(c, season_item, "poster_grid", season_poster.as_deref())?;
                    for (no, title, u, still) in &eps {
                        let title = if title.is_empty() {
                            format!("Episode {no}")
                        } else {
                            title.clone()
                        };
                        let eid = catalog::ensure_episode(c, season_item, *no, &title)?;
                        catalog::apply_meta(c, eid, u)?;
                        catalog::set_artwork(c, eid, "still", still.as_deref())?;
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
        // Fetched at w780 so the larger grid tier can be made from the same download.
        "poster_grid" => ("w780", artwork::GRID_WIDTH),
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
    // The full poster and the grid thumbnail usually share one image: download it once.
    let mut fetched: std::collections::HashMap<(String, &str), Arc<[u8]>> = Default::default();
    for (owner, role, remote) in pending {
        let (size, width) = sizes(&role);
        let bytes = match fetched.get(&(remote.clone(), size)) {
            Some(b) => b.clone(),
            None => {
                let b: Arc<[u8]> = Arc::from(&p.image(&remote, size).await?[..]);
                fetched.insert((remote.clone(), size), b.clone());
                b
            }
        };
        if role == "poster_grid" {
            let b = bytes.clone();
            write_large_tier(inner, owner, move || Ok(b.to_vec())).await;
        }
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

/// Writes the `poster_grid_l` tier of an item from a full-size image. Failures are logged and
/// skipped: the grid then keeps using the 360 px tier.
async fn write_large_tier(
    inner: &Arc<Inner>,
    item: i64,
    source: impl FnOnce() -> std::io::Result<Vec<u8>> + Send + 'static,
) -> bool {
    let file = inner
        .data_dir
        .join("artwork")
        .join(item.to_string())
        .join("poster_grid_l.jpg");
    let target = file.clone();
    let made = tokio::task::spawn_blocking(move || -> Result<(u32, u32), String> {
        let data = source().map_err(|e| e.to_string())?;
        let (jpg, w, h) = artwork::scale_to_jpeg(&data, artwork::GRID_LARGE_WIDTH)?;
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&target, jpg).map_err(|e| e.to_string())?;
        Ok((w, h))
    })
    .await;
    let (w, h) = match made {
        Ok(Ok(size)) => size,
        Ok(Err(e)) => {
            tracing::warn!(item, "large grid tier not written: {e}");
            return false;
        }
        Err(e) => {
            tracing::warn!(item, "large grid tier not written: {e}");
            return false;
        }
    };
    let local = VPath::new(file.to_string_lossy()).to_string();
    match inner
        .lib
        .write(move |c| catalog::set_artwork_tier(c, item, "poster_grid_l", &local, w, h))
        .await
    {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(item, "large grid tier not recorded: {e}");
            false
        }
    }
}

/// Background job queued by migration 4 (issue #32, no network): give every show the key the
/// current parser derives and merge the duplicates older versions created. Shows whose title
/// changed and that have no match yet are matched again.
async fn regroup(inner: &Arc<Inner>) -> Result<(), String> {
    let report = inner
        .lib
        .write(|c| {
            let tx = c.transaction()?;
            let report = regroup_shows(&tx)
                .map_err(|e| huehuecoyotl_library::DbError::Other(e.to_string()))?;
            for id in &report.rematch {
                jobs::enqueue(
                    &tx,
                    "match",
                    &json!({ "item_id": id }).to_string(),
                    jobs::priority::NORMAL,
                    Some(&format!("match:{id}")),
                )?;
            }
            tx.commit()?;
            Ok(report)
        })
        .await
        .map_err(|e| e.to_string())?;
    if !report.rematch.is_empty() {
        inner.wake.notify_waiters();
    }
    // The job loop reports the library change (grids re-query) after every job.
    Ok(())
}

/// Number of tiers one `tiers` job makes before it re-queues itself, so scans are not held up.
const TIER_BATCH: u32 = 100;

/// Background job (no network): makes the 540 px grid tier from downloaded full-size posters for
/// libraries created before it existed. Until a tier exists the grid shows the 360 px one.
async fn large_tiers(inner: &Arc<Inner>) -> Result<(), String> {
    let todo = inner
        .lib
        .read(|c| catalog::missing_large_tiers(c, TIER_BATCH))
        .await
        .map_err(|e| e.to_string())?;
    let full = todo.len() as u32 == TIER_BATCH;
    let last = todo.last().map_or(0, |t| t.0);
    let mut made = 0;
    for (item, original) in todo {
        let path = std::path::PathBuf::from(original);
        // An unreadable original is skipped; the item keeps the 360 px tier.
        if write_large_tier(inner, item, move || std::fs::read(path)).await {
            made += 1;
        }
    }
    if made > 0 {
        tracing::info!(made, "large grid tiers written");
    }
    if full && made > 0 {
        // Its own key: the running job still holds "tiers", and a merged entry would be
        // completed with it.
        enqueue_tiers(inner, jobs::priority::BACKGROUND, &format!("tiers:{last}")).await;
    }
    Ok(())
}

/// Queue the `tiers` job, deduplicated by `key`.
pub async fn enqueue_tiers(inner: &Arc<Inner>, prio: i64, key: &str) {
    let key = key.to_string();
    let r = inner
        .lib
        .write(move |c| jobs::enqueue(c, "tiers", "{}", prio, Some(&key)))
        .await;
    match r {
        Ok(_) => inner.wake.notify_waiters(),
        Err(e) => tracing::error!("queueing the tiers job failed: {e}"),
    }
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
