//! Issue #80: "Play these" on the Songs list queues exactly what the list shows, in its order:
//! here the Hi-Res songs of a small library, best quality first, read in the core from the
//! query (the UI never sends the ids).
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

fn tone(path: &Path, codec: &[&str], album: &str, n: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let ok = Command::new("ffmpeg")
        .args([
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=duration=0.3",
        ])
        .args(codec)
        .args(["-metadata", "artist=Band", "-metadata", "album_artist=Band"])
        .arg("-metadata")
        .arg(format!("album={album}"))
        .arg("-metadata")
        .arg(format!("title={album} {n}"))
        .arg("-metadata")
        .arg(format!("track={n}"))
        .arg(path)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

fn next_queue(rx: &Receiver<Event>) -> Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("no event")
        {
            Event::QueueChanged { json, .. } => return serde_json::from_str(&json).unwrap(),
            Event::Error { message, .. } => panic!("error: {message}"),
            _ => {}
        }
    }
}

#[test]
fn play_these_queues_the_filtered_list() {
    if !ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("music");
    let hi96 = ["-c:a", "flac", "-sample_fmt", "s32", "-ar", "96000"];
    let hi192 = ["-c:a", "flac", "-sample_fmt", "s32", "-ar", "192000"];
    let mp3 = ["-c:a", "libmp3lame", "-b:a", "192k"];
    for n in 1..=2 {
        tone(
            &m.join(format!("Band/Ninety Six/0{n}.flac")),
            &hi96,
            "Ninety Six",
            n,
        );
        tone(
            &m.join(format!("Band/One Ninety Two/0{n}.flac")),
            &hi192,
            "One Ninety Two",
            n,
        );
        tone(&m.join(format!("Band/Lossy/0{n}.mp3")), &mp3, "Lossy", n);
    }
    let data = d.path().join("data");
    let (tx, rx) = unbounded();
    let core = Core::open(&data, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    core.set_music_online(1, false);
    core.add_library(2, &m.to_string_lossy(), "music");
    let deadline = Instant::now() + Duration::from_secs(30);
    while Connection::open(data.join("library.db"))
        .and_then(|c| {
            c.query_row("SELECT count(*) FROM item WHERE kind = 'track'", [], |r| {
                r.get::<_, i64>(0)
            })
        })
        .unwrap_or(0)
        < 6
    {
        assert!(Instant::now() < deadline, "not scanned");
        std::thread::sleep(Duration::from_millis(100));
    }

    core.queue_play_query(3, "track", "quality", "hires", 0, false);
    let q = next_queue(&rx);
    let titles: Vec<&str> = q["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles.len(), 4, "{q}");
    assert!(
        titles[..2].iter().all(|t| t.starts_with("One Ninety Two")),
        "{titles:?}"
    );
    assert!(
        titles[2..].iter().all(|t| t.starts_with("Ninety Six")),
        "{titles:?}"
    );

    // Nothing matches: an error, and the queue stays.
    core.queue_play_query(4, "track", "title", "hires\ngenre=Nothing", 0, false);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("no answer")
        {
            Event::Error { request_id: 4, .. } => break,
            Event::QueueChanged { request_id: 4, .. } => panic!("queued nothing"),
            _ => {}
        }
    }
}
