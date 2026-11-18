//! Issue #67: audiobooks are looked up online through the book provider (and the music provider
//! for books tagged with a MusicBrainz release); fakes here, the network is never used. With
//! online information off, neither is asked. A book with an ISBN in its OPF is matched with one
//! search; local metadata is never overwritten; books without a cover get one; a doubtful book
//! waits for review, and "Search again" and choosing a work settle it.
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event};
use huehuecoyotl_library::rusqlite::Connection;
use huehuecoyotl_metadata::books::{BookHit, BookProvider, BookQuery, Work};
use huehuecoyotl_metadata::music::{MusicProvider, Release};
use huehuecoyotl_metadata::{MetaError, Result};
use serde_json::Value;

fn png() -> Bytes {
    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::new(400, 600))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    Bytes::from(png)
}

#[derive(Default)]
struct Books {
    calls: Mutex<Vec<String>>,
}

fn hit(id: &str, title: &str, author: &str, year: u16, cover: Option<i64>) -> BookHit {
    BookHit {
        id: id.into(),
        title: title.into(),
        authors: vec![author.into()],
        year: Some(year),
        cover_id: cover,
        languages: vec!["eng".into()],
    }
}

#[async_trait]
impl BookProvider for Books {
    async fn search(&self, q: BookQuery<'_>) -> Result<Vec<BookHit>> {
        self.calls.lock().unwrap().push(format!("search {q:?}"));
        Ok(match q {
            BookQuery::Isbn("9780000000001") => {
                vec![hit("OL1W", "Isbn Book", "Opf Writer", 1901, Some(11))]
            }
            BookQuery::Isbn(_) => vec![],
            BookQuery::TitleAuthor {
                title: "Plain Title",
                ..
            } => vec![
                hit("OL2W", "Plain Title", "Plain Author", 1902, Some(22)),
                hit("OL9W", "Plain Title", "Plain Author", 1990, None),
            ],
            BookQuery::TitleAuthor { title, .. } if title.contains("Doubtful") => vec![
                hit("OL3W", "Doubtful Times", "Someone", 1903, None),
                hit("OL4W", "The Doubtful Guest", "Another", 1904, None),
            ],
            BookQuery::TitleAuthor { .. } => vec![],
        })
    }
    async fn work(&self, id: &str) -> Result<Work> {
        self.calls.lock().unwrap().push(format!("work {id}"));
        Ok(Work {
            id: id.into(),
            description: Some(format!("Online text of {id}.")),
            subjects: vec!["Fiction".into()],
            year: Some(1950),
            cover_ids: vec![44],
            ..Default::default()
        })
    }
    async fn cover(&self, id: i64) -> Result<Option<Bytes>> {
        self.calls.lock().unwrap().push(format!("cover {id}"));
        Ok(Some(png()))
    }
}

#[derive(Default)]
struct Music {
    calls: Mutex<Vec<String>>,
}

#[async_trait]
impl MusicProvider for Music {
    async fn release(&self, id: &str) -> Result<Release> {
        self.calls.lock().unwrap().push(format!("release {id}"));
        if id != "book-release" {
            return Err(MetaError::NotFound);
        }
        Ok(Release {
            id: id.into(),
            title: "Tagged Book".into(),
            date: Some("1977-05-01".into()),
            group_id: Some("book-group".into()),
            ..Default::default()
        })
    }
    async fn front_cover(&self, release: &str, _group: Option<&str>) -> Result<Option<Bytes>> {
        self.calls.lock().unwrap().push(format!("cover {release}"));
        Ok(Some(png()))
    }
}

fn ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn mp3(out: &Path, tags: &[(&str, &str)]) {
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "sine=duration=0.3",
        "-c:a",
        "libmp3lame",
    ]);
    for (k, v) in tags {
        cmd.arg("-metadata").arg(format!("{k}={v}"));
    }
    assert!(cmd.arg(out).status().unwrap().success());
}

/// title → (match state, overview, year, has a cover), polled until `done` says so.
type Row = (String, String, String, Option<i64>, bool);
fn books(db: &Path, done: impl Fn(&[Row]) -> bool) -> Vec<Row> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let rows: Vec<Row> = Connection::open(db)
            .ok()
            .and_then(|c| {
                c.prepare(
                    "SELECT i.title, coalesce(i.match_state, ''), coalesce(i.overview, ''), i.year,
                            EXISTS (SELECT 1 FROM artwork w WHERE w.item_id = i.id AND w.role = 'poster_grid' AND w.local_path IS NOT NULL)
                     FROM item i WHERE i.kind = 'audiobook' ORDER BY i.title",
                )
                .ok()?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
                .ok()?
                .collect::<std::result::Result<_, _>>()
                .ok()
            })
            .unwrap_or_default();
        if done(&rows) || Instant::now() > deadline {
            return rows;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn detail(rx: &Receiver<Event>, want: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("no event")
        {
            Event::MusicDetail { kind, json, .. } if kind == want => {
                return serde_json::from_str(&json).unwrap();
            }
            Event::Error { message, .. } => panic!("error: {message}"),
            _ => {}
        }
    }
}

