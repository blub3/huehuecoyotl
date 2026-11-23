//! Issues #108 to #110 through the public core API with a fake Open Library (the network is
//! never used): choosing a book's work brings its unmatched author along with photo and
//! biography (never a locked author); the field preview keeps the book's own data; authors of
//! one name are told apart by the user's books; a chosen author is locked.
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event};
use huehuecoyotl_library::rusqlite::Connection;
use huehuecoyotl_metadata::Result;
use huehuecoyotl_metadata::books::{Author, BookHit, BookProvider, BookQuery, Work};
use serde_json::Value;

struct Ol;

fn png() -> Bytes {
    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::new(40, 60))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    Bytes::from(png)
}

#[async_trait]
impl BookProvider for Ol {
    async fn search(&self, _q: BookQuery<'_>) -> Result<Vec<BookHit>> {
        // Nothing sure at first: the books stay unmatched until the user chooses.
        Ok(vec![])
    }
    async fn work(&self, id: &str) -> Result<Work> {
        Ok(Work {
            id: id.into(),
            title: "The Salt Archive".into(),
            description: Some("Online text.".into()),
            year: Some(2018),
            subjects: vec!["Fiction".into()],
            author_ids: vec!["OL1A".into(), "OL9A".into()],
            ..Default::default()
        })
    }
    async fn cover(&self, _id: i64) -> Result<Option<Bytes>> {
        Ok(None)
    }
    async fn search_authors(&self, name: &str) -> Result<Vec<Author>> {
        Ok(if name == "Liv Ostrander" {
            vec![
                Author {
                    id: "OL2A".into(),
                    name: name.into(),
                    birth: Some("1902".into()),
                    ..Default::default()
                },
                Author {
                    id: "OL1A".into(),
                    name: name.into(),
                    birth: Some("1974".into()),
                    ..Default::default()
                },
            ]
        } else {
            vec![]
        })
    }
    async fn author(&self, id: &str) -> Result<Author> {
        Ok(match id {
            "OL1A" => Author {
                id: id.into(),
                name: "Liv Ostrander".into(),
                bio: Some("Norwegian novelist.".into()),
                photo_ids: vec![7],
                ..Default::default()
            },
            "OL2A" => Author {
                id: id.into(),
                name: "Liv Ostrander".into(),
                ..Default::default()
            },
            _ => Author {
                id: id.into(),
                name: "Locked Writer".into(),
                ..Default::default()
            },
        })
    }
    async fn author_works(&self, id: &str) -> Result<Vec<String>> {
        Ok(if id == "OL1A" {
            vec!["The Salt Archive".into(), "Harbour Ledger".into()]
        } else {
            vec!["Hymns".into()]
        })
    }
    async fn author_photo(&self, _id: i64) -> Result<Option<Bytes>> {
        Ok(Some(png()))
    }
}

fn ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn mp3(out: &Path) {
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    assert!(
        Command::new("ffmpeg")
            .args([
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=0.3",
                "-c:a",
                "libmp3lame"
            ])
            .arg(out)
            .status()
            .unwrap()
            .success()
    );
}

fn answer(rx: &Receiver<Event>, id: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    while let Ok(e) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        match e {
            Event::MusicDetail {
                request_id, json, ..
            } if request_id == id => return serde_json::from_str(&json).unwrap(),
            Event::Error {
                request_id,
                message,
                ..
            } if request_id == id => panic!("error: {message}"),
            _ => {}
        }
    }
    panic!("no answer to {id}");
}

fn q<T: huehuecoyotl_library::rusqlite::types::FromSql>(db: &Path, sql: &str) -> Option<T> {
    Connection::open(db)
        .ok()?
        .query_row(sql, [], |r| r.get(0))
        .ok()
}

