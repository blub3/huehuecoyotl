//! Audiobooks (2.5, SPEC 7 "Audiobook library"): books, the people who wrote and read them,
//! series, the files that make a book's timeline, and its chapters. The scanner hands a book over
//! as a `BookInput`; this module writes it and answers the book page. The timeline arithmetic
//! (a position on the book ↔ a file and an offset in it) and the chapter lookup are plain
//! functions, unit-tested without a database.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;
use crate::text::{normalize, sort_title};

/// One file of a book, in timeline order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileInput {
    pub file_id: i64,
    pub duration_ms: i64,
    /// Its title tag or file name: the chapter name when the book has no embedded chapters.
    pub title: String,
}

/// A chapter on the book's timeline.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// Everything a book is made from, after the local metadata sources were merged.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BookInput {
    pub key: String,
    pub title: String,
    pub authors: Vec<String>,
    pub narrators: Vec<String>,
    pub series: Option<String>,
    pub series_position: Option<f64>,
    pub publisher: Option<String>,
    pub published: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub asin: Option<String>,
    pub description: Option<String>,
    /// Issue #67: a MusicBrainz release id from the tags (Picard), for an exact lookup.
    pub mb_release: Option<String>,
    pub drm: bool,
    pub files: Vec<FileInput>,
    /// Embedded chapters on the timeline, when known; else every file is a chapter.
    pub chapters: Vec<Chapter>,
}

impl BookInput {
    /// The year a `published` date starts with.
    pub fn year(&self) -> Option<i64> {
        self.published
            .as_deref()
            .and_then(|d| d.get(..4))
            .and_then(|y| y.parse().ok())
    }
}

// ---- timeline -------------------------------------------------------------------------------

/// Where `position` (ms on the book) falls: (index of the file, offset in it). Past the end:
/// the end of the last file.
pub fn locate(durations: &[i64], position: i64) -> (usize, i64) {
    let mut start = 0;
    for (i, d) in durations.iter().enumerate() {
        if position < start + d || i + 1 == durations.len() {
            return (i, (position - start).clamp(0, *d));
        }
        start += d;
    }
    (0, 0)
}

/// The position on the book of an offset in file `index`.
pub fn position_of(durations: &[i64], index: usize, offset: i64) -> i64 {
    durations.iter().take(index).sum::<i64>() + offset.max(0)
}

/// The chapter that holds `position` (the last one that starts at or before it).
pub fn chapter_at(chapters: &[Chapter], position: i64) -> Option<usize> {
    if chapters.is_empty() {
        return None;
    }
    let i = chapters.partition_point(|c| c.start_ms <= position);
    Some(i.saturating_sub(1))
}

/// Issue #66: where "previous chapter" goes: the start of the current chapter, or the one
/// before when within 3 s of the current one's start. `starts` are the chapters' start times.
pub fn previous_chapter(starts: &[i64], position: i64) -> i64 {
    let i = starts.partition_point(|s| *s <= position).saturating_sub(1);
    match starts.get(i) {
        Some(s) if position - s < 3_000 && i > 0 => starts[i - 1],
        Some(s) => *s,
        None => 0,
    }
}

/// Where "next chapter" goes; `None` in the last chapter.
pub fn next_chapter(starts: &[i64], position: i64) -> Option<i64> {
    let i = starts.partition_point(|s| *s <= position);
    starts.get(i).copied()
}

/// Issue #66: the sleep timer's stop point for "end of chapter": the end of the chapter that
/// holds `position`, or the end of the book.
pub fn end_of_chapter(starts: &[i64], duration: i64, position: i64) -> i64 {
    next_chapter(starts, position).unwrap_or(duration)
}

/// The last 30 s of a book count as finished (SPEC 5.8).
pub const FINISHED_WITHIN_MS: i64 = 30_000;