#[test]
fn books_are_looked_up_online() {
    if !ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let b = d.path().join("books");
    // An OPF with an ISBN and a description of its own.
    mp3(&b.join("Opf Writer/Isbn Book/01.mp3"), &[]);
    std::fs::write(
        b.join("Opf Writer/Isbn Book/metadata.opf"),
        r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
<dc:title>Isbn Book</dc:title><dc:creator opf:role="aut">Opf Writer</dc:creator>
<dc:identifier opf:scheme="ISBN">9780000000001</dc:identifier><dc:description>The local description of the book.</dc:description>
</metadata></package>"#,
    )
    .unwrap();
    // Names only.
    mp3(&b.join("Plain Author/Plain Title/01.mp3"), &[]);
    // Tagged by Picard.
    mp3(
        &b.join("Tag Writer/Tagged Book/01.mp3"),
        &[
            ("album", "Tagged Book"),
            ("artist", "Tag Writer"),
            ("MusicBrainz Album Id", "book-release"),
        ],
    );
    // Nothing clear online.
    mp3(&b.join("Doubt Writer/Doubtful/01.mp3"), &[]);

    let data = d.path().join("data");
    let db = data.join("library.db");
    let (tx, rx) = unbounded();
    let core = Core::open(&data, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    let (bp, mp) = (Arc::new(Books::default()), Arc::new(Music::default()));
    core.set_book_provider(bp.clone());
    core.set_music_provider(mp.clone());

    // Off: the books are scanned, and nothing is asked.
    core.set_music_online(1, false);
    let _ = detail_or_online(&rx);
    core.add_library(2, &b.to_string_lossy(), "audiobooks");
    let rows = books(&db, |r| r.len() == 4);
    assert_eq!(rows.len(), 4, "{rows:?}");
    std::thread::sleep(Duration::from_millis(1500));
    assert!(bp.calls.lock().unwrap().is_empty());
    assert!(mp.calls.lock().unwrap().is_empty());

    // On: every book is looked up.
    core.set_music_online(3, true);
    let rows = books(&db, |r| {
        r.iter().all(|b| b.1 != "unmatched") && r.iter().filter(|b| b.4).count() == 3
    });
    let by = |t: &str| rows.iter().find(|r| r.0 == t).unwrap().clone();
    // The ISBN: one search; the local description stays; the year and a cover are filled.
    assert_eq!(
        by("Isbn Book"),
        (
            "Isbn Book".into(),
            "matched".into(),
            "The local description of the book.".into(),
            Some(1901),
            true
        )
    );
    // By title and author: the first agreeing work, its description and cover.
    assert_eq!(
        by("Plain Title"),
        (
            "Plain Title".into(),
            "matched".into(),
            "Online text of OL2W.".into(),
            Some(1902),
            true
        )
    );
    // Tagged: MusicBrainz's year and cover; Open Library is not asked.
    assert_eq!(
        by("Tagged Book"),
        (
            "Tagged Book".into(),
            "matched".into(),
            String::new(),
            Some(1977),
            true
        )
    );
    let doubtful = by("Doubtful");
    assert_eq!((doubtful.1.as_str(), doubtful.4), ("needs_review", false));
    let calls = bp.calls.lock().unwrap().clone();
    let isbn_calls: Vec<&String> = calls.iter().filter(|c| c.contains("Isbn")).collect();
    assert_eq!(isbn_calls.len(), 1, "{calls:?}");
    assert!(!calls.iter().any(|c| c.contains("Isbn Book")), "{calls:?}");
    assert!(!calls.iter().any(|c| c.contains("Tagged")), "{calls:?}");
    assert!(!calls.contains(&"work OL1W".to_string()), "{calls:?}");
    assert_eq!(
        mp.calls.lock().unwrap().clone(),
        ["release book-release", "cover book-release"]
    );

    // "Search again" and a choice settle the doubtful book.
    let id: i64 = Connection::open(&db)
        .unwrap()
        .query_row("SELECT id FROM item WHERE title = 'Doubtful'", [], |r| {
            r.get(0)
        })
        .unwrap();
    core.book_detail(4, id);
    let page = detail(&rx, "book");
    assert_eq!(page["matchState"], "needs_review");
    assert_eq!(page["candidates"].as_array().unwrap().len(), 2);
    core.search_book(5, id, "The Doubtful Guest", "");
    let found = detail(&rx, "bookCandidates");
    assert_eq!(found["bookId"], id);
    assert_eq!(found["candidates"][1]["id"], "OL4W");
    core.choose_book_match(6, id, "OL4W");
    let page = detail(&rx, "book");
    assert_eq!(page["matchState"], "matched");
    assert_eq!(page["description"], "Online text of OL4W.");
    assert_eq!(page["year"], 1950);
    assert_eq!(page["subjects"][0], "Fiction");
    assert!(!page["poster"].as_str().unwrap().is_empty());

    // Off again: "Search again" sends nothing.
    core.set_music_online(7, false);
    assert!(!detail_or_online(&rx));
    let before = bp.calls.lock().unwrap().len();
    core.search_book(8, id, "", "");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("no answer")
        {
            Event::Error { .. } => break,
            Event::MusicDetail { kind, .. } if kind == "bookCandidates" => {
                panic!("searched while off")
            }
            _ => {}
        }
    }
    assert_eq!(bp.calls.lock().unwrap().len(), before);
}

/// Wait for the answer to `set_music_online`.
fn detail_or_online(rx: &Receiver<Event>) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Event::MusicOnline { online, .. } = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("no event")
        {
            return online;
        }
    }
}