#[test]
fn authors_follow_and_are_told_apart() {
    if !ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    // SAFETY: set before any thread of this test process reads the environment.
    unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
    let d = tempfile::tempdir().unwrap();
    let b = d.path().join("books");
    mp3(&b.join("Liv Ostrander/The Salt Archive/01.mp3"));
    std::fs::write(
        b.join("Liv Ostrander/The Salt Archive/desc.txt"),
        "My own description.",
    )
    .unwrap();
    mp3(&b.join("Liv Ostrander/Harbour Ledger/01.mp3"));
    mp3(&b.join("Locked Writer/Quiet Count/01.mp3"));
    let data = d.path().join("data");
    let db = data.join("library.db");
    let (tx, rx) = unbounded();
    let core = Core::open(&data, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    core.set_book_provider(Arc::new(Ol));
    core.add_library(1, &b.to_string_lossy(), "audiobooks");
    let deadline = Instant::now() + Duration::from_secs(30);
    while q::<i64>(
        &db,
        "SELECT count(*) FROM item WHERE kind = 'audiobook' AND match_state = 'unmatched'",
    ) != Some(3)
    {
        assert!(Instant::now() < deadline, "books looked up");
        std::thread::sleep(Duration::from_millis(100));
    }
    Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE item SET locked = 1 WHERE kind = 'person' AND title = 'Locked Writer'",
            [],
        )
        .unwrap();
    let salt: i64 = q(
        &db,
        "SELECT id FROM item WHERE kind = 'audiobook' AND title = 'The Salt Archive'",
    )
    .unwrap();

    // #109: the preview keeps the book's own description, fills the year.
    core.match_preview(2, salt, r#"{"id":"OL9W","kind":"work"}"#);
    let p = answer(&rx, 2);
    let field = |name: &str| {
        p["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["field"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(field("description")["shown"], "yours");
    assert_eq!(field("year")["shown"], "openLibrary");
    assert_eq!(field("year")["theirs"], "2018");

    // Choosing the work: the book is locked; Liv Ostrander follows the work's author with photo
    // and biography; the locked writer (also credited by the work, by a fake) does not.
    core.match_choose(3, salt, r#"{"id":"OL9W","kind":"work"}"#);
    answer(&rx, 3);
    assert_eq!(
        q::<i64>(&db, &format!("SELECT locked FROM item WHERE id = {salt}")),
        Some(1)
    );
    assert_eq!(
        q::<String>(&db, &format!("SELECT overview FROM item WHERE id = {salt}")).as_deref(),
        Some("My own description.")
    );
    let liv: i64 = q(
        &db,
        "SELECT id FROM item WHERE kind = 'person' AND title = 'Liv Ostrander'",
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        core.book_detail(4, liv);
        let page = answer(&rx, 4);
        if page["image"].as_str().is_some_and(|s| !s.is_empty()) {
            assert_eq!(page["matched"], true);
            assert_eq!(page["matchedVia"]["title"], "The Salt Archive");
            assert_eq!(page["biography"], "Norwegian novelist.");
            break;
        }
        assert!(Instant::now() < deadline, "photo saved: {page}");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        q::<i64>(
            &db,
            "SELECT count(*) FROM external_id e JOIN item p ON p.id = e.item_id WHERE p.title = 'Locked Writer' AND e.provider = 'openlibrary'"
        ),
        Some(0)
    );

    // #110: two authors of one name; the one with the user's books is the match.
    core.match_again(5, liv, false);
    assert_eq!(answer(&rx, 5)["result"], "unchanged");
    core.match_search(6, liv, r#"{"text":""}"#);
    let s = answer(&rx, 6);
    let c = s["candidates"].as_array().unwrap();
    let one = c.iter().find(|x| x["id"] == "OL1A").unwrap();
    assert_eq!(
        (
            one["books"].as_i64(),
            one["ofBooks"].as_i64(),
            one["current"].as_bool()
        ),
        (Some(2), Some(2), Some(true))
    );
    assert_eq!(c.iter().find(|x| x["id"] == "OL2A").unwrap()["books"], 0);
    core.match_choose(7, liv, r#"{"id":"OL2A","kind":"author"}"#);
    answer(&rx, 7);
    core.book_detail(8, liv);
    let page = answer(&rx, 8);
    assert_eq!(
        (page["locked"].as_bool(), page["matchedVia"].is_null()),
        (Some(true), true)
    );
}
