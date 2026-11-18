//! Issue #82: what Now Playing shows besides the tags: the album cover's background tint
//! (computed once and kept) and the song's waveform, computed by a background job after the
//! first ask (until then no peaks) and announced with ItemChanged.
use std::process::Command;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event};
use huehuecoyotl_library::rusqlite::Connection;
use serde_json::Value;

fn ffmpeg(args: &[&str]) -> bool {
    Command::new("ffmpeg")
        .args(["-loglevel", "error", "-y"])
        .args(args)
        .status()
        .is_ok_and(|s| s.success())
}

fn visuals(rx: &Receiver<Event>, id: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("no event")
        {
            Event::MusicDetail {
                request_id, json, ..
            } if request_id == id => {
                return serde_json::from_str(&json).unwrap();
            }
            Event::Error { message, .. } => panic!("error: {message}"),
            _ => {}
        }
    }
}

#[test]
fn tint_and_waveform() {
    // SAFETY: set before the probe loads FFmpeg, the only test in this file.
    unsafe {
        std::env::set_var(
            "COYOTL_FFMPEG_DIR",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../third_party/mpv/out/lib"),
        )
    };
    if !ffmpeg(&["-version"]) || huehuecoyotl_probe::ffmpeg().is_err() {
        eprintln!("skipped: needs an ffmpeg command and the libmpv bundle");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let album = d.path().join("music/Band/Album");
    std::fs::create_dir_all(&album).unwrap();
    let song = album.join("01.flac").to_string_lossy().to_string();
    assert!(ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "sine=duration=2,apad=pad_dur=2",
        "-c:a",
        "flac",
        "-metadata",
        "artist=Band",
        "-metadata",
        "album_artist=Band",
        "-metadata",
        "album=Album",
        "-metadata",
        "title=Song",
        "-metadata",
        "track=1",
        &song,
    ]));
    // A light yellow cover: the tint must still be dark enough for light text.
    let cover = album.join("cover.jpg").to_string_lossy().to_string();
    assert!(ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "color=c=0xf0e070:s=400x400",
        "-frames:v",
        "1",
        &cover
    ]));

    let data = d.path().join("data");
    let (tx, rx) = unbounded();
    let core = Core::open(&data, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    core.set_music_online(1, false);
    core.add_library(2, &d.path().join("music").to_string_lossy(), "music");
    let deadline = Instant::now() + Duration::from_secs(30);
    let track: i64 = loop {
        if let Ok(c) = Connection::open(data.join("library.db"))
            && let Ok(id) = c.query_row(
                "SELECT t.id FROM item t JOIN artwork w ON w.item_id = t.parent_id AND w.role = 'poster_grid'
                 WHERE t.kind = 'track' AND w.local_path IS NOT NULL",
                [],
                |r| r.get::<_, i64>(0),
            )
        {
            break id;
        }
        assert!(Instant::now() < deadline, "not scanned");
        std::thread::sleep(Duration::from_millis(100));
    };

    core.track_visuals(3, track, 0);
    let v = visuals(&rx, 3);
    let color = v["color"].as_str().unwrap().to_string();
    assert_eq!(color.len(), 7, "{v}");
    // Yellow hue (red and green above blue), dark: every channel below 0x60.
    let ch = |i: usize| u8::from_str_radix(&color[i..i + 2], 16).unwrap();
    assert!(
        ch(1) > ch(5) && ch(3) > ch(5) && ch(1) < 0x60 && ch(3) < 0x60,
        "{color}"
    );
    assert_eq!(v["peaks"].as_array().unwrap().len(), 0, "no waveform yet");

    // The job computes it and says so.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("no waveform")
        {
            Event::ItemChanged { item_id } if item_id == track => break,
            _ => {}
        }
    }
    core.track_visuals(4, track, 0);
    let v = visuals(&rx, 4);
    let peaks: Vec<u64> = v["peaks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_u64().unwrap())
        .collect();
    assert_eq!(peaks.len(), 160);
    assert!(peaks[5..75].iter().all(|p| *p > 20), "{peaks:?}");
    assert!(peaks[85..].iter().all(|p| *p < 5), "{peaks:?}");
    // The colour was kept.
    assert_eq!(v["color"], color);
}