/// Save where a book is: its position on the timeline; finished in its last 30 s. A later
/// position in an unfinished book never comes from an earlier save (saves arrive in order).
pub fn save_progress(c: &Connection, book: i64, position: i64, now: i64) -> Result<bool> {
    let duration: i64 = c
        .query_row(
            "SELECT duration_ms FROM book WHERE item_id = ?1",
            [book],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);
    let finished = duration > 0 && position + FINISHED_WITHIN_MS >= duration;
    c.execute(
        "INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at, last_played_at)
         VALUES (?1, 1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(item_id, profile_id) DO UPDATE SET position_ms = ?2, duration_ms = ?3,
           watched = CASE WHEN ?4 THEN 1 ELSE 0 END, updated_at = ?5, last_played_at = ?5",
        params![book, position.clamp(0, duration.max(position)), duration, finished, now],
    )?;
    Ok(finished)
}

/// "Mark as finished" (or unfinished); the position stays until the book is played again.
pub fn mark_finished(c: &Connection, book: i64, finished: bool, now: i64) -> Result<()> {
    c.execute(
        "INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at)
         VALUES (?1, 1, 0, (SELECT duration_ms FROM book WHERE item_id = ?1), ?2, ?3)
         ON CONFLICT(item_id, profile_id) DO UPDATE SET watched = ?2, updated_at = ?3",
        params![book, finished, now],
    )?;
    Ok(())
}

/// The speed chosen for a book (0.5 to 3.0); `None` uses the default from Settings.
pub fn set_speed(c: &Connection, book: i64, speed: Option<f64>) -> Result<()> {
    let speed = speed.map(|s| (s.clamp(0.5, 3.0) * 10.0).round() / 10.0);
    c.execute(
        "UPDATE book SET speed = ?2 WHERE item_id = ?1",
        params![book, speed],
    )?;
    Ok(())
}

/// Where a book starts: its saved position, or the beginning when it was finished.
pub fn start_position(c: &Connection, book: i64) -> Result<i64> {
    Ok(c
        .query_row(
            "SELECT CASE WHEN watched THEN 0 ELSE position_ms END FROM playstate WHERE item_id = ?1 AND profile_id = 1",
            [book],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

/// "Continue series": the series' books in order with (finished, last played), then the pick.
pub fn continue_in_series(c: &Connection, series: i64) -> Result<Option<i64>> {
    let mut st = c.prepare(
        "SELECT i.id, coalesce(p.watched, 0), p.last_played_at FROM item i JOIN book b ON b.item_id = i.id
         LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = 1
         WHERE i.parent_id = ?1 AND i.kind = 'audiobook'
           AND EXISTS (SELECT 1 FROM book_file bf WHERE bf.book_id = i.id)
         ORDER BY coalesce(b.series_position, 1e9), i.year, i.sort_title",
    )?;
    let rows: Vec<(i64, bool, Option<i64>)> = st
        .query_map([series], |r| {
            Ok((r.get(0)?, r.get::<_, i64>(1)? != 0, r.get(2)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(continue_series(&rows))
}

/// Every file as a chapter (books without embedded chapters).
pub fn file_chapters(files: &[FileInput]) -> Vec<Chapter> {
    let mut start = 0;
    files
        .iter()
        .map(|f| {
            let c = Chapter {
                title: f.title.clone(),
                start_ms: start,
                end_ms: start + f.duration_ms,
            };
            start += f.duration_ms;
            c
        })
        .collect()
}

// ---- writing --------------------------------------------------------------------------------

fn insert_item(
    c: &Connection,
    kind: &str,
    title: &str,
    year: Option<i64>,
    key: &str,
    now: i64,
) -> Result<i64> {
    c.execute(
        "INSERT INTO item(kind, title, sort_title, year, match_state, added_at, group_key)
         VALUES (?1, ?2, ?3, ?4, 'unmatched', ?5, ?6)",
        params![kind, title, sort_title(title), year, now, key],
    )?;
    let id = c.last_insert_rowid();
    c.execute(
        "INSERT INTO item_fts(rowid, title) VALUES (?1, ?2)",
        params![id, title],
    )?;
    Ok(id)
}

/// The person item for a name (authors and narrators share them).
pub fn person(c: &Connection, name: &str, now: i64) -> Result<i64> {
    let key = format!("person:{}", normalize(name));
    match c
        .query_row(
            "SELECT id FROM item WHERE kind = 'person' AND group_key = ?1",
            [&key],
            |r| r.get(0),
        )
        .optional()?
    {
        Some(id) => Ok(id),
        None => insert_item(c, "person", name.trim(), None, &key, now),
    }
}

fn series(c: &Connection, name: &str, author: &str, now: i64) -> Result<i64> {
    let key = format!("series:{}|{}", normalize(author), normalize(name));
    match c
        .query_row(
            "SELECT id FROM item WHERE kind = 'book_series' AND group_key = ?1",
            [&key],
            |r| r.get(0),
        )
        .optional()?
    {
        Some(id) => Ok(id),
        None => insert_item(c, "book_series", name.trim(), None, &key, now),
    }
}

/// Write a book (new, or again after its files or metadata changed). Fields the user corrected
/// (`book.locked`) are kept. Returns (book, whether it is new).
pub fn upsert(c: &Connection, b: &BookInput, now: i64) -> Result<(i64, bool)> {
    let found: Option<(i64, String)> = c
        .query_row(
            "SELECT item_id, locked FROM book WHERE key = ?1",
            [&b.key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let author = b.authors.first().cloned().unwrap_or_default();
    let group = format!("book:{}|{}", normalize(&author), normalize(&b.title));
    let (id, created, locked) = match found {
        Some((id, locked)) => (id, false, locked),
        None => {
            let id = insert_item(c, "audiobook", &b.title, b.year(), &group, now)?;
            c.execute(
                "INSERT INTO book(item_id, key) VALUES (?1, ?2)",
                params![id, b.key],
            )?;
            (id, true, "[]".to_string())
        }
    };
    let locked: Vec<String> = serde_json::from_str(&locked).unwrap_or_default();
    let free = |f: &str| !locked.iter().any(|l| l == f);
    if free("title") {
        c.execute(
            "UPDATE item SET title = ?2, sort_title = ?3, group_key = ?4 WHERE id = ?1",
            params![id, b.title, sort_title(&b.title), group],
        )?;
        c.execute(
            "UPDATE item_fts SET title = ?2 WHERE rowid = ?1",
            params![id, b.title],
        )?;
    }
    if free("year") {
        c.execute(
            "UPDATE item SET year = coalesce(?2, year) WHERE id = ?1",
            params![id, b.year()],
        )?;
    }
    if free("description") {
        c.execute(
            "UPDATE item SET overview = coalesce(?2, overview) WHERE id = ?1",
            params![id, b.description],
        )?;
    }
    let duration: i64 = b.files.iter().map(|f| f.duration_ms).sum();
    c.execute(
        "UPDATE book SET publisher = coalesce(?2, publisher), published = coalesce(?3, published),
                         language = coalesce(?4, language), isbn = coalesce(?5, isbn), asin = coalesce(?6, asin),
                         duration_ms = ?7, drm = ?8
         WHERE item_id = ?1",
        params![id, b.publisher, b.published, b.language, b.isbn, b.asin, duration, b.drm],
    )?;
    c.execute(
        "DELETE FROM external_id WHERE item_id = ?1 AND provider = 'musicbrainz' AND kind = 'release'",
        [id],
    )?;
    if let Some(r) = b.mb_release.as_deref().filter(|r| !r.trim().is_empty()) {
        c.execute(
            "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value) VALUES (?1, 'musicbrainz', 'release', ?2)",
            params![id, r.trim()],
        )?;
    }
    // People.
    if free("authors") {
        set_credits(c, id, "author", &b.authors, now)?;
    }
    if free("narrators") {
        set_credits(c, id, "narrator", &b.narrators, now)?;
    }
    // Series.
    if free("series") {
        let parent = match &b.series {
            Some(s) if !s.trim().is_empty() => Some(series(c, s, &author, now)?),
            _ => None,
        };
        c.execute(
            "UPDATE item SET parent_id = ?2 WHERE id = ?1",
            params![id, parent],
        )?;
        c.execute(
            "UPDATE book SET series_position = ?2 WHERE item_id = ?1",
            params![id, b.series_position],
        )?;
    }
    // The timeline and the chapters.
    c.execute("DELETE FROM book_file WHERE book_id = ?1", [id])?;
    let mut start = 0;
    for (i, f) in b.files.iter().enumerate() {
        c.execute(
            "INSERT OR REPLACE INTO book_file(file_id, book_id, idx, start_ms, duration_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![f.file_id, id, i as i64, start, f.duration_ms],
        )?;
        c.execute(
            "UPDATE file SET item_id = ?2 WHERE id = ?1",
            params![f.file_id, id],
        )?;
        start += f.duration_ms;
    }
    let chapters = if b.chapters.is_empty() {
        file_chapters(&b.files)
    } else {
        b.chapters.clone()
    };
    set_chapters(c, id, &chapters)?;
    Ok((id, created))
}

fn set_credits(c: &Connection, book: i64, role: &str, names: &[String], now: i64) -> Result<()> {
    c.execute(
        "DELETE FROM book_credit WHERE book_id = ?1 AND role = ?2",
        params![book, role],
    )?;
    let mut seen = Vec::new();
    for name in names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        let p = person(c, name, now)?;
        if seen.contains(&p) {
            continue;
        }
        seen.push(p);
        c.execute(
            "INSERT INTO book_credit(book_id, person_id, role, position) VALUES (?1, ?2, ?3, ?4)",
            params![book, p, role, (seen.len() - 1) as i64],
        )?;
    }
    Ok(())
}

/// Replace a book's chapters (the file chapters, or the embedded ones from the probe). The
/// file of each chapter is the one its start falls into.
pub fn set_chapters(c: &Connection, book: i64, chapters: &[Chapter]) -> Result<()> {
    let files: Vec<(i64, i64)> = {
        let mut st = c.prepare_cached(
            "SELECT file_id, duration_ms FROM book_file WHERE book_id = ?1 ORDER BY idx",
        )?;
        st.query_map([book], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let durations: Vec<i64> = files.iter().map(|f| f.1).collect();
    c.execute("DELETE FROM chapter WHERE book_id = ?1", [book])?;
    for (i, ch) in chapters.iter().enumerate() {
        let file = if files.is_empty() {
            None
        } else {
            Some(files[locate(&durations, ch.start_ms).0].0)
        };
        c.execute(
            "INSERT INTO chapter(book_id, idx, title, start_ms, end_ms, file_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![book, i as i64, ch.title, ch.start_ms, ch.end_ms, file],
        )?;
    }
    Ok(())
}

/// A file's embedded chapters from the probe (issue #65): they replace the file's part of its
/// book's chapters, moved onto the book's timeline. Returns the book when it changed.
pub fn apply_probe_chapters(
    c: &Connection,
    file_id: i64,
    embedded: &[Chapter],
) -> Result<Option<i64>> {
    if embedded.is_empty() {
        return Ok(None);
    }
    let Some((book, start, duration)): Option<(i64, i64, i64)> = c
        .query_row(
            "SELECT book_id, start_ms, duration_ms FROM book_file WHERE file_id = ?1",
            [file_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let end = start + duration;
    // The chapters of the other files stay; this file's part is replaced.
    let mut all: Vec<Chapter> = chapters(c, book)?
        .into_iter()
        .filter(|ch| ch.start_ms < start || ch.start_ms >= end)
        .collect();
    // Chapters that start past the file's end (a broken or cut file) are left out; a chapter
    // never ends before it starts.
    let limit = if duration > 0 { end } else { i64::MAX };
    all.extend(
        embedded
            .iter()
            .filter(|ch| start + ch.start_ms < limit)
            .map(|ch| Chapter {
                title: ch.title.clone(),
                start_ms: start + ch.start_ms,
                end_ms: (start + ch.end_ms).min(limit).max(start + ch.start_ms),
            }),
    );
    all.sort_by_key(|ch| ch.start_ms);
    set_chapters(c, book, &all)?;
    Ok(Some(book))
}

// ---- online information (issue #67) --------------------------------------------------------

/// A book is looked up again after a week when nothing was found or it waits for review.
const RETRY_AFTER_MS: i64 = 7 * 24 * 3600 * 1000;

/// What a lookup needs to know about a book.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BookTarget {
    pub id: i64,
    pub title: String,
    pub authors: Vec<String>,
    pub isbn: Option<String>,
    pub mb_release: Option<String>,
    /// The Open Library work it is matched to.
    pub work: Option<String>,
    pub match_state: String,
    pub has_cover: bool,
    pub has_description: bool,
    pub has_year: bool,
    pub has_language: bool,
}

fn external(c: &Connection, item: i64, provider: &str, kind: &str) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT value FROM external_id WHERE item_id = ?1 AND provider = ?2 AND kind = ?3",
        params![item, provider, kind],
        |r| r.get(0),
    )
    .optional()?)
}

pub fn target(c: &Connection, id: i64) -> Result<Option<BookTarget>> {
    let row = c
        .query_row(
            "SELECT coalesce(i.title, ''), b.isbn, coalesce(i.match_state, 'unmatched'),
                    EXISTS (SELECT 1 FROM artwork w WHERE w.item_id = i.id AND w.role = 'poster_grid' AND w.local_path IS NOT NULL),
                    coalesce(i.overview, '') <> '', i.year IS NOT NULL, coalesce(b.language, '') <> ''
             FROM item i JOIN book b ON b.item_id = i.id WHERE i.id = ?1 AND i.kind = 'audiobook'",
            [id],
            |r| {
                Ok(BookTarget {
                    id,
                    title: r.get(0)?,
                    isbn: r.get::<_, Option<String>>(1)?.filter(|s| !s.trim().is_empty()),
                    match_state: r.get(2)?,
                    has_cover: r.get(3)?,
                    has_description: r.get(4)?,
                    has_year: r.get(5)?,
                    has_language: r.get(6)?,
                    ..Default::default()
                })
            },
        )
        .optional()?;
    let Some(mut t) = row else {
        return Ok(None);
    };
    t.authors = people(c, id, "author")?
        .into_iter()
        .map(|p| p.name)
        .collect();
    t.mb_release = external(c, id, "musicbrainz", "release")?;
    t.work = external(c, id, "openlibrary", "work")?;
    Ok(Some(t))
}

/// Books to look up: never looked up, or not found or waiting for review and not tried for a
/// week. Books with copy protection too: their pages show the same information.
pub fn due(c: &Connection, now: i64) -> Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT id FROM item WHERE kind = 'audiobook' AND locked = 0
           AND coalesce(match_state, 'unmatched') IN ('unmatched', 'needs_review')
           AND (meta_fetched_at IS NULL OR meta_fetched_at < ?1)",
    )?;
    let rows = st.query_map([now - RETRY_AFTER_MS], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// What a lookup found for a book.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnlineBook {
    pub work: Option<String>,
    pub description: Option<String>,
    pub year: Option<i64>,
    /// ISO 639 code.
    pub language: Option<String>,
    pub subjects: Vec<String>,
}

/// Fill what the local metadata left empty (and the user did not correct) from a lookup, and mark
/// the book matched. Fields filled online are remembered in `meta_json.online`, so a match the
/// user chooses later (`replace`) replaces them; a local value is never replaced.
pub fn apply_online(
    c: &Connection,
    id: i64,
    o: &OnlineBook,
    replace: bool,
    now: i64,
) -> Result<()> {
    let (locked, meta): (String, Option<String>) = c.query_row(
        "SELECT b.locked, i.meta_json FROM book b JOIN item i ON i.id = b.item_id WHERE b.item_id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let locked: Vec<String> = serde_json::from_str(&locked).unwrap_or_default();
    let mut meta: serde_json::Value = meta
        .and_then(|m| serde_json::from_str(&m).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    let mut online: Vec<String> = meta["online"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    // A field may be written when the user did not set it and it is empty, or it came from an
    // earlier lookup that the user's choice replaces.
    let may = |field: &str, empty: bool| {
        !locked.iter().any(|l| l == field)
            && (empty || (replace && online.iter().any(|f| f == field)))
    };
    let (no_desc, no_year, no_lang): (bool, bool, bool) = c.query_row(
        "SELECT coalesce(i.overview, '') = '', i.year IS NULL, coalesce(b.language, '') = ''
         FROM item i JOIN book b ON b.item_id = i.id WHERE i.id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let mut filled = Vec::new();
    if let Some(d) = o.description.as_deref().filter(|d| !d.trim().is_empty())
        && may("description", no_desc)
    {
        c.execute(
            "UPDATE item SET overview = ?2 WHERE id = ?1",
            params![id, d],
        )?;
        filled.push("description");
    }
    if let Some(y) = o.year
        && may("year", no_year)
    {
        c.execute("UPDATE item SET year = ?2 WHERE id = ?1", params![id, y])?;
        filled.push("year");
    }
    if let Some(l) = o.language.as_deref().filter(|l| !l.is_empty())
        && may("language", no_lang)
    {
        c.execute(
            "UPDATE book SET language = ?2 WHERE item_id = ?1",
            params![id, l],
        )?;
        filled.push("language");
    }
    if replace {
        online.retain(|f| {
            filled.contains(&f.as_str())
                || !["description", "year", "language"].contains(&f.as_str())
        });
    }
    for f in filled {
        if !online.iter().any(|x| x == f) {
            online.push(f.to_string());
        }
    }
    meta["online"] = serde_json::json!(online);
    if !o.subjects.is_empty() {
        meta["subjects"] = serde_json::json!(o.subjects);
    }
    c.execute(
        "UPDATE item SET meta_json = ?2, match_state = 'matched', meta_fetched_at = ?3 WHERE id = ?1",
        params![id, meta.to_string(), now],
    )?;
    c.execute(
        "DELETE FROM external_id WHERE item_id = ?1 AND provider = 'openlibrary'",
        [id],
    )?;
    if let Some(w) = &o.work {
        c.execute(
            "INSERT OR IGNORE INTO external_id(item_id, provider, kind, value) VALUES (?1, 'openlibrary', 'work', ?2)",
            params![id, w],
        )?;
    }
    c.execute("DELETE FROM match_candidate WHERE item_id = ?1", [id])?;
    Ok(())
}

/// One stored candidate for a book in review: the work id, title, authors and year.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookCandidate {
    pub id: String,
    pub title: String,
    pub authors: String,
    pub year: Option<i64>,
}

pub fn candidates(c: &Connection, id: i64) -> Result<Vec<BookCandidate>> {
    let mut st = c.prepare(
        "SELECT CAST(provider_id AS TEXT), coalesce(title, ''), coalesce(overview, ''), year
         FROM match_candidate WHERE item_id = ?1 ORDER BY score DESC, rowid",
    )?;
    let rows = st.query_map([id], |r| {
        Ok(BookCandidate {
            id: r.get(0)?,
            title: r.get(1)?,
            authors: r.get(2)?,
            year: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Books, people and series no file needs any more.
pub fn prune(c: &Connection) -> Result<()> {
    c.execute_batch(
        "DELETE FROM item WHERE kind = 'audiobook' AND NOT EXISTS (SELECT 1 FROM book_file f WHERE f.book_id = item.id);
         DELETE FROM item WHERE kind = 'book_series' AND NOT EXISTS (SELECT 1 FROM item b WHERE b.parent_id = item.id);
         DELETE FROM item WHERE kind = 'person' AND NOT EXISTS (SELECT 1 FROM book_credit c WHERE c.person_id = item.id);
         DELETE FROM item_fts WHERE rowid NOT IN (SELECT id FROM item);",
    )?;
    Ok(())
}

// ---- reading --------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookFile {
    pub file_id: i64,
    pub path: String,
    pub start_ms: i64,
    pub duration_ms: i64,
}

/// A book's page.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookDetail {
    pub id: i64,
    pub title: String,
    pub authors: Vec<Person>,
    pub narrators: Vec<Person>,
    pub series_id: i64,
    pub series: String,
    pub series_position: Option<f64>,
    pub year: Option<i64>,
    pub publisher: String,
    pub language: String,
    pub isbn: String,
    pub description: String,
    pub duration_ms: i64,
    pub position_ms: i64,
    pub finished: bool,
    pub speed: Option<f64>,
    pub cover_ratio: Option<f64>,
    pub drm: bool,
    pub poster: String,
    pub poster_full: String,
    pub chapters: Vec<Chapter>,
    pub files: Vec<BookFile>,
    /// Issue #67: "matched", "unmatched" or "needs_review" (then "Search again" offers
    /// `candidates`).
    pub match_state: String,
    pub subjects: Vec<String>,
    pub candidates: Vec<BookCandidate>,
}

fn people(c: &Connection, book: i64, role: &str) -> Result<Vec<Person>> {
    let mut st = c.prepare_cached(
        "SELECT p.id, coalesce(p.title, '') FROM book_credit bc JOIN item p ON p.id = bc.person_id
         WHERE bc.book_id = ?1 AND bc.role = ?2 ORDER BY bc.position",
    )?;
    Ok(st
        .query_map(params![book, role], |r| {
            Ok(Person {
                id: r.get(0)?,
                name: r.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

pub fn chapters(c: &Connection, book: i64) -> Result<Vec<Chapter>> {
    let mut st = c.prepare_cached(
        "SELECT title, start_ms, end_ms FROM chapter WHERE book_id = ?1 ORDER BY idx",
    )?;
    Ok(st
        .query_map([book], |r| {
            Ok(Chapter {
                title: r.get(0)?,
                start_ms: r.get(1)?,
                end_ms: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

pub fn files(c: &Connection, book: i64) -> Result<Vec<BookFile>> {
    let mut st = c.prepare_cached(
        "SELECT bf.file_id, f.path, bf.start_ms, bf.duration_ms FROM book_file bf JOIN file f ON f.id = bf.file_id
         WHERE bf.book_id = ?1 ORDER BY bf.idx",
    )?;
    Ok(st
        .query_map([book], |r| {
            Ok(BookFile {
                file_id: r.get(0)?,
                path: r.get(1)?,
                start_ms: r.get(2)?,
                duration_ms: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

fn art(c: &Connection, item: i64, role: &str) -> Result<String> {
    Ok(c.query_row(
        "SELECT local_path FROM artwork WHERE item_id = ?1 AND role = ?2 AND local_path IS NOT NULL",
        params![item, role],
        |r| r.get(0),
    )
    .optional()?
    .unwrap_or_default())
}

pub fn detail(c: &Connection, book: i64) -> Result<Option<BookDetail>> {
    let d = c
        .query_row(
            "SELECT coalesce(i.title, ''), coalesce(i.parent_id, 0), coalesce(s.title, ''), b.series_position, i.year,
                    coalesce(b.publisher, ''), coalesce(b.language, ''), coalesce(b.isbn, ''), coalesce(i.overview, ''),
                    b.duration_ms, coalesce(p.position_ms, 0), coalesce(p.watched, 0), b.speed, b.cover_ratio, b.drm,
                    coalesce(i.match_state, 'unmatched'), i.meta_json
             FROM item i JOIN book b ON b.item_id = i.id
             LEFT JOIN item s ON s.id = i.parent_id
             LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = 1
             WHERE i.id = ?1",
            [book],
            |r| {
                Ok(BookDetail {
                    id: book,
                    title: r.get(0)?,
                    series_id: r.get(1)?,
                    series: r.get(2)?,
                    series_position: r.get(3)?,
                    year: r.get(4)?,
                    publisher: r.get(5)?,
                    language: r.get(6)?,
                    isbn: r.get(7)?,
                    description: r.get(8)?,
                    duration_ms: r.get(9)?,
                    position_ms: r.get(10)?,
                    finished: r.get::<_, i64>(11)? != 0,
                    speed: r.get(12)?,
                    cover_ratio: r.get(13)?,
                    drm: r.get::<_, i64>(14)? != 0,
                    match_state: r.get(15)?,
                    subjects: r
                        .get::<_, Option<String>>(16)?
                        .and_then(|m| serde_json::from_str::<serde_json::Value>(&m).ok())
                        .and_then(|m| serde_json::from_value(m["subjects"].clone()).ok())
                        .unwrap_or_default(),
                    ..Default::default()
                })
            },
        )
        .optional()?;
    let Some(mut d) = d else { return Ok(None) };
    d.authors = people(c, book, "author")?;
    d.narrators = people(c, book, "narrator")?;
    d.chapters = chapters(c, book)?;
    d.files = files(c, book)?;
    d.poster = art(c, book, "poster_grid")?;
    d.poster_full = art(c, book, "poster")?;
    if d.match_state == "needs_review" {
        d.candidates = candidates(c, book)?;
    }
    Ok(Some(d))
}

/// A book listed on a series page, an author's or a narrator's page.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookCard {
    pub id: i64,
    pub title: String,
    pub series_position: Option<f64>,
    pub year: Option<i64>,
    pub duration_ms: i64,
    pub position_ms: i64,
    pub finished: bool,
    pub cover_ratio: Option<f64>,
    pub poster: String,
    pub poster_large: String,
}

fn cards(c: &Connection, sql: &str, id: i64) -> Result<Vec<BookCard>> {
    let mut st = c.prepare(sql)?;
    let rows = st.query_map([id], |r| {
        Ok(BookCard {
            id: r.get(0)?,
            title: r.get(1)?,
            series_position: r.get(2)?,
            year: r.get(3)?,
            duration_ms: r.get(4)?,
            position_ms: r.get(5)?,
            finished: r.get::<_, i64>(6)? != 0,
            cover_ratio: r.get(7)?,
            ..Default::default()
        })
    })?;
    let mut out: Vec<BookCard> = rows.collect::<rusqlite::Result<_>>()?;
    for b in &mut out {
        b.poster = art(c, b.id, "poster_grid")?;
        b.poster_large = art(c, b.id, "poster_grid_l")?;
    }
    Ok(out)
}

const CARD_COLS: &str = "i.id, coalesce(i.title, ''), b.series_position, i.year, b.duration_ms,
    coalesce(p.position_ms, 0), coalesce(p.watched, 0), b.cover_ratio";

/// A series' books in order.
pub fn series_books(c: &Connection, series: i64) -> Result<Vec<BookCard>> {
    cards(
        c,
        &format!(
            "SELECT {CARD_COLS} FROM item i JOIN book b ON b.item_id = i.id
             LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = 1
             WHERE i.parent_id = ?1 AND i.kind = 'audiobook'
             ORDER BY coalesce(b.series_position, 1e9), i.year, i.sort_title"
        ),
        series,
    )
}

/// A person's books (as author or narrator), by series and year.
pub fn person_books(c: &Connection, person: i64) -> Result<Vec<BookCard>> {
    cards(
        c,
        &format!(
            "SELECT DISTINCT {CARD_COLS} FROM item i JOIN book b ON b.item_id = i.id
             JOIN book_credit bc ON bc.book_id = i.id AND bc.person_id = ?1
             LEFT JOIN playstate p ON p.item_id = i.id AND p.profile_id = 1
             ORDER BY i.parent_id, coalesce(b.series_position, 1e9), i.year, i.sort_title"
        ),
        person,
    )
}

// ---- authors (2.7, issues #109 and #110) ------------------------------------------------------

/// What an author's match needs: the name, the Open Library id, the lock and the titles of the
/// person's books as author.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AuthorTarget {
    pub id: i64,
    pub name: String,
    pub ol_id: Option<String>,
    pub locked: bool,
    pub books: Vec<String>,
}

pub fn author_target(c: &Connection, person: i64) -> Result<Option<AuthorTarget>> {
    let row = c
        .query_row(
            "SELECT coalesce(title, ''), locked FROM item WHERE id = ?1 AND kind = 'person'",
            [person],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? != 0)),
        )
        .optional()?;
    let Some((name, locked)) = row else {
        return Ok(None);
    };
    let mut st = c.prepare(
        "SELECT DISTINCT coalesce(i.title, '') FROM book_credit bc JOIN item i ON i.id = bc.book_id
         WHERE bc.person_id = ?1 AND bc.role = 'author' ORDER BY 1",
    )?;
    let books = st
        .query_map([person], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(AuthorTarget {
        id: person,
        name,
        ol_id: external(c, person, "openlibrary", "author")?,
        locked,
        books,
    }))
}

/// The authors of a book that have no Open Library match and are not locked: (person, name).
pub fn authors_without_match(c: &Connection, book: i64) -> Result<Vec<(i64, String)>> {
    let mut st = c.prepare(
        "SELECT p.id, coalesce(p.title, '') FROM book_credit bc JOIN item p ON p.id = bc.person_id
         WHERE bc.book_id = ?1 AND bc.role = 'author' AND p.locked = 0
           AND NOT EXISTS (SELECT 1 FROM external_id e WHERE e.item_id = p.id AND e.provider = 'openlibrary' AND e.kind = 'author')
         ORDER BY bc.position",
    )?;
    Ok(st
        .query_map([book], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

/// Issue #110: an author's Open Library match (replacing one it had), locked when the user chose
/// it; `via` ("book:12") when it came through a book.
pub fn set_author_match(
    c: &Connection,
    person: i64,
    ol_id: &str,
    lock: bool,
    via: Option<&str>,
) -> Result<()> {
    crate::matching::set_external(c, person, "openlibrary", "author", Some(ol_id))?;
    crate::matching::set_matched_via(c, person, via)?;
    c.execute(
        "UPDATE item SET locked = ?2, match_state = 'matched' WHERE id = ?1",
        params![person, lock as i64],
    )?;
    Ok(())
}

/// Issue #110: an author's biography from Open Library: kept unless the person has one of its own
/// (one filled online is replaced; `meta_json.onlineBio` says which).
pub fn set_author_bio(c: &Connection, person: i64, bio: Option<&str>) -> Result<()> {
    let (overview, meta): (String, Option<String>) = c.query_row(
        "SELECT coalesce(overview, ''), meta_json FROM item WHERE id = ?1",
        [person],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut m: serde_json::Value = meta
        .and_then(|m| serde_json::from_str(&m).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    let online = m["onlineBio"].as_bool().unwrap_or(false);
    if !overview.is_empty() && !online {
        return Ok(());
    }
    m["onlineBio"] = serde_json::json!(bio.is_some());
    c.execute(
        "UPDATE item SET overview = ?2, meta_json = ?3 WHERE id = ?1",
        params![person, bio, m.to_string()],
    )?;
    Ok(())
}

/// Issue #110: what the person page adds about the match: matched, locked, through which book,
/// the biography and the photo.
pub fn author_info(c: &Connection, person: i64) -> Result<serde_json::Value> {
    let (overview, locked): (String, i64) = c.query_row(
        "SELECT coalesce(overview, ''), locked FROM item WHERE id = ?1",
        [person],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let via = crate::matching::external(c, person, "coyotl", "matched_via")?
        .and_then(|v| v.strip_prefix("book:").and_then(|n| n.parse::<i64>().ok()))
        .and_then(|id| {
            c.query_row("SELECT title FROM item WHERE id = ?1", [id], |r| {
                r.get::<_, String>(0)
            })
            .ok()
            .map(|t| serde_json::json!({ "kind": "book", "id": id, "title": t }))
        });
    let photo: Option<String> = c
        .query_row(
            "SELECT local_path FROM artwork WHERE item_id = ?1 AND role = 'poster' AND local_path IS NOT NULL",
            [person],
            |r| r.get(0),
        )
        .optional()?;
    Ok(serde_json::json!({
        "matched": external(c, person, "openlibrary", "author")?.is_some(),
        "locked": locked != 0,
        "matchedVia": via,
        "biography": overview,
        "image": photo.unwrap_or_default(),
    }))
}

/// "Continue series": the first unfinished book after the last one listened to, else the first
/// unfinished one.
pub fn continue_series(books: &[(i64, bool, Option<i64>)]) -> Option<i64> {
    // (book, finished, last played) in series order
    let last = books
        .iter()
        .enumerate()
        .filter(|(_, b)| b.2.is_some())
        .max_by_key(|(_, b)| b.2)
        .map(|(i, _)| i);
    let from = match last {
        Some(i) if !books[i].1 => return Some(books[i].0),
        Some(i) => i + 1,
        None => 0,
    };
    books[from..]
        .iter()
        .chain(books[..from].iter())
        .find(|b| !b.1)
        .map(|b| b.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_navigation_and_sleep() {
        let starts = [0, 60_000, 120_000];
        // Previous: the chapter's start, or the one before within 3 s of it.
        assert_eq!(previous_chapter(&starts, 90_000), 60_000);
        assert_eq!(previous_chapter(&starts, 61_000), 0);
        assert_eq!(previous_chapter(&starts, 1_000), 0);
        assert_eq!(next_chapter(&starts, 61_000), Some(120_000));
        assert_eq!(next_chapter(&starts, 130_000), None);
        assert_eq!(end_of_chapter(&starts, 150_000, 61_000), 120_000);
        assert_eq!(end_of_chapter(&starts, 150_000, 130_000), 150_000);
        assert_eq!(previous_chapter(&[], 5_000), 0);
    }

    #[test]
    fn progress_and_finishing() {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrate(&mut c).unwrap();
        c.execute_batch(
            "INSERT INTO item(id, kind, title) VALUES (5, 'audiobook', 'B');
             INSERT INTO book(item_id, key, duration_ms) VALUES (5, 'b:1:B', 100000);",
        )
        .unwrap();
        assert_eq!(start_position(&c, 5).unwrap(), 0);
        assert!(!save_progress(&c, 5, 42_000, 1).unwrap());
        assert_eq!(start_position(&c, 5).unwrap(), 42_000);
        // The last 30 s: finished, and the next start is from the beginning.
        assert!(save_progress(&c, 5, 71_000, 2).unwrap());
        assert_eq!(start_position(&c, 5).unwrap(), 0);
        mark_finished(&c, 5, false, 3).unwrap();
        assert_eq!(start_position(&c, 5).unwrap(), 71_000);
        set_speed(&c, 5, Some(1.27)).unwrap();
        let speed: f64 = c
            .query_row("SELECT speed FROM book WHERE item_id = 5", [], |r| r.get(0))
            .unwrap();
        assert_eq!(speed, 1.3);
        set_speed(&c, 5, Some(9.0)).unwrap();
        let speed: f64 = c
            .query_row("SELECT speed FROM book WHERE item_id = 5", [], |r| r.get(0))
            .unwrap();
        assert_eq!(speed, 3.0);
    }

    #[test]
    fn timeline() {
        let d = [60_000, 30_000, 45_000];
        assert_eq!(locate(&d, 0), (0, 0));
        assert_eq!(locate(&d, 59_999), (0, 59_999));
        assert_eq!(locate(&d, 60_000), (1, 0));
        assert_eq!(locate(&d, 100_000), (2, 10_000));
        assert_eq!(locate(&d, 999_999), (2, 45_000));
        assert_eq!(position_of(&d, 2, 10_000), 100_000);
        assert_eq!(position_of(&d, 0, 5), 5);
        assert_eq!(locate(&[], 5), (0, 0));
    }

    #[test]
    fn chapters_on_the_timeline() {
        let files = vec![
            FileInput {
                file_id: 1,
                duration_ms: 1000,
                title: "One".into(),
            },
            FileInput {
                file_id: 2,
                duration_ms: 2000,
                title: "Two".into(),
            },
        ];
        let ch = file_chapters(&files);
        assert_eq!(
            ch[1],
            Chapter {
                title: "Two".into(),
                start_ms: 1000,
                end_ms: 3000
            }
        );
        assert_eq!(chapter_at(&ch, 0), Some(0));
        assert_eq!(chapter_at(&ch, 999), Some(0));
        assert_eq!(chapter_at(&ch, 1000), Some(1));
        assert_eq!(chapter_at(&ch, 99_999), Some(1));
        assert_eq!(chapter_at(&[], 5), None);
    }

    #[test]
    fn continue_with_the_series() {
        // Nothing heard yet: the first.
        assert_eq!(
            continue_series(&[(1, false, None), (2, false, None)]),
            Some(1)
        );
        // Book 1 finished last: book 2.
        assert_eq!(
            continue_series(&[(1, true, Some(5)), (2, false, None), (3, false, None)]),
            Some(2)
        );
        // Book 2 in progress, heard last: book 2.
        assert_eq!(
            continue_series(&[(1, true, Some(5)), (2, false, Some(9)), (3, false, None)]),
            Some(2)
        );
        // The last one finished, an earlier one not: that one.
        assert_eq!(
            continue_series(&[(1, false, None), (2, true, Some(9))]),
            Some(1)
        );
        // All finished: none.
        assert_eq!(
            continue_series(&[(1, true, Some(1)), (2, true, Some(2))]),
            None
        );
    }

    /// Issue #67: online data fills only empty fields the user did not correct; a later choice
    /// replaces what an earlier lookup filled, never what was local.
    #[test]
    fn online_data_fills_gaps_only() {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrate(&mut c).unwrap();
        c.execute_batch(
            "INSERT INTO source(id, kind, name, content) VALUES (2, 'local', 'Books', 'audiobooks');
             INSERT INTO file(id, source_id, path) VALUES (10, 2, 'A/B.m4b');",
        )
        .unwrap();
        let input = BookInput {
            key: "b:2:A/B.m4b".into(),
            title: "Book".into(),
            authors: vec!["Ann Author".into()],
            description: Some("From the OPF.".into()),
            isbn: Some("9780000000001".into()),
            mb_release: Some("rel-1".into()),
            files: vec![FileInput {
                file_id: 10,
                duration_ms: 1000,
                title: "B".into(),
            }],
            ..Default::default()
        };
        let (id, _) = upsert(&c, &input, 1).unwrap();
        let t = target(&c, id).unwrap().unwrap();
        assert_eq!(t.authors, ["Ann Author"]);
        assert_eq!(t.isbn.as_deref(), Some("9780000000001"));
        assert_eq!(t.mb_release.as_deref(), Some("rel-1"));
        assert!(t.has_description && !t.has_year && !t.has_language && !t.has_cover);
        assert_eq!(due(&c, 10).unwrap(), [id]);

        let online = OnlineBook {
            work: Some("OL1W".into()),
            description: Some("From Open Library.".into()),
            year: Some(1950),
            language: Some("eng".into()),
            subjects: vec!["Fiction".into()],
        };
        // The user corrected the language before.
        c.execute(
            "UPDATE book SET locked = '[\"language\"]' WHERE item_id = ?1",
            [id],
        )
        .unwrap();
        apply_online(&c, id, &online, false, 5).unwrap();
        let d = detail(&c, id).unwrap().unwrap();
        assert_eq!(d.description, "From the OPF.");
        assert_eq!(d.year, Some(1950));
        assert_eq!(d.language, "");
        assert_eq!(d.subjects, ["Fiction"]);
        assert_eq!(d.match_state, "matched");
        assert!(due(&c, 10).unwrap().is_empty());
        assert_eq!(
            target(&c, id).unwrap().unwrap().work.as_deref(),
            Some("OL1W")
        );

        // The user picks another work: the year it filled is replaced, the OPF text stays.
        let other = OnlineBook {
            work: Some("OL2W".into()),
            description: Some("Other.".into()),
            year: Some(1960),
            ..Default::default()
        };
        apply_online(&c, id, &other, true, 6).unwrap();
        let d = detail(&c, id).unwrap().unwrap();
        assert_eq!(
            (d.description.as_str(), d.year),
            ("From the OPF.", Some(1960))
        );
        assert_eq!(
            target(&c, id).unwrap().unwrap().work.as_deref(),
            Some("OL2W")
        );
        // A rescan keeps what was filled online (the local sources still have no year).
        upsert(&c, &input, 7).unwrap();
        assert_eq!(detail(&c, id).unwrap().unwrap().year, Some(1960));
    }

    #[test]
    fn a_book_written_and_read() {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrate(&mut c).unwrap();
        c.execute_batch(
            "INSERT INTO source(id, kind, name, content) VALUES (2, 'local', 'Books', 'audiobooks');
             INSERT INTO file(id, source_id, path) VALUES (10, 2, 'A/B/01.mp3'), (11, 2, 'A/B/02.mp3');",
        )
        .unwrap();
        let input = BookInput {
            key: "b:2:A/B".into(),
            title: "The Book".into(),
            authors: vec!["Ann Author".into()],
            narrators: vec!["Ned Narrator".into(), "Ann Author".into()],
            series: Some("Saga".into()),
            series_position: Some(2.5),
            published: Some("1999-01-02".into()),
            files: vec![
                FileInput {
                    file_id: 10,
                    duration_ms: 1000,
                    title: "Part 1".into(),
                },
                FileInput {
                    file_id: 11,
                    duration_ms: 2000,
                    title: "Part 2".into(),
                },
            ],
            ..Default::default()
        };
        let (id, created) = upsert(&c, &input, 1).unwrap();
        assert!(created);
        let d = detail(&c, id).unwrap().unwrap();
        assert_eq!(d.title, "The Book");
        assert_eq!(d.year, Some(1999));
        assert_eq!(
            d.authors
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["Ann Author"]
        );
        assert_eq!(
            d.narrators
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["Ned Narrator", "Ann Author"]
        );
        // One person item for Ann, as author and narrator.
        let persons: i64 = c
            .query_row("SELECT count(*) FROM item WHERE kind = 'person'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(persons, 2);
        assert_eq!(d.series, "Saga");
        assert_eq!(d.series_position, Some(2.5));
        assert_eq!(d.duration_ms, 3000);
        assert_eq!(d.chapters.len(), 2);
        assert_eq!(d.files[1].start_ms, 1000);
        // Again, unchanged: the same book.
        let (again, created) = upsert(&c, &input, 2).unwrap();
        assert_eq!((again, created), (id, false));
        // A corrected title stays.
        c.execute("UPDATE item SET title = 'Corrected' WHERE id = ?1", [id])
            .unwrap();
        c.execute(
            "UPDATE book SET locked = '[\"title\"]' WHERE item_id = ?1",
            [id],
        )
        .unwrap();
        upsert(&c, &input, 3).unwrap();
        assert_eq!(detail(&c, id).unwrap().unwrap().title, "Corrected");
        // Gone files take the book, its series and its people along.
        c.execute("DELETE FROM file", []).unwrap();
        prune(&c).unwrap();
        let left: i64 = c
            .query_row(
                "SELECT count(*) FROM item WHERE kind IN ('audiobook','person','book_series')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
    }
}
