//! Matching again (2.7, issue #92, SPEC 7 "Matching again"): one way to match any item again,
//! to search for a match by hand and to choose one, for movies, shows, albums and audiobooks;
//! and library rematches that run in the background, one library at a time, through the job
//! queue (so they survive a restart and keep the providers' rate limits).
//!
//! Rules: a match the user chose is locked and automatic matching keeps it (unless a library
//! rematch includes locked items, or the user unlocks one); an automatic rematch never removes a
//! match it cannot replace with a sure one; metadata from the user's files is never replaced;
//! nothing is sent while the online setting is off or no TMDB key is set.

use std::sync::Arc;

use huehuecoyotl_library::{catalog, jobs, matching as m, music, rusqlite, versions};
use huehuecoyotl_metadata::books::{BookDecision, BookHit, BookQuery, decide as decide_book};
use huehuecoyotl_metadata::ids::{self, Ref};
use huehuecoyotl_metadata::matching::{Decision, decide};
use huehuecoyotl_metadata::music::{Decision as MusicDecision, ReleaseHit, decide as decide_music};
use huehuecoyotl_metadata::{MediaType, MetaError};
use serde_json::{Value, json};

use crate::{Core, CoreError, ErrorKind, Event, Inner};

pub const TMDB_THUMB: &str = "https://image.tmdb.org/t/p/w185";
pub const CAA_THUMB: &str = "https://coverartarchive.org/release";
pub const OL_COVERS: &str = "https://covers.openlibrary.org";

/// What a rematch did to one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The match stays (or there was none and still is none to choose).
    Unchanged,
    /// Matched now, and was not before.
    Newly,
    /// Matched to something else.
    Changed,
    /// Several fit: waits for the user's choice (the old data stays meanwhile).
    Review,
    /// Nothing sure found; an existing match stays.
    NotFound,
}

impl Outcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Newly => "newly",
            Self::Changed => "changed",
            Self::Review => "review",
            Self::NotFound => "notFound",
        }
    }
}

fn db(e: huehuecoyotl_library::DbError) -> MetaError {
    MetaError::Other(e.to_string())
}

fn year_of(date: Option<&str>) -> Option<u16> {
    date.and_then(|d| d.get(..4)).and_then(|y| y.parse().ok())
}

// ---- one item ----------------------------------------------------------------------------------

/// Match one item again as if it were new. `force`: also when the user chose its match (the
/// lock is lifted only when the match changes). Returns what happened and the item that carries
/// the result (a merge can keep another one).
pub(crate) async fn rematch_item(
    inner: &Arc<Inner>,
    item: i64,
    force: bool,
) -> Result<(Outcome, i64), MetaError> {
    let Some((kind, _)) = inner
        .lib
        .read(move |c| m::kind_title(c, item))
        .await
        .map_err(db)?
    else {
        return Err(MetaError::NotFound);
    };
    let locked = inner
        .lib
        .read(move |c| m::locked(c, item))
        .await
        .map_err(db)?;
    if locked && !force {
        return Ok((Outcome::Unchanged, item));
    }
    match kind.as_str() {
        "movie" | "show" => rematch_video(inner, item, locked).await,
        "album" => rematch_album(inner, item, locked).await,
        "audiobook" => rematch_book(inner, item, locked).await,
        "track" => crate::music_matching::rematch_track(inner, item).await,
        "artist" => crate::music_matching::rematch_artist(inner, item).await,
        "person" => crate::books_matching::rematch_author(inner, item).await,
        _ => Err(MetaError::Other(format!(
            "{kind} items cannot be matched again"
        ))),
    }
}

fn candidate_rows(
    cands: &[huehuecoyotl_metadata::Candidate],
    scores: &[f64],
) -> Vec<catalog::CandidateRow> {
    cands
        .iter()
        .zip(scores)
        .take(8)
        .map(|(c, s)| catalog::CandidateRow {
            provider_id: c.id,
            title: c.title.clone(),
            year: c.year,
            score: *s,
            poster_path: c.poster_path.clone(),
            overview: c.overview.clone(),
        })
        .collect()
}

