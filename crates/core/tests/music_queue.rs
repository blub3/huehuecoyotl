//! Issue #8: the play queue in the core, without Qt. Playing an album hands out the current and
//! the following track as tickets; mpv moving on advances the queue; "play next" lands after
//! the current track; repeat one repeats it; plays are counted at half a track; and the queue
//! with its position comes back after a restart.
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

fn flac(dir: &Path, rel: &str, album: &str, n: u32) {
    let out = dir.join(rel);
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
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
        .args(["-c:a", "flac"])
        .arg("-metadata")
        .arg(format!("title={album}{n}"))
        .args(["-metadata", "artist=Band", "-metadata"])
        .arg(format!("album={album}"))
        .arg("-metadata")
        .arg(format!("track={n}"))
        .arg(&out)
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

/// The next queue or ticket event (others are skipped), as JSON with a tag.
fn next(rx: &Receiver<Event>) -> (&'static str, Value) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(Event::QueueChanged { json, .. }) => {
                return ("queue", serde_json::from_str(&json).unwrap());
            }
            Ok(Event::MusicTickets { json }) => {
                return ("tickets", serde_json::from_str(&json).unwrap());
            }
            Ok(Event::Error { message, .. }) => panic!("error: {message}"),
            Ok(_) => {}
            Err(_) => panic!("no queue event"),
        }
    }
}

fn titles(q: &Value) -> Vec<String> {
    q["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["title"].as_str().unwrap().to_string())
        .collect()
}

fn album_id(db: &Path, title: &str) -> i64 {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let c = Connection::open(db).unwrap();
        let id: Option<i64> = c
            .query_row(
                "SELECT a.id FROM item a WHERE a.kind = 'album' AND a.title = ?1
                   AND (SELECT count(*) FROM item t WHERE t.parent_id = a.id) = 3",
                [title],
                |r| r.get(0),
            )
            .ok();
        if let Some(id) = id {
            return id;
        }
        assert!(Instant::now() < deadline, "album {title} not scanned");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn the_play_queue() {
    if !ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("music");
    for n in 1..=3 {
        flac(&m, &format!("Band/A/0{n}.flac"), "A", n);
        flac(&m, &format!("Band/B/0{n}.flac"), "B", n);
    }
    let data = d.path().join("data");
    let db = data.join("library.db");
    let (core, rx) = open(&data);
    core.set_music_online(9, false); // nothing is looked up online here
    core.add_library(1, &m.to_string_lossy(), "music");
    let a = album_id(&db, "A");
    let b = album_id(&db, "B");

    // Play album A: the queue, then the tickets for A1 and A2.
    core.queue_play(2, vec![a], 0, false);
    let (k, q) = next(&rx);
    assert_eq!(k, "queue");
    assert_eq!(titles(&q), ["A1", "A2", "A3"]);
    assert_eq!(q["position"], 0);
    let (k, t) = next(&rx);
    assert_eq!(k, "tickets");
    assert_eq!(t["action"], "play");
    assert_eq!(t["current"]["entry"]["title"], "A1");
    assert_eq!(t["current"]["entry"]["artists"], "Band");
    assert_eq!(t["next"]["entry"]["title"], "A2");
    assert!(t["current"]["url"].as_str().unwrap().ends_with("A/01.flac"));
    let a1 = t["current"]["ticket"].as_u64().unwrap();
    let a2 = t["next"]["ticket"].as_u64().unwrap();

    // A late report for a ticket that is not the following one changes nothing.
    core.queue_advanced(a1);
    // mpv went on to A2: the queue moves, and A3 follows.
    core.queue_advanced(a2);
    let (_, q) = next(&rx);
    assert_eq!(q["position"], 1);
    let (_, t) = next(&rx);
    assert_eq!(t["action"], "next");
    assert_eq!(t["next"]["entry"]["title"], "A3");

    // Play album B next: after A2, before A3.
    core.queue_insert(3, vec![b], true);
    let (_, q) = next(&rx);
    assert_eq!(titles(&q), ["A1", "A2", "B1", "B2", "B3", "A3"]);
    let (_, t) = next(&rx);
    assert_eq!(t["next"]["entry"]["title"], "B1");

    // Repeat one: A2 follows itself.
    core.queue_set_mode(4, false, "one");
    let (_, q) = next(&rx);
    assert_eq!(q["repeat"], "one");
    let (_, t) = next(&rx);
    assert_eq!(t["next"]["entry"]["title"], "A2");

    // Half of A2 played: counted once, however often it is reported.
    core.music_progress(a2, 100_000, 200_000);
    core.music_progress(a2, 120_000, 200_000);
    std::thread::sleep(Duration::from_millis(300));
    let c = Connection::open(&db).unwrap();
    let plays: i64 = c
        .query_row(
            "SELECT p.play_count FROM playstate p JOIN item t ON t.id = p.item_id WHERE t.title = 'A2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(plays, 1);
    drop(c);

    // After a restart: the same queue and position; resuming loads A2 where it was.
    drop(core);
    let (core, rx) = open(&data);
    core.queue_state(5);
    let (_, q) = next(&rx);
    assert_eq!(titles(&q), ["A1", "A2", "B1", "B2", "B3", "A3"]);
    assert_eq!(q["position"], 1);
    assert_eq!(q["repeat"], "one");
    core.queue_resume(6, false);
    let _ = next(&rx);
    let (_, t) = next(&rx);
    assert_eq!(t["action"], "play");
    assert_eq!(t["startMs"], 120_000);
    assert_eq!(t["current"]["entry"]["title"], "A2");

    // Play after the end of the queue: what was added since (here: what follows A2).
    core.queue_set_mode(12, false, "off");
    let _ = next(&rx);
    let _ = next(&rx);
    core.queue_resume(10, true);
    let (_, q) = next(&rx);
    assert_eq!(q["position"], 2);
    let (_, t) = next(&rx);
    assert_eq!(t["current"]["entry"]["title"], "B1");
    core.queue_jump(11, 1);
    let _ = next(&rx);
    let _ = next(&rx);

    // Removing the current entry plays the one after it; clearing stops.
    core.queue_remove(7, 1);
    let (_, q) = next(&rx);
    assert_eq!(titles(&q), ["A1", "B1", "B2", "B3", "A3"]);
    let (_, t) = next(&rx);
    assert_eq!(t["current"]["entry"]["title"], "B1");
    core.queue_clear(8);
    let (_, q) = next(&rx);
    assert_eq!(q["count"], 0);
    let (_, t) = next(&rx);
    assert_eq!(t["action"], "stop");
}
