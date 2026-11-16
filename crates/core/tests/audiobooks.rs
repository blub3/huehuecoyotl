//! Issue #65 in the core, without Qt: an audiobook folder added as such becomes a book; the
//! probe's embedded chapters of an M4B become the book's chapters; the book page, its author
//! and its series are served as `MusicDetail` events.
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event};
use huehuecoyotl_library::rusqlite::Connection;
use serde_json::Value;

fn ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A three-second M4B with three chapters, in a series by its tags.
fn m4b(out: &Path) {
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    let meta = out.with_extension("txt");
    std::fs::write(
        &meta,
        ";FFMETADATA1\nalbum=Chaptered\nartist=Writer\ncomposer=Reader\n\
         [CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1000\ntitle=Opening\n\
         [CHAPTER]\nTIMEBASE=1/1000\nSTART=1000\nEND=2000\ntitle=Middle\n\
         [CHAPTER]\nTIMEBASE=1/1000\nSTART=2000\nEND=3000\ntitle=Ending\n",
    )
    .unwrap();
    let ok = Command::new("ffmpeg")
        .args([
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=duration=3",
        ])
        .arg("-i")
        .arg(&meta)
        .args([
            "-map",
            "0:a",
            "-map_metadata",
            "1",
            "-map_chapters",
            "1",
            "-c:a",
            "aac",
            "-f",
            "mp4",
        ])
        .arg(out)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    std::fs::remove_file(meta).unwrap();
}

fn detail(rx: &Receiver<Event>) -> (String, Value) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("no event")
        {
            Event::MusicDetail { kind, json, .. } => {
                return (kind.to_string(), serde_json::from_str(&json).unwrap());
            }
            Event::Error { message, .. } => panic!("error: {message}"),
            _ => {}
        }
    }
}