async fn rematch_video(
    inner: &Arc<Inner>,
    item: i64,
    locked: bool,
) -> Result<(Outcome, i64), MetaError> {
    let p = inner.provider().ok_or(MetaError::Auth)?;
    let t = inner
        .lib
        .read(move |c| catalog::match_target(c, item))
        .await
        .map_err(db)?
        .ok_or(MetaError::NotFound)?;
    let state = inner
        .lib
        .read(move |c| {
            Ok(c.query_row(
                "SELECT coalesce(match_state, 'unmatched') FROM item WHERE id = ?1",
                [item],
                |r| r.get::<_, String>(0),
            )?)
        })
        .await
        .map_err(db)?;
    let was_matched = state == "matched" && t.tmdb_id.is_some();
    let ty = if t.kind == "show" {
        MediaType::Show
    } else {
        MediaType::Movie
    };
    let on_disk: Vec<(u16, u16)> = if ty == MediaType::Show {
        inner
            .lib
            .read(move |c| catalog::show_structure(c, item, true))
            .await
            .map_err(db)?
            .into_iter()
            .map(|(_, season, eps)| {
                let max = eps.iter().map(|(_, e)| *e).max().unwrap_or(0);
                (season as u16, max as u16)
            })
            .collect()
    } else {
        vec![]
    };
    let (decision, cands, scores) =
        crate::worker::find_match(&*p, ty, &t.title, t.year, &on_disk).await?;
    let best = cands.first().map(|c| c.id);
    match (decision, best) {
        (Decision::Matched, Some(id)) => {
            if was_matched && t.tmdb_id == Some(id) {
                return Ok((Outcome::Unchanged, item));
            }
            let new = inner
                .lib
                .write(move |c| {
                    let new = huehuecoyotl_library::merge::claim_tmdb(c, item, id)?;
                    c.execute(
                        "UPDATE item SET tmdb_id = ?2, match_state = 'matched', locked = 0 WHERE id = ?1",
                        rusqlite::params![new, id],
                    )?;
                    c.execute("DELETE FROM match_candidate WHERE item_id = ?1", [new])?;
                    Ok(new)
                })
                .await
                .map_err(db)?;
            crate::worker::enqueue_details(inner, new, id, jobs::priority::NORMAL).await;
            inner.sink.post(Event::ItemChanged { item_id: new });
            Ok((
                if was_matched {
                    Outcome::Changed
                } else {
                    Outcome::Newly
                },
                new,
            ))
        }
        (Decision::NeedsReview, _) => {
            // The current match among the plausible ones: it stays.
            if was_matched && cands.iter().take(3).any(|c| Some(c.id) == t.tmdb_id) {
                return Ok((Outcome::Unchanged, item));
            }
            let rows = candidate_rows(&cands, &scores);
            inner
                .lib
                .write(move |c| {
                    if locked {
                        m::set_locked(c, item, false)?;
                    }
                    catalog::set_match_state(c, item, "needs_review", &rows)
                })
                .await
                .map_err(db)?;
            inner.sink.post(Event::ItemChanged { item_id: item });
            Ok((Outcome::Review, item))
        }
        _ => {
            if !was_matched {
                inner
                    .lib
                    .write(move |c| catalog::set_match_state(c, item, "unmatched", &[]))
                    .await
                    .map_err(db)?;
            }
            Ok((Outcome::NotFound, item))
        }
    }
}

async fn rematch_album(
    inner: &Arc<Inner>,
    album: i64,
    locked: bool,
) -> Result<(Outcome, i64), MetaError> {
    let t = inner
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
        .ok_or(MetaError::Auth)?;
    let was_matched = t.match_state == "matched" && t.release_id.is_some();
    let p = inner.music();
    let tracks = (t.tracks > 0).then_some(t.tracks);
    let hits = p.search_releases(&t.artist, &t.title, tracks).await?;
    match decide_music(&hits, &t.artist, &t.title, tracks) {
        MusicDecision::Matched(id) => {
            let group = hits
                .iter()
                .find(|h| h.id == id)
                .and_then(|h| h.group_id.clone());
            let same = t.release_id.as_deref() == Some(id.as_str())
                || (group.is_some() && group == t.group_id);
            if was_matched && same {
                return Ok((Outcome::Unchanged, album));
            }
            let new = apply_release(inner, album, &id, false).await?;
            Ok((
                if was_matched {
                    Outcome::Changed
                } else {
                    Outcome::Newly
                },
                new,
            ))
        }
        MusicDecision::Review(hits) => {
            let current = |h: &ReleaseHit| {
                t.release_id.as_deref() == Some(h.id.as_str())
                    || (h.group_id.is_some() && h.group_id == t.group_id)
            };
            if was_matched && hits.iter().take(3).any(current) {
                return Ok((Outcome::Unchanged, album));
            }
            let cands: Vec<music::Candidate> = hits
                .iter()
                .map(|h| music::Candidate {
                    id: h.id.clone(),
                    title: h.title.clone(),
                    artist: h.artist.clone(),
                    year: year_of(h.date.as_deref()),
                    score: f64::from(h.score) / 100.0,
                    tracks: h.track_count,
                })
                .collect();
            inner
                .lib
                .write(move |c| {
                    if locked {
                        m::set_locked(c, album, false)?;
                    }
                    music::set_review(c, album, &cands)
                })
                .await
                .map_err(db)?;
            inner.sink.post(Event::ItemChanged { item_id: album });
            Ok((Outcome::Review, album))
        }
        MusicDecision::None => {
            if !was_matched {
                inner
                    .lib
                    .write(move |c| music::set_unmatched(c, album))
                    .await
                    .map_err(db)?;
            }
            Ok((Outcome::NotFound, album))
        }
    }
}

