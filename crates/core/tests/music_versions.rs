//! Issue #63 in the core, without Qt: an album in two versions (FLAC 24/96 and MP3, in two
//! folders) and one track with an extra file. The album page lists both versions and follows the
//! one chosen; playing a version queues its files; a restored queue plays exactly those files;
//! a queued file that is gone plays from the other version and is marked; the version played
//! last is preselected next time.
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

fn tone(path: &Path, codec: &[&str], rate: u32, n: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let ok = Command::new("ffmpeg")
        .args(["-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("sine=sample_rate={rate}:duration=0.3"))
        .args(codec)
        .args([
            "-metadata",
            "artist=Band",
            "-metadata",
            "album_artist=Band",
            "-metadata",
            "album=Album",
        ])
        .args(["-metadata", "date=2000", "-metadata"])
        .arg(format!("title=Song {n}"))
        .arg("-metadata")
        .arg(format!("track={n}"))
        .arg(path)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

fn open(data: &Path) -> (Core, Receiver<Event>) {
    let (tx, rx) = unbounded();
    let core = Core::open(data, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    (core, rx)
}

/// The next event of a kind ("detail", "queue", "tickets").
fn next(rx: &Receiver<Event>, want: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let ev = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("no event");
        let (kind, json) = match ev {
            Event::MusicDetail { json, .. } => ("detail", json),
            Event::QueueChanged { json, .. } => ("queue", json),
            Event::MusicTickets { json } => ("tickets", json),
            Event::Error { message, .. } => panic!("error: {message}"),
            _ => continue,
        };
        if kind == want {
            return serde_json::from_str(&json).unwrap();
        }
    }
}

fn wait_album(db: &Path) -> i64 {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(c) = Connection::open(db)
            && let Ok(id) = c.query_row(
                "SELECT a.id FROM item a WHERE a.kind = 'album'
                   AND (SELECT count(*) FROM album_version v WHERE v.album_id = a.id) = 2
                   AND (SELECT count(*) FROM file f WHERE f.version_id IN (SELECT id FROM album_version WHERE album_id = a.id)) = 7",
                [],
                |r| r.get::<_, i64>(0),
            )
        {
            return id;
        }
        assert!(Instant::now() < deadline, "album not scanned");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn versions_in_the_core() {
    if !ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("music");
    let flac24 = ["-c:a", "flac", "-sample_fmt", "s32"];
    let mp3 = ["-c:a", "libmp3lame", "-b:a", "192k"];
    for n in 1..=3 {
        tone(
            &m.join(format!("Band/Album (2000)/0{n}.flac")),
            &flac24,
            96_000,
            n,
        );
        tone(&m.join(format!("Band/Album MP3/0{n}.mp3")), &mp3, 44_100, n);
    }
    // A second file of Song 2 in the FLAC folder (a replaced file, 16 bit).
    tone(
        &m.join("Band/Album (2000)/02 again.flac"),
        &["-c:a", "flac"],
        44_100,
        2,
    );
    let data = d.path().join("data");
    let db = data.join("library.db");
    let (core, rx) = open(&data);
    core.set_music_online(9, false);
    core.add_library(1, &m.to_string_lossy(), "music");
    let album = wait_album(&db);

    // The page: two versions, the 24/96 one first and preselected; Song 2 offers both files.
    core.album_detail(2, album, 0, "quality");
    let page = next(&rx, "detail");
    let versions = page["versions"].as_array().unwrap().clone();
    assert_eq!(versions.len(), 2);
    let (flac, mp3v) = (
        versions[0]["id"].as_i64().unwrap(),
        versions[1]["id"].as_i64().unwrap(),
    );
    assert_eq!(versions[0]["codec"], "flac");
    assert_eq!(versions[1]["codec"], "mp3");
    assert_eq!(page["versionId"], flac);
    assert_eq!(page["hires"], true);
    assert!(page["playable"].as_bool().unwrap());
    let song2 = &page["tracks"][1];
    assert_eq!(song2["files"].as_array().unwrap().len(), 2);
    // The MP3 version: its own files, no Hi-Res, no second file of Song 2.
    core.album_detail(3, album, mp3v, "quality");
    let page = next(&rx, "detail");
    assert_eq!(page["versionId"], mp3v);
    assert_eq!(page["hires"], false);
    assert!(
        page["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["files"].as_array().unwrap().is_empty())
    );
    let mp3_files: Vec<i64> = page["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["fileId"].as_i64().unwrap())
        .collect();

    // Play the MP3 version: the queue holds its files, the player gets them.
    core.queue_play(4, vec![album], 0, false, mp3v, "quality");
    let q = next(&rx, "queue");
    let queued: Vec<i64> = q["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["fileId"].as_i64().unwrap())
        .collect();
    assert_eq!(queued, mp3_files);
    let t = next(&rx, "tickets");
    assert!(
        t["current"]["url"]
            .as_str()
            .unwrap()
            .ends_with("Album MP3/01.mp3")
    );
    assert!(
        t["next"]["url"]
            .as_str()
            .unwrap()
            .ends_with("Album MP3/02.mp3")
    );

    // After a restart: the same files, in the same order.
    drop(core);
    let (core, rx) = open(&data);
    core.queue_state(5);
    let q = next(&rx, "queue");
    let restored: Vec<i64> = q["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["fileId"].as_i64().unwrap())
        .collect();
    assert_eq!(restored, mp3_files);
    // The version played last is the album page's choice now.
    core.album_detail(6, album, 0, "quality");
    assert_eq!(next(&rx, "detail")["versionId"], mp3v);

    // A queued file that is gone: the other version's file plays, and the entry says so.
    std::fs::remove_file(m.join("Band/Album MP3/02.mp3")).unwrap();
    core.queue_jump(7, 1);
    let _ = next(&rx, "queue");
    let t = next(&rx, "tickets");
    assert!(
        t["current"]["url"]
            .as_str()
            .unwrap()
            .contains("Album (2000)/02"),
        "{t}"
    );
    core.queue_state(8);
    let q = next(&rx, "queue");
    assert_eq!(q["entries"][1]["fallback"], true);
    assert_eq!(q["entries"][0]["fallback"], false);
}