/// The book's id once its embedded chapters are in.
fn wait_chapters(db: &Path) -> i64 {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(c) = Connection::open(db)
            && let Ok(id) = c.query_row(
                "SELECT book_id FROM chapter WHERE title = 'Ending'",
                [],
                |r| r.get::<_, i64>(0),
            )
        {
            return id;
        }
        if Instant::now() > deadline {
            let c = Connection::open(db).unwrap();
            let dump = |sql: &str| -> Vec<String> {
                let mut st = c.prepare(sql).unwrap();
                st.query_map([], |r| r.get::<_, String>(0))
                    .unwrap()
                    .map(|r| r.unwrap())
                    .collect()
            };
            panic!(
                "chapters not probed: books {:?} files {:?} jobs {:?} chapters {:?}",
                dump("SELECT key FROM book"),
                dump("SELECT path || ' ' || coalesce(substr(probe_json, 1, 300), '-') FROM file"),
                dump(
                    "SELECT kind || ' ' || state || ' ' || attempts || ' ' || (run_after - strftime('%s','now')) || ' ' || coalesce(last_error, '') FROM job"
                ),
                dump("SELECT title FROM chapter"),
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_book_with_chapters() {
    // SAFETY: set before the probe loads FFmpeg, the only test in this file.
    unsafe {
        std::env::set_var(
            "COYOTL_FFMPEG_DIR",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../third_party/mpv/out/lib"),
        )
    };
    if !ffmpeg() || huehuecoyotl_probe::ffmpeg().is_err() {
        eprintln!(
            "skipped: needs an ffmpeg command and the libmpv bundle (third_party/mpv/build.sh)"
        );
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let books = d.path().join("books");
    m4b(&books.join("Writer/Chaptered.m4b"));
    let data = d.path().join("data");
    let (tx, rx) = unbounded();
    let core = Core::open(&data, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    core.set_music_online(1, false);
    core.add_library(2, &books.to_string_lossy(), "audiobooks");
    let book = wait_chapters(&data.join("library.db"));

    core.book_detail(3, book);
    let (kind, page) = detail(&rx);
    assert_eq!(kind, "book");
    assert_eq!(page["title"], "Chaptered");
    assert_eq!(page["authors"][0]["name"], "Writer");
    assert_eq!(page["narrators"][0]["name"], "Reader");
    let chapters: Vec<(&str, i64)> = page["chapters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["title"].as_str().unwrap(), c["startMs"].as_i64().unwrap()))
        .collect();
    assert_eq!(
        chapters,
        [("Opening", 0), ("Middle", 1000), ("Ending", 2000)]
    );
    let duration = page["durationMs"].as_i64().unwrap();
    assert!((2900..=3200).contains(&duration), "{duration}");

    // The author's page lists the book.
    let author = page["authors"][0]["id"].as_i64().unwrap();
    core.book_detail(4, author);
    let (kind, person) = detail(&rx);
    assert_eq!(kind, "person");
    assert!(person.to_string().contains("Chaptered"), "{person}");
}

/// Wait until the book's saved position is `at` (saves are written in the background).
fn saved(db: &Path, book: i64, at: i64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Connection::open(db)
        .and_then(|c| {
            c.query_row(
                "SELECT position_ms FROM playstate WHERE item_id = ?1",
                [book],
                |r| r.get::<_, i64>(0),
            )
        })
        .ok()
        != Some(at)
    {
        assert!(Instant::now() < deadline, "position {at} not saved");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The titles of the next `browse_items` page.
fn page_titles(rx: &Receiver<Event>) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("no event")
        {
            Event::Page { items, .. } => return items.into_iter().map(|s| s.title).collect(),
            Event::Error { message, .. } => panic!("error: {message}"),
            _ => {}
        }
    }
}

/// Issue #66: a two-file book plays from a ticket that puts both files on one timeline; a
/// position inside the second file is where it starts again after a restart, exactly; the
/// last 30 s finish it; the filters and "continue series" follow.
#[test]
fn a_book_resumes_exactly_after_a_restart() {
    if !ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let books = d.path().join("books");
    for (dir, n) in [
        ("Writer/Saga/Book 1 - Two Files", 2),
        ("Writer/Saga/Book 2 - Next", 1),
    ] {
        for i in 1..=n {
            let out = books.join(format!("{dir}/0{i}.mp3"));
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
                        "sine=duration=100"
                    ])
                    .args(["-c:a", "libmp3lame", "-b:a", "32k"])
                    .arg(&out)
                    .status()
                    .unwrap()
                    .success()
            );
        }
    }
    let data = d.path().join("data");
    let db = data.join("library.db");
    let open = || {
        let (tx, rx) = unbounded();
        let core = Core::open(&data, "en-US", move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        (core, rx)
    };
    let (core, rx) = open();
    core.set_music_online(1, false);
    core.add_library(2, &books.to_string_lossy(), "audiobooks");
    let deadline = Instant::now() + Duration::from_secs(30);
    let (book, series): (i64, i64) = loop {
        if let Ok(c) = Connection::open(&db)
            && let Ok(r) = c.query_row(
                "SELECT i.id, i.parent_id FROM item i WHERE i.title = 'Two Files'
                   AND (SELECT count(*) FROM book_file f WHERE f.book_id = i.id) = 2
                   AND (SELECT count(*) FROM item n WHERE n.title = 'Next') = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        {
            break r;
        }
        assert!(Instant::now() < deadline, "not scanned");
        std::thread::sleep(Duration::from_millis(100));
    };

    core.book_play(3, book, -1);
    let (kind, t) = detail(&rx);
    assert_eq!(kind, "bookTicket");
    assert_eq!(t["positionMs"], 0);
    let files = t["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert!(files[0]["url"].as_str().unwrap().ends_with("01.mp3"));
    let second = files[1]["startMs"].as_i64().unwrap();
    assert!((99_000..=101_000).contains(&second), "{second}");
    assert_eq!(t["chapters"].as_array().unwrap().len(), 2);
    // 12.345 s into the second file.
    let at = second + 12_345;
    core.book_progress(book, at);
    core.book_set_speed(book, 1.5);
    saved(&db, book, at);
    core.browse_items(4, "audiobook", 0, "played", "in_progress", 0, 50);
    assert_eq!(page_titles(&rx), ["Two Files"]);
    core.browse_items(5, "audiobook", 0, "title", "unread", 0, 50);
    assert_eq!(page_titles(&rx), ["Next"]);
    core.browse_items(6, "person", 0, "title", "narrator", 0, 50);
    assert!(page_titles(&rx).is_empty());
    drop(core);

    let (core, rx) = open();
    core.book_play(7, book, -1);
    let (_, t) = detail(&rx);
    assert_eq!(t["positionMs"].as_i64().unwrap(), at);
    assert_eq!(t["speed"], 1.5);
    core.continue_series(8, series);
    let (kind, c) = detail(&rx);
    assert_eq!(
        (kind.as_str(), c["bookId"].as_i64().unwrap()),
        ("continueSeries", book)
    );
    // Its last 30 s: finished; the series continues with the next book; it starts over.
    let end = t["durationMs"].as_i64().unwrap() - 10_000;
    core.book_progress(book, end);
    saved(&db, book, end);
    core.browse_items(9, "audiobook", 0, "title", "finished", 0, 50);
    assert_eq!(page_titles(&rx), ["Two Files"]);
    core.continue_series(10, series);
    let (_, c) = detail(&rx);
    assert_ne!(c["bookId"].as_i64().unwrap(), book);
    core.book_play(11, book, -1);
    assert_eq!(detail(&rx).1["positionMs"], 0);
}