/// A release becomes the album's match (replacing an earlier one); `lock` when the user chose
/// it. Other copies of the release join the album. Returns the album that carries it.
pub(crate) async fn apply_release(
    inner: &Arc<Inner>,
    album: i64,
    release: &str,
    lock: bool,
) -> Result<i64, MetaError> {
    let p = inner.music();
    let r = p.release(release).await?;
    let f = crate::music::facts(&r);
    let has_cover = inner
        .lib
        .read(move |c| Ok(music::album_target(c, album)?.is_some_and(|t| t.has_cover)))
        .await
        .map_err(db)?;
    let (new, followed) = inner
        .lib
        .write(move |c| {
            let followed = music::replace_release(c, album, &f)?;
            m::set_locked(c, album, lock)?;
            c.execute("DELETE FROM match_candidate WHERE item_id = ?1", [album])?;
            let new = versions::merge_same_release(c, album)?;
            crate::music_matching::move_via(c, album, new)?;
            Ok((new, followed))
        })
        .await
        .map_err(db)?;
    if !has_cover && let Some(img) = p.front_cover(&r.id, r.group_id.as_deref()).await? {
        crate::worker::write_cover(
            inner,
            &inner.local,
            new,
            huehuecoyotl_scanner::music::Cover::Embedded(img.to_vec()),
        )
        .await;
    }
    crate::extras::enqueue_artists(inner, &[]).await;
    crate::music_matching::after_artists(inner, &followed).await;
    inner.sink.post(Event::ItemChanged { item_id: new });
    Ok(new)
}

async fn rematch_book(
    inner: &Arc<Inner>,
    id: i64,
    locked: bool,
) -> Result<(Outcome, i64), MetaError> {
    let t = inner
        .lib
        .read(move |c| {
            Ok(if music::online(c)? {
                huehuecoyotl_library::books::target(c, id)?
            } else {
                None
            })
        })
        .await
        .map_err(db)?
        .ok_or(MetaError::Auth)?;
    let was_matched = t.match_state == "matched";
    let p = inner.books();
    let author = t.authors.first().cloned().unwrap_or_default();
    let mut by_isbn = false;
    let mut hits = Vec::new();
    if let Some(isbn) = &t.isbn {
        hits = p.search(BookQuery::Isbn(isbn)).await?;
        by_isbn = !hits.is_empty();
    }
    if hits.is_empty() {
        hits = p
            .search(BookQuery::TitleAuthor {
                title: &t.title,
                author: &author,
            })
            .await?;
    }
    match decide_book(&hits, &t.title, &t.authors, by_isbn) {
        BookDecision::Matched(h) => {
            if was_matched && t.work.as_deref() == Some(h.id.as_str()) {
                return Ok((Outcome::Unchanged, id));
            }
            crate::books::apply_work(inner, &*p, &t, &h, None, was_matched).await?;
            crate::books_matching::follow_authors(inner, id, &h.author_ids).await?;
            inner
                .lib
                .write(move |c| m::set_locked(c, id, false))
                .await
                .map_err(db)?;
            inner.sink.post(Event::ItemChanged { item_id: id });
            Ok((
                if was_matched {
                    Outcome::Changed
                } else {
                    Outcome::Newly
                },
                id,
            ))
        }
        BookDecision::Review(hits) => {
            if was_matched
                && hits
                    .iter()
                    .take(3)
                    .any(|h| t.work.as_deref() == Some(h.id.as_str()))
            {
                return Ok((Outcome::Unchanged, id));
            }
            let cands = book_candidates(&hits);
            inner
                .lib
                .write(move |c| {
                    if locked {
                        m::set_locked(c, id, false)?;
                    }
                    if was_matched {
                        // The data stays; the book waits for a choice.
                        c.execute(
                            "UPDATE item SET match_state = 'needs_review' WHERE id = ?1",
                            [id],
                        )?;
                        c.execute("DELETE FROM match_candidate WHERE item_id = ?1", [id])?;
                        for k in &cands {
                            c.execute(
                                "INSERT INTO match_candidate(item_id, provider_id, title, year, score, overview) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                                rusqlite::params![id, k.id, k.title, k.year, k.score, k.artist],
                            )?;
                        }
                        Ok(())
                    } else {
                        music::set_review(c, id, &cands)
                    }
                })
                .await
                .map_err(db)?;
            inner.sink.post(Event::ItemChanged { item_id: id });
            Ok((Outcome::Review, id))
        }
        BookDecision::None => {
            if !was_matched {
                inner
                    .lib
                    .write(move |c| music::set_unmatched(c, id))
                    .await
                    .map_err(db)?;
            }
            Ok((Outcome::NotFound, id))
        }
    }
}

