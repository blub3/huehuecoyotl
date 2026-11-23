//! Matching audiobooks again (2.7, issues #108 to #110): a book's search shows what a match would
//! add field by field; authors get an Open Library match of their own with the photo and
//! biography (decided in issue #110), chosen among authors of one name by the user's books; a
//! book's match brings its authors without one along.

use std::sync::Arc;

use huehuecoyotl_library::{DbError, books, music};
use huehuecoyotl_metadata::MetaError;
use huehuecoyotl_metadata::books::{Author, same_author, same_title};
use huehuecoyotl_metadata::ids::{self, Ref};
use huehuecoyotl_scanner::music::Cover;
use serde_json::{Value, json};

use crate::matching::Outcome;
use crate::{CResult, CoreError, ErrorKind, Event, Inner};

fn db(e: DbError) -> MetaError {
    MetaError::Other(e.to_string())
}

/// At most this many authors of one name are checked against the user's books (one request
/// each), and at most this many of a work's authors are looked up.
const MAX_CHECKS: usize = 3;

/// The author becomes matched; its biography and photo come from Open Library.
async fn apply_author(
    inner: &Arc<Inner>,
    person: i64,
    a: &Author,
    lock: bool,
    via: Option<String>,
) -> Result<(), MetaError> {
    let p = inner.books();
    // The search gives no biography or photo: the author's own record does.
    let full = if a.bio.is_none() && a.photo_ids.is_empty() {
        p.author(&a.id).await.unwrap_or_else(|_| a.clone())
    } else {
        a.clone()
    };
    let (id, bio) = (a.id.clone(), full.bio.clone());
    inner
        .lib
        .write(move |c| {
            books::set_author_match(c, person, &id, lock, via.as_deref())?;
            books::set_author_bio(c, person, bio.as_deref())
        })
        .await
        .map_err(db)?;
    if let Some(photo) = full.photo_ids.first()
        && let Some(img) = p.author_photo(*photo).await?
    {
        crate::worker::write_cover(inner, &inner.local, person, Cover::Embedded(img.to_vec()))
            .await;
    }
    inner.sink.post(Event::ItemChanged { item_id: person });
    Ok(())
}

/// How many of the user's books are among an author's works.
async fn fit(inner: &Arc<Inner>, a: &Author, mine: &[String]) -> Result<usize, MetaError> {
    if mine.is_empty() {
        return Ok(0);
    }
    let works = inner.books().author_works(&a.id).await?;
    Ok(mine
        .iter()
        .filter(|b| works.iter().any(|w| same_title(w, b)))
        .count())
}

/// Issue #110: match an author again: by name; when several authors share it, the one whose
/// works hold the user's books, if exactly one does.
pub(crate) async fn rematch_author(
    inner: &Arc<Inner>,
    person: i64,
) -> Result<(Outcome, i64), MetaError> {
    let t = inner
        .lib
        .read(move |c| {
            Ok(if music::online(c)? {
                books::author_target(c, person)?
            } else {
                None
            })
        })
        .await
        .map_err(db)?
        .ok_or(MetaError::Auth)?;
    let hits = inner.books().search_authors(&t.name).await?;
    let same: Vec<&Author> = hits
        .iter()
        .filter(|a| same_author(&a.name, &t.name))
        .collect();
    let chosen = match same.as_slice() {
        [] => return Ok((Outcome::NotFound, person)),
        [one] => (*one).clone(),
        many => {
            let mut best: Vec<(&Author, usize)> = Vec::new();
            for a in many.iter().take(MAX_CHECKS) {
                best.push((a, fit(inner, a, &t.books).await?));
            }
            best.sort_by_key(|x| std::cmp::Reverse(x.1));
            match best.as_slice() {
                [(a, n), rest @ ..] if *n > 0 && rest.iter().all(|(_, m)| m < n) => (*a).clone(),
                _ => {
                    if t.ol_id
                        .as_ref()
                        .is_some_and(|id| many.iter().any(|a| &a.id == id))
                    {
                        return Ok((Outcome::Unchanged, person));
                    }
                    return Ok((Outcome::Review, person));
                }
            }
        }
    };
    if t.ol_id.as_deref() == Some(chosen.id.as_str()) {
        return Ok((Outcome::Unchanged, person));
    }
    let was = t.ol_id.is_some();
    apply_author(inner, person, &chosen, false, None).await?;
    Ok((
        if was {
            Outcome::Changed
        } else {
            Outcome::Newly
        },
        person,
    ))
}

/// Issue #109: a book's authors without a match take the work's authors of the same name.
/// `author_ids` come with the search result or the work, so no request is made just for them;
/// each author is looked up once (its biography and photo). An author the provider does not know
/// is skipped.
pub(crate) async fn follow_authors(
    inner: &Arc<Inner>,
    book: i64,
    author_ids: &[String],
) -> Result<(), MetaError> {
    if author_ids.is_empty() {
        return Ok(());
    }
    let open = inner
        .lib
        .read(move |c| books::authors_without_match(c, book))
        .await
        .map_err(db)?;
    if open.is_empty() {
        return Ok(());
    }
    let p = inner.books();
    for id in author_ids.iter().take(MAX_CHECKS) {
        let a = match p.author(id).await {
            Ok(a) => a,
            Err(MetaError::NotFound) => continue,
            Err(e) => return Err(e),
        };
        if let Some((person, _)) = open.iter().find(|(_, name)| same_author(name, &a.name)) {
            apply_author(inner, *person, &a, false, Some(format!("book:{book}"))).await?;
        }
    }
    Ok(())
}

