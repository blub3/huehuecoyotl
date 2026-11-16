//! Book information online (issue #67, SPEC 7 "Book providers"): a book tagged with a
//! MusicBrainz release id is looked up there (its year and the Cover Art Archive's cover);
//! others at Open Library, by ISBN when the local metadata has one (one search), else by title
//! and author. Only what the local metadata left empty is filled; a doubtful result waits for
//! the user ("Search again"). With "Music and book information online" off, nothing is sent.

use std::sync::Arc;

use huehuecoyotl_library::{books, jobs, music};
use huehuecoyotl_metadata::MetaError;
use huehuecoyotl_metadata::books::{BookDecision, BookHit, BookProvider, BookQuery, Work, decide};
use huehuecoyotl_scanner::music::Cover;
use serde_json::json;

use crate::{Core, CoreError, ErrorKind, Event, Inner};

impl Inner {
    pub(crate) fn books(&self) -> Arc<dyn BookProvider> {
        self.books.read().unwrap().clone()
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn db(e: huehuecoyotl_library::DbError) -> MetaError {
    MetaError::Other(e.to_string())
}

/// Queue lookups for these books and every book due for one, unless online information is off.
pub(crate) async fn enqueue_lookups(inner: &Arc<Inner>, ids: &[i64]) {
    let ids = ids.to_vec();
    let queued = inner
        .lib
        .write(move |c| {
            if !music::online(c)? {
                return Ok(false);
            }
            let mut all = ids;
            for id in books::due(c, now_ms())? {
                if !all.contains(&id) {
                    all.push(id);
                }
            }
            for id in all {
                jobs::enqueue(
                    c,
                    "book_match",
                    &json!({ "item_id": id }).to_string(),
                    jobs::priority::NORMAL,
                    Some(&format!("book_match:{id}")),
                )?;
            }
            Ok(true)
        })
        .await;
    if matches!(queued, Ok(true)) {
        inner.wake.notify_waiters();
    }
}

/// Write a cover into the poster tiers and keep its aspect for the grid.
pub(crate) async fn set_cover(inner: &Arc<Inner>, book: i64, cover: Cover) {
    crate::worker::write_cover(inner, &inner.local, book, cover).await;
    keep_ratio(inner, book).await;
}

/// One grid shows square and portrait covers: keep the cover's aspect.
pub(crate) async fn keep_ratio(inner: &Arc<Inner>, book: i64) {
    let _ = inner
        .lib
        .write(move |c| {
            c.execute(
                "UPDATE book SET cover_ratio = (SELECT CAST(height AS REAL) / width FROM artwork
                   WHERE item_id = ?1 AND role = 'poster' AND width > 0) WHERE item_id = ?1",
                [book],
            )?;
            Ok(())
        })
        .await;
}

/// A work's details become the book's (filling gaps only), and its cover when it has none.
async fn apply_work(
    inner: &Arc<Inner>,
    p: &dyn BookProvider,
    t: &books::BookTarget,
    hit: &BookHit,
    work: Option<Work>,
    replace: bool,
) -> Result<(), MetaError> {
    // The search already gave the year and a cover; the work is fetched for the description.
    let work = match work {
        Some(w) => Some(w),
        None if !t.has_description => Some(p.work(&hit.id).await?),
        None => None,
    };
    let online = books::OnlineBook {
        work: Some(hit.id.clone()),
        description: work.as_ref().and_then(|w| w.description.clone()),
        year: hit
            .year
            .or_else(|| work.as_ref().and_then(|w| w.year))
            .map(i64::from),
        language: (hit.languages.len() == 1).then(|| hit.languages[0].clone()),
        subjects: work
            .as_ref()
            .map(|w| w.subjects.clone())
            .unwrap_or_default(),
    };
    let id = t.id;
    inner
        .lib
        .write(move |c| books::apply_online(c, id, &online, replace, now_ms()))
        .await
        .map_err(db)?;
    if !t.has_cover {
        let cover_id = hit
            .cover_id
            .or_else(|| work.as_ref().and_then(|w| w.cover_ids.first().copied()));
        if let Some(cid) = cover_id
            && let Some(img) = p.cover(cid).await?
        {
            set_cover(inner, id, Cover::Embedded(img.to_vec())).await;
        }
    }
    Ok(())
}

fn candidates(hits: &[BookHit]) -> Vec<music::Candidate> {
    hits.iter()
        .enumerate()
        .map(|(i, h)| music::Candidate {
            id: h.id.clone(),
            title: h.title.clone(),
            artist: h.authors.join(", "),
            year: h.year,
            // Kept in the provider's order.
            score: 1.0 - i as f64 / 100.0,
            tracks: None,
        })
        .collect()
}

/// The `book_match` job: look one book up.
pub(crate) async fn match_book(inner: &Arc<Inner>, id: i64) -> Result<(), MetaError> {
    let Some(t) = inner
        .lib
        .read(move |c| {
            Ok(if music::online(c)? {
                books::target(c, id)?
            } else {
                None
            })
        })
        .await
        .map_err(db)?
    else {
        return Ok(()); // offline by setting, or the book is gone
    };
    if t.match_state == "matched" {
        return Ok(());
    }
    // Tagged with a MusicBrainz release (Picard): that release, exactly.
    if let Some(rel) = &t.mb_release {
        let m = inner.music();
        match m.release(rel).await {
            Ok(r) => {
                let online = books::OnlineBook {
                    year: r
                        .date
                        .as_deref()
                        .and_then(|d| d.get(..4))
                        .and_then(|y| y.parse().ok()),
                    ..Default::default()
                };
                inner
                    .lib
                    .write(move |c| books::apply_online(c, id, &online, false, now_ms()))
                    .await
                    .map_err(db)?;
                if !t.has_cover
                    && let Some(img) = m.front_cover(&r.id, r.group_id.as_deref()).await?
                {
                    set_cover(inner, id, Cover::Embedded(img.to_vec())).await;
                }
                inner.sink.post(Event::ItemChanged { item_id: id });
                return Ok(());
            }
            Err(MetaError::NotFound) => {} // a stale id: search instead
            Err(e) => return Err(e),
        }
    }
    let p = inner.books();
    let author = t.authors.first().cloned().unwrap_or_default();
    let (hits, by_isbn) = match &t.isbn {
        Some(isbn) => (p.search(BookQuery::Isbn(isbn)).await?, true),
        None => (Vec::new(), false),
    };
    let (hits, by_isbn) = if by_isbn && !hits.is_empty() {
        (hits, true)
    } else {
        let hits = p
            .search(BookQuery::TitleAuthor {
                title: &t.title,
                author: &author,
            })
            .await?;
        (hits, false)
    };
    match decide(&hits, &t.title, &t.authors, by_isbn) {
        BookDecision::Matched(h) => apply_work(inner, &*p, &t, &h, None, false).await?,
        BookDecision::Review(hits) => {
            let cands = candidates(&hits);
            inner
                .lib
                .write(move |c| music::set_review(c, id, &cands))
                .await
                .map_err(db)?;
        }
        BookDecision::None => {
            inner
                .lib
                .write(move |c| music::set_unmatched(c, id))
                .await
                .map_err(db)?;
        }
    }
    inner.sink.post(Event::ItemChanged { item_id: id });
    Ok(())
}

fn meta_err(e: MetaError) -> CoreError {
    CoreError::new(
        match e {
            MetaError::NotFound => ErrorKind::NotFound,
            MetaError::Network(_) | MetaError::RateLimited(_) => ErrorKind::Unreachable,
            _ => ErrorKind::Internal,
        },
        e.to_string(),
    )
}

impl Core {
    /// Use this book provider instead of Open Library (tests).
    pub fn set_book_provider(&self, p: Arc<dyn BookProvider>) {
        *self.inner.books.write().unwrap() = p;
    }