fn book_candidates(hits: &[BookHit]) -> Vec<music::Candidate> {
    hits.iter()
        .enumerate()
        .map(|(i, h)| music::Candidate {
            id: h.id.clone(),
            title: h.title.clone(),
            artist: h.authors.join(", "),
            year: h.year,
            score: 1.0 - i as f64 / 100.0,
            tracks: None,
        })
        .collect()
}

// ---- library rematch runs ----------------------------------------------------------------------

use huehuecoyotl_library::matching::job_kind;

/// Start the next waiting run, if no run is going on.
pub(crate) async fn kick(inner: &Arc<Inner>) {
    let started = inner
        .lib
        .write(|c| {
            m::reconcile(c)?;
            let Some(next) = m::next_run(c)? else {
                return Ok(false);
            };
            let Some((run, items)) = m::start_run(c, &next.library)? else {
                return Ok(false);
            };
            let kind = job_kind(&run.library);
            for id in &items {
                jobs::enqueue(
                    c,
                    kind,
                    &json!({ "item_id": id, "library": run.library }).to_string(),
                    jobs::priority::BACKGROUND,
                    Some(&format!("{kind}:{id}")),
                )?;
            }
            Ok(true)
        })
        .await;
    match started {
        Ok(true) => {
            inner.wake.notify_waiters();
            inner.mark_changed();
            // A run without items is done at once: the next one may start.
            Box::pin(kick(inner)).await;
        }
        Ok(false) => {}
        Err(e) => tracing::error!("starting a rematch failed: {e}"),
    }
}

/// The job: rematch one item of a library run and count it. Returns the provider's error for a
/// retry; anything else counts as done.
pub(crate) async fn run_job(inner: &Arc<Inner>, payload: &str) -> Result<(), MetaError> {
    let v: Value = serde_json::from_str(payload).unwrap_or_default();
    let item = v["item_id"].as_i64().unwrap_or(0);
    let library = v["library"].as_str().unwrap_or_default().to_string();
    let include_locked = inner
        .lib
        .read({
            let library = library.clone();
            move |c| {
                Ok(m::runs(c)?
                    .into_iter()
                    .find(|r| r.library == library)
                    .is_some_and(|r| r.include_locked))
            }
        })
        .await
        .map_err(db)?;
    let outcome = match rematch_item(inner, item, include_locked).await {
        Ok((o, _)) => o,
        Err(e @ (MetaError::RateLimited(_) | MetaError::Network(_) | MetaError::Auth)) => {
            return Err(e);
        }
        Err(e) => {
            tracing::info!(item, "rematch skipped: {e}");
            Outcome::Unchanged
        }
    };
    let done = inner
        .lib
        .write(move |c| m::count_result(c, &library, outcome.as_str()))
        .await
        .map_err(db)?;
    if done.as_ref().is_some_and(|r| r.state == "done") {
        tracing::info!(?done, "rematch finished");
        kick(inner).await;
    }
    inner.mark_changed();
    Ok(())
}

/// Settings → Matching: each library's counts, its run and whether it can run now.
async fn status(inner: &Arc<Inner>) -> crate::CResult<Value> {
    kick(inner).await;
    let has_key = inner.provider().is_some();
    let (online, list, runs) = inner
        .lib
        .read(|c| {
            let mut list = Vec::new();
            for l in m::LIBRARIES {
                list.push(m::counts(c, l)?);
            }
            Ok((music::online(c)?, list, m::runs(c)?))
        })
        .await?;
    let libraries: Vec<Value> = list
        .into_iter()
        .map(|n| {
            let blocked = match n.library.as_str() {
                "movies" | "shows" if !has_key => "noKey",
                "music" | "books" if !online => "offline",
                _ => "",
            };
            let run = runs.iter().find(|r| r.library == n.library);
            let mut v = serde_json::to_value(&n).unwrap_or_default();
            v["blocked"] = json!(blocked);
            v["run"] = run.map_or(Value::Null, |r| serde_json::to_value(r).unwrap_or_default());
            v
        })
        .collect();
    Ok(json!({ "libraries": libraries }))
}

fn matching_event(request_id: u64, v: Value) -> Event {
    Event::MusicDetail {
        request_id,
        kind: "matching".into(),
        json: v.to_string(),
    }
}

// ---- searching and choosing --------------------------------------------------------------------

