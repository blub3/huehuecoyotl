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
    all.extend(embedded.iter().map(|ch| Chapter {
        title: ch.title.clone(),
        start_ms: start + ch.start_ms,
        end_ms: (start + ch.end_ms).min(if duration > 0 { end } else { i64::MAX }),
    }));
    all.sort_by_key(|ch| ch.start_ms);
    set_chapters(c, book, &all)?;
    Ok(Some(book))
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
                    b.duration_ms, coalesce(p.position_ms, 0), coalesce(p.watched, 0), b.speed, b.cover_ratio, b.drm
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