    /// "Search again" on a book (issue #67): works for a title and author (the book's own when
    /// empty), as a `MusicDetail` of kind "bookCandidates". Nothing is sent while online
    /// information is off.
    pub fn search_book(&self, request_id: u64, id: i64, title: &str, author: &str) {
        let (title, author) = (title.trim().to_string(), author.trim().to_string());
        self.spawn(request_id, move |inner| async move {
            let (online, t) = inner
                .lib
                .read(move |c| Ok((music::online(c)?, books::target(c, id)?)))
                .await?;
            let t = t.ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("book {id}")))?;
            if !online {
                return Err(CoreError::new(
                    ErrorKind::Unsupported,
                    "Music and book information online is off.".to_string(),
                ));
            }
            let title = if title.is_empty() { t.title } else { title };
            let author = if author.is_empty() {
                t.authors.first().cloned().unwrap_or_default()
            } else {
                author
            };
            let hits = inner
                .books()
                .search(BookQuery::TitleAuthor {
                    title: &title,
                    author: &author,
                })
                .await
                .map_err(meta_err)?;
            let list: Vec<books::BookCandidate> = hits
                .iter()
                .map(|h| books::BookCandidate {
                    id: h.id.clone(),
                    title: h.title.clone(),
                    authors: h.authors.join(", "),
                    year: h.year.map(i64::from),
                })
                .collect();
            Ok(Event::MusicDetail {
                request_id,
                kind: "bookCandidates".into(),
                json: serde_json::to_string(&json!({ "bookId": id, "candidates": list }))
                    .unwrap_or_default(),
            })
        });
    }

    /// The user chose a work for a book: its details replace what earlier lookups filled (never
    /// local data), and the book page is sent again.
    pub fn choose_book_match(&self, request_id: u64, id: i64, work: &str) {
        let work = work.trim().to_string();
        self.spawn(request_id, move |inner| async move {
            let t = inner
                .lib
                .read(move |c| books::target(c, id))
                .await?
                .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("book {id}")))?;
            let p = inner.books();
            let w = p.work(&work).await.map_err(meta_err)?;
            let hit = BookHit {
                id: w.id.clone(),
                year: w.year,
                cover_id: w.cover_ids.first().copied(),
                ..Default::default()
            };
            apply_work(&inner, &*p, &t, &hit, Some(w), true)
                .await
                .map_err(meta_err)?;
            inner.mark_changed();
            let d = inner
                .lib
                .read(move |c| books::detail(c, id))
                .await?
                .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("book {id}")))?;
            Ok(Event::MusicDetail {
                request_id,
                kind: "book".into(),
                json: serde_json::to_string(&d).unwrap_or_default(),
            })
        });
    }
}