fn tmdb_candidate(
    ty: MediaType,
    c: &huehuecoyotl_metadata::Candidate,
    score: f64,
    current: Option<i64>,
) -> Value {
    json!({
        "provider": "tmdb",
        "id": c.id.to_string(),
        "kind": if ty == MediaType::Show { "show" } else { "movie" },
        "title": c.title,
        "year": c.year,
        "overview": c.overview,
        "image": c.poster_path.as_deref().map(|p| format!("{TMDB_THUMB}{p}")).unwrap_or_default(),
        "score": score,
        "current": current == Some(c.id),
    })
}

fn release_candidate(h: &ReleaseHit, local_tracks: u32, current: Option<&str>) -> Value {
    json!({
        "provider": "musicbrainz",
        "id": h.id,
        "kind": "release",
        "title": h.title,
        "artist": h.artist,
        "year": year_of(h.date.as_deref()),
        "date": h.date,
        "country": h.country,
        "label": h.label,
        "formats": h.formats,
        "type": h.release_type,
        "tracks": h.track_count,
        "group": h.group_id,
        "localTracks": local_tracks,
        "good": local_tracks > 0 && h.track_count == Some(local_tracks),
        "score": f64::from(h.score) / 100.0,
        "image": format!("{CAA_THUMB}/{}/front-250", h.id),
        "current": current == Some(h.id.as_str()),
    })
}

fn book_candidate(h: &BookHit, title: &str, authors: &[String], current: Option<&str>) -> Value {
    let agrees = huehuecoyotl_metadata::books::agrees(h, title, authors);
    json!({
        "provider": "openlibrary",
        "id": h.id,
        "kind": "work",
        "title": h.title,
        "artist": h.authors.join(", "),
        "year": h.year,
        "fit": if agrees { "agrees" } else { "" },
        "good": agrees,
        "image": h.cover_id.map(|c| format!("{OL_COVERS}/b/id/{c}-M.jpg")).unwrap_or_default(),
        "current": current == Some(h.id.as_str()),
    })
}

/// What the dialog says about the item's current match.
async fn current_of(inner: &Arc<Inner>, item: i64) -> crate::CResult<Value> {
    Ok(inner
        .lib
        .read(move |c| {
            Ok(c.query_row(
                "SELECT kind, coalesce(title, ''), year, coalesce(match_state, 'unmatched'), locked, tmdb_id
                 FROM item WHERE id = ?1",
                [item],
                |r| {
                    Ok(json!({
                        "kind": r.get::<_, String>(0)?,
                        "title": r.get::<_, String>(1)?,
                        "year": r.get::<_, Option<i64>>(2)?,
                        "state": r.get::<_, String>(3)?,
                        "locked": r.get::<_, i64>(4)? != 0,
                        "tmdbId": r.get::<_, Option<i64>>(5)?,
                    }))
                },
            )?)
        })
        .await?)
}

pub(crate) fn unsupported(msg: &str) -> CoreError {
    CoreError::new(ErrorKind::Unsupported, msg.to_string())
}

fn offline_error() -> CoreError {
    unsupported("Music and book information online is off.")
}

fn no_key_error() -> CoreError {
    CoreError::new(ErrorKind::Auth, "No TMDB API key is set.".to_string())
}

