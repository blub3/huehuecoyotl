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