fn offline() -> CoreError {
    CoreError::new(
        ErrorKind::Unsupported,
        "Music and book information online is off.".to_string(),
    )
}

/// Issue #110: candidates for an author, with how many of the user's books are theirs.
pub(crate) async fn search_authors(
    inner: &Arc<Inner>,
    person: i64,
    text: &str,
) -> CResult<Vec<Value>> {
    if !inner.lib.read(music::online).await? {
        return Err(offline());
    }
    let t = inner
        .lib
        .read(move |c| books::author_target(c, person))
        .await?
        .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("author {person}")))?;
    let p = inner.books();
    let hits = match ids::parse(text) {
        Some(Ref::OlAuthor(id)) => vec![p.author(&id).await.map_err(crate::books::meta_err)?],
        _ => {
            let name = if text.is_empty() {
                t.name.clone()
            } else {
                text.to_string()
            };
            p.search_authors(&name)
                .await
                .map_err(crate::books::meta_err)?
        }
    };
    let mut out = Vec::new();
    for (i, a) in hits.iter().enumerate() {
        let n = if i < MAX_CHECKS {
            Some(
                fit(inner, a, &t.books)
                    .await
                    .map_err(crate::books::meta_err)?,
            )
        } else {
            None
        };
        out.push(json!({
            "provider": "openlibrary",
            "id": a.id,
            "kind": "author",
            "title": a.name,
            "birth": a.birth,
            "death": a.death,
            "workCount": a.work_count,
            "topWork": a.top_work,
            "books": n,
            "ofBooks": t.books.len(),
            "good": n.is_some_and(|n| n > 0),
            "image": format!("{}/a/olid/{}-M.jpg", crate::matching::OL_COVERS, a.id),
            "current": t.ol_id.as_deref() == Some(a.id.as_str()),
        }));
    }
    Ok(out)
}

/// Issue #110: the user's choice for an author; it is locked.
pub(crate) async fn choose_author(inner: &Arc<Inner>, person: i64, id: &str) -> CResult<i64> {
    let a = inner
        .books()
        .author(id)
        .await
        .map_err(crate::books::meta_err)?;
    apply_author(inner, person, &a, true, None)
        .await
        .map_err(crate::books::meta_err)?;
    inner.mark_changed();
    Ok(person)
}

/// Issue #109: what a work would add to a book, field by field: yours, Open Library's, and which
/// one is shown (your metadata, or a field you corrected, always wins).
pub(crate) async fn preview_book(inner: &Arc<Inner>, book: i64, work: &str) -> CResult<Value> {
    let w = inner
        .books()
        .work(work)
        .await
        .map_err(crate::books::meta_err)?;
    let local = inner
        .lib
        .read(move |c| {
            Ok(c.query_row(
                "SELECT coalesce(i.overview, ''), i.year, coalesce(i.meta_json, '{}'), b.locked,
                        EXISTS (SELECT 1 FROM artwork a WHERE a.item_id = i.id AND a.role = 'poster' AND a.local_path IS NOT NULL)
                 FROM item i JOIN book b ON b.item_id = i.id WHERE i.id = ?1",
                [book],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, bool>(4)?,
                    ))
                },
            )?)
        })
        .await?;
    let (overview, year, meta, locked, has_cover) = local;
    let meta: Value = serde_json::from_str(&meta).unwrap_or_default();
    let online: Vec<String> = meta["online"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let locked: Vec<String> = serde_json::from_str(&locked).unwrap_or_default();
    // A field is the user's when it was corrected, or has a value that no lookup filled.
    let yours = |field: &str, has: bool| {
        locked.iter().any(|l| l == field) || (has && !online.iter().any(|f| f == field))
    };
    let subjects: Vec<String> = meta["subjects"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let short = |s: &str| -> String {
        let mut t: String = s.chars().take(80).collect();
        if s.chars().count() > 80 {
            t.push('…');
        }
        t
    };
    let row = |field: &str, mine: String, theirs: String, keep: bool| {
        json!({
            "field": field,
            "yours": mine,
            "theirs": theirs,
            "shown": if keep || theirs.is_empty() { "yours" } else { "openLibrary" },
        })
    };
    let rows = vec![
        row(
            "description",
            short(&overview),
            short(w.description.as_deref().unwrap_or_default()),
            yours("description", !overview.is_empty()),
        ),
        row(
            "year",
            year.map(|y| y.to_string()).unwrap_or_default(),
            w.year.map(|y| y.to_string()).unwrap_or_default(),
            yours("year", year.is_some()),
        ),
        row(
            "subjects",
            subjects
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
            w.subjects
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
            yours("subjects", !subjects.is_empty()),
        ),
        row(
            "cover",
            if has_cover {
                "✓".into()
            } else {
                String::new()
            },
            if w.cover_ids.is_empty() {
                String::new()
            } else {
                "✓".into()
            },
            has_cover,
        ),
    ];
    Ok(json!({ "itemId": book, "change": "none", "fields": rows }))
}