/// Search for a match. `q`: `text` (a title, or an id or link), and by kind `year` and `kind`
/// ("movie" or "show") for video, `artist` for albums, `author` for books.
pub(crate) async fn search(inner: &Arc<Inner>, item: i64, q: &Value) -> crate::CResult<Value> {
    let current = current_of(inner, item).await?;
    let kind = current["kind"].as_str().unwrap_or_default().to_string();
    let text = q["text"].as_str().unwrap_or_default().trim().to_string();
    let reference = ids::parse(&text);
    let candidates: Vec<Value> = match kind.as_str() {
        "movie" | "show" | "episode" | "season" => {
            let p = inner.provider().ok_or_else(no_key_error)?;
            let want_show = match q["kind"].as_str() {
                Some("show") => true,
                Some("movie") => false,
                _ => kind != "movie",
            };
            let ty = if want_show {
                MediaType::Show
            } else {
                MediaType::Movie
            };
            let cur = current["tmdbId"].as_i64();
            match reference {
                Some(Ref::Tmdb { show, id }) => {
                    let ty = if show {
                        MediaType::Show
                    } else {
                        MediaType::Movie
                    };
                    let d = p.details(ty, id).await.map_err(crate::books::meta_err)?;
                    let c = huehuecoyotl_metadata::Candidate {
                        id: d.id,
                        title: d.title,
                        year: d.year,
                        overview: d.overview,
                        poster_path: d.poster_path,
                        ..Default::default()
                    };
                    vec![tmdb_candidate(ty, &c, 1.0, cur)]
                }
                Some(Ref::Imdb(imdb)) => p
                    .find_imdb(&imdb)
                    .await
                    .map_err(crate::books::meta_err)?
                    .iter()
                    .map(|(ty, c)| tmdb_candidate(*ty, c, 1.0, cur))
                    .collect(),
                _ => {
                    let title = if text.is_empty() {
                        current["title"].as_str().unwrap_or_default().to_string()
                    } else {
                        text.clone()
                    };
                    let year = q["year"].as_u64().filter(|y| *y > 0).map(|y| y as u16);
                    let mut found = p
                        .search(ty, &title, year)
                        .await
                        .map_err(crate::books::meta_err)?;
                    let (_, scores) = decide(&mut found, &title, year);
                    found
                        .iter()
                        .zip(scores)
                        .map(|(c, s)| tmdb_candidate(ty, c, s, cur))
                        .collect()
                }
            }
        }
        "album" => {
            let (online, t) = inner
                .lib
                .read(move |c| Ok((music::online(c)?, music::album_target(c, item)?)))
                .await?;
            if !online {
                return Err(offline_error());
            }
            let t =
                t.ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("album {item}")))?;
            let p = inner.music();
            let hits = match reference {
                Some(Ref::MbRelease(id) | Ref::Mbid(id)) => {
                    let r = p.release(&id).await.map_err(crate::books::meta_err)?;
                    vec![ReleaseHit {
                        id: r.id.clone(),
                        title: r.title.clone(),
                        artist: r
                            .artists
                            .iter()
                            .map(|a| format!("{}{}", a.name, a.join))
                            .collect(),
                        date: r.date.clone(),
                        track_count: Some(r.media.iter().map(|m| m.tracks.len() as u32).sum()),
                        group_id: r.group_id.clone(),
                        score: 100,
                        release_type: r.release_type.clone(),
                        ..Default::default()
                    }]
                }
                Some(Ref::MbReleaseGroup(g)) => {
                    p.group_releases(&g).await.map_err(crate::books::meta_err)?
                }
                _ => {
                    let title = if text.is_empty() {
                        t.title.clone()
                    } else {
                        text.clone()
                    };
                    let artist = q["artist"]
                        .as_str()
                        .map(str::trim)
                        .filter(|a| !a.is_empty())
                        .map_or_else(|| t.artist.clone(), str::to_string);
                    p.search_releases(&artist, &title, None)
                        .await
                        .map_err(crate::books::meta_err)?
                }
            };
            let current_release = t.release_id.clone();
            hits.iter()
                .map(|h| release_candidate(h, t.tracks, current_release.as_deref()))
                .collect()
        }
        "audiobook" => {
            let (online, t) = inner
                .lib
                .read(move |c| {
                    Ok((
                        music::online(c)?,
                        huehuecoyotl_library::books::target(c, item)?,
                    ))
                })
                .await?;
            if !online {
                return Err(offline_error());
            }
            let t = t.ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("book {item}")))?;
            let p = inner.books();
            let hits = match reference {
                Some(Ref::OlWork(id)) => {
                    let w = p.work(&id).await.map_err(crate::books::meta_err)?;
                    vec![BookHit {
                        id: w.id,
                        title: w.title,
                        year: w.year,
                        cover_id: w.cover_ids.first().copied(),
                        ..Default::default()
                    }]
                }
                Some(Ref::Isbn(isbn)) => p
                    .search(BookQuery::Isbn(&isbn))
                    .await
                    .map_err(crate::books::meta_err)?,
                _ => {
                    let isbn = q["isbn"].as_str().map(str::trim).unwrap_or_default();
                    if !isbn.is_empty() {
                        p.search(BookQuery::Isbn(isbn))
                            .await
                            .map_err(crate::books::meta_err)?
                    } else {
                        let title = if text.is_empty() {
                            t.title.clone()
                        } else {
                            text.clone()
                        };
                        let author = q["author"]
                            .as_str()
                            .map(str::trim)
                            .filter(|a| !a.is_empty())
                            .map_or_else(
                                || t.authors.first().cloned().unwrap_or_default(),
                                str::to_string,
                            );
                        p.search(BookQuery::TitleAuthor {
                            title: &title,
                            author: &author,
                        })
                        .await
                        .map_err(crate::books::meta_err)?
                    }
                }
            };
            hits.iter()
                .map(|h| book_candidate(h, &t.title, &t.authors, t.work.as_deref()))
                .collect()
        }
        "track" | "artist" => crate::music_matching::search(inner, item, &kind, &text, q).await?,
        "person" => crate::books_matching::search_authors(inner, item, &text).await?,
        other => {
            return Err(unsupported(&format!(
                "{other} items cannot be searched yet"
            )));
        }
    };
    Ok(json!({
        "itemId": item,
        "itemKind": kind,
        "current": current,
        "candidates": candidates,
    }))
}

/// The user chose a match: `choice` is a candidate (`provider`, `id`, `kind`). The match is
/// locked. Returns the item that carries it.
pub(crate) async fn choose(inner: &Arc<Inner>, item: i64, choice: &Value) -> crate::CResult<i64> {
    let current = current_of(inner, item).await?;
    let kind = current["kind"].as_str().unwrap_or_default().to_string();
    let id = choice["id"]
        .as_str()
        .map(str::to_string)
        .or_else(|| choice["id"].as_i64().map(|i| i.to_string()))
        .unwrap_or_default();
    if id.is_empty() {
        return Err(CoreError::new(
            ErrorKind::NotFound,
            "no match chosen".to_string(),
        ));
    }
    match kind.as_str() {
        "movie" | "show" | "episode" => {
            let chosen = choice["kind"].as_str().unwrap_or(&kind).to_string();
            // Issues #99 and #100: a movie that is really a show, or the reverse.
            if chosen != kind && !(kind == "episode" && chosen == "show") {
                return crate::kinds::change_kind(inner, item, choice).await;
            }
            if kind == "episode" {
                return Err(unsupported("choose the show's match on the show's page"));
            }
            let tmdb: i64 = id.parse().map_err(|_| unsupported("not a TMDB id"))?;
            let new = inner
                .lib
                .write(move |c| catalog::lock(c, item, Some(tmdb)))
                .await?;
            crate::worker::enqueue_details(inner, new, tmdb, jobs::priority::USER).await;
            inner.mark_changed();
            Ok(new)
        }
        "album" => {
            let new = apply_release(inner, item, &id, true)
                .await
                .map_err(crate::books::meta_err)?;
            inner.mark_changed();
            Ok(new)
        }
        "audiobook" => {
            let t = inner
                .lib
                .read(move |c| huehuecoyotl_library::books::target(c, item))
                .await?
                .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("book {item}")))?;
            let p = inner.books();
            let w = p.work(&id).await.map_err(crate::books::meta_err)?;
            let hit = BookHit {
                id: w.id.clone(),
                year: w.year,
                cover_id: w.cover_ids.first().copied(),
                ..Default::default()
            };
            let author_ids = w.author_ids.clone();
            crate::books::apply_work(inner, &*p, &t, &hit, Some(w), true)
                .await
                .map_err(crate::books::meta_err)?;
            // Issue #109: authors without a match follow the work's.
            if let Err(e) = crate::books_matching::follow_authors(inner, item, &author_ids).await {
                tracing::info!(item, "authors not followed: {e}");
            }
            inner
                .lib
                .write(move |c| {
                    m::set_locked(c, item, true)?;
                    c.execute("DELETE FROM match_candidate WHERE item_id = ?1", [item])?;
                    Ok(())
                })
                .await?;
            inner.sink.post(Event::ItemChanged { item_id: item });
            inner.mark_changed();
            Ok(item)
        }
        "track" | "artist" => crate::music_matching::choose(inner, item, &kind, &id).await,
        "person" => crate::books_matching::choose_author(inner, item, &id).await,
        other => Err(unsupported(&format!(
            "{other} items cannot be matched by hand yet"
        ))),
    }
}

fn result_event(request_id: u64, kind: &str, v: Value) -> Event {
    Event::MusicDetail {
        request_id,
        kind: kind.into(),
        json: v.to_string(),
    }
}

impl Core {
    /// Issue #92: match one item again. A match the user chose is kept unless `unlock`; then the
    /// answer says "locked". Answers with a `MusicDetail` of kind "matchAgain": `itemId`,
    /// `newItemId`, `result` (unchanged, newly, changed, review, notFound, locked), `title`.
    pub fn match_again(&self, request_id: u64, item: i64, unlock: bool) {
        self.spawn(request_id, move |inner| async move {
            let locked = inner.lib.read(move |c| m::locked(c, item)).await?;
            if locked && !unlock {
                return Ok(result_event(
                    request_id,
                    "matchAgain",
                    json!({ "itemId": item, "newItemId": item, "result": "locked" }),
                ));
            }
            let (outcome, new) = rematch_item(&inner, item, unlock).await.map_err(|e| match e {
                MetaError::Auth => CoreError::new(
                    ErrorKind::Auth,
                    "Matching needs the TMDB key, or music and book information online.".to_string(),
                ),
                e => crate::books::meta_err(e),
            })?;
            inner.mark_changed();
            let title = inner
                .lib
                .read(move |c| m::kind_title(c, new))
                .await?
                .map(|(_, t)| t)
                .unwrap_or_default();
            Ok(result_event(
                request_id,
                "matchAgain",
                json!({ "itemId": item, "newItemId": new, "result": outcome.as_str(), "title": title }),
            ))
        });
    }

    /// Issue #92: search for a match by hand. `query_json`: `text` (title, id or link), and
    /// `year`, `kind` (movie or show), `artist`, `author` or `isbn` by the item's kind. Answers
    /// with a `MusicDetail` of kind "matchCandidates".
    pub fn match_search(&self, request_id: u64, item: i64, query_json: &str) {
        let q: Value = serde_json::from_str(query_json).unwrap_or_default();
        self.spawn(request_id, move |inner| async move {
            let v = search(&inner, item, &q).await?;
            Ok(result_event(request_id, "matchCandidates", v))
        });
    }

    /// Issue #92: the user's choice among the candidates (`choice_json`: one candidate). Answers
    /// with a `MusicDetail` of kind "matchChosen": `itemId`, `newItemId`.
    pub fn match_choose(&self, request_id: u64, item: i64, choice_json: &str) {
        let choice: Value = serde_json::from_str(choice_json).unwrap_or_default();
        self.spawn(request_id, move |inner| async move {
            let new = choose(&inner, item, &choice).await?;
            Ok(result_event(
                request_id,
                "matchChosen",
                json!({ "itemId": item, "newItemId": new }),
            ))
        });
    }

    /// Issues #97 to #100: what choosing `choice_json` would do, before it is applied (a merge,
    /// episodes the new show lacks, or for a kind change the files that move). Answers with a
    /// `MusicDetail` of kind "matchPreview".
    pub fn match_preview(&self, request_id: u64, item: i64, choice_json: &str) {
        let choice: Value = serde_json::from_str(choice_json).unwrap_or_default();
        self.spawn(request_id, move |inner| async move {
            let kind = inner
                .lib
                .read(move |c| m::kind_title(c, item))
                .await?
                .map(|(k, _)| k)
                .unwrap_or_default();
            // Issue #102: an album against a release, track by track.
            let v = if kind == "album" {
                let release = choice["id"].as_str().unwrap_or_default().to_string();
                crate::music_matching::preview_album(&inner, item, &release).await?
            } else if kind == "audiobook" {
                // Issue #109: what the work would add, field by field.
                let work = choice["id"].as_str().unwrap_or_default().to_string();
                crate::books_matching::preview_book(&inner, item, &work).await?
            } else {
                crate::kinds::preview(&inner, item, &choice).await?
            };
            Ok(result_event(request_id, "matchPreview", v))
        });
    }

    /// Issue #92: match a whole library again (`library`: movies, shows, music, books): only the
    /// unmatched and uncertain items, or `all`; matches the user chose only with
    /// `include_locked`. Waits for a run of another library. Answers with the status
    /// (`MusicDetail` "matching").
    pub fn rematch_library(&self, request_id: u64, library: &str, all: bool, include_locked: bool) {
        let library = library.to_string();
        self.spawn(request_id, move |inner| async move {
            if m::item_kind(&library).is_none() {
                return Err(CoreError::new(
                    ErrorKind::NotFound,
                    format!("library {library}"),
                ));
            }
            let online = inner.lib.read(music::online).await?;
            match library.as_str() {
                "movies" | "shows" if inner.provider().is_none() => return Err(no_key_error()),
                "music" | "books" if !online => return Err(offline_error()),
                _ => {}
            }
            let l2 = library.clone();
            inner
                .lib
                .write(move |c| m::add_run(c, &l2, all, include_locked))
                .await?;
            kick(&inner).await;
            Ok(matching_event(request_id, status(&inner).await?))
        });
    }

    /// Issue #92: stop a library's rematch (or take it out of the line). What was matched so
    /// far stays.
    pub fn rematch_stop(&self, request_id: u64, library: &str) {
        let library = library.to_string();
        self.spawn(request_id, move |inner| async move {
            let kind = job_kind(&library);
            let l2 = library.clone();
            inner
                .lib
                .write(move |c| {
                    c.execute(
                        "DELETE FROM job WHERE kind = ?1 AND state = 'queued' AND json_extract(payload_json, '$.library') = ?2",
                        rusqlite::params![kind, l2],
                    )?;
                    m::update_run(c, &l2, |r| {
                        if r.state == "running" || r.state == "waiting" {
                            r.state = "stopped".into();
                            r.finished_at = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map_or(0, |d| d.as_millis() as i64);
                        }
                    })?;
                    Ok(())
                })
                .await?;
            kick(&inner).await;
            inner.mark_changed();
            Ok(matching_event(request_id, status(&inner).await?))
        });
    }

    /// Issue #92: Settings → Matching: every library's counts, its last or current run and
    /// whether it can run now (`blocked`: noKey, offline). `MusicDetail` of kind "matching".
    pub fn matching_status(&self, request_id: u64) {
        self.spawn(request_id, move |inner| async move {
            Ok(matching_event(request_id, status(&inner).await?))
        });
    }
}
