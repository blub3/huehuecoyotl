//! Issues #146 and #148 through the core, with a fake MusicBrainz (the real one is never used):
//! an artist tagged "Rosalia" with Rosalía's artist ID shows MusicBrainz's spelling "Rosalía";
//! the user's own name and the files' spelling survive a rescan; a second album tagged
//! "Rosalía" joins the same artist.
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use crossbeam_channel::unbounded;
use huehuecoyotl_core::Core;
use huehuecoyotl_library::rusqlite::Connection;
use huehuecoyotl_metadata::Result;
use huehuecoyotl_metadata::music::{Artist, MusicProvider};

const ROSALIA: &str = "25f3abd9-63b5-471a-bd25-feb9672dfa11";

struct Mb;

#[async_trait]
impl MusicProvider for Mb {
    async fn artist(&self, id: &str) -> Result<Artist> {
        if id == ROSALIA {
            Ok(Artist {
                id: id.into(),
                name: "Rosalía".into(),
                score: 100,
                ..Default::default()
            })
        } else {
            Err(huehuecoyotl_metadata::MetaError::NotFound)
        }
    }
}

fn ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn flac(out: &Path, tags: &[(&str, &str)]) {
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
        "flac",
    ]);
    for (k, v) in tags {
        cmd.args(["-metadata", &format!("{k}={v}")]);
    }
    assert!(cmd.arg(out).status().unwrap().success());
}

fn q<T: huehuecoyotl_library::rusqlite::types::FromSql>(db: &Path, sql: &str) -> Option<T> {
    Connection::open(db)
        .ok()?
        .query_row(sql, [], |r| r.get(0))
        .ok()
}

fn poll<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn the_name_shown_follows_musicbrainz_and_the_user() {
    if !ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    // SAFETY: set before the core starts any thread.
    unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("music");
    flac(
        &m.join("Rosalia/LUX/01.flac"),
        &[
            ("artist", "Rosalia"),
            ("album", "LUX"),
            ("title", "One"),
            ("track", "1"),
            ("MUSICBRAINZ_ARTISTID", ROSALIA),
        ],
    );
    let data = d.path().join("data");
    let db = data.join("library.db");
    let (tx, _rx) = unbounded();
    let core = Core::open(&data, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    core.set_music_provider(std::sync::Arc::new(Mb));
    core.add_library(1, &m.to_string_lossy(), "music");
    let artist: i64 = poll("scan", || {
        q(&db, "SELECT id FROM item WHERE kind = 'artist'")
    });
    // MusicBrainz's spelling is shown; the files' is kept apart.
    poll("MusicBrainz's name", || {
        q::<String>(&db, "SELECT title FROM item WHERE kind = 'artist'").filter(|t| t == "Rosalía")
    });
    assert_eq!(
        q::<String>(&db, "SELECT files_title FROM item WHERE kind = 'artist'").unwrap(),
        "Rosalia"
    );

    // The user's own name, then the files' spelling, each surviving a rescan.
    core.set_artist_name(2, artist, "own", "ROSALÍA");
    poll("own name", || {
        q::<String>(&db, "SELECT title FROM item WHERE kind = 'artist'").filter(|t| t == "ROSALÍA")
    });
    let source: i64 = q(&db, "SELECT id FROM source WHERE content = 'music'").unwrap();
    core.rescan(source);
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        q::<String>(&db, "SELECT title FROM item WHERE kind = 'artist'").unwrap(),
        "ROSALÍA"
    );
    core.set_artist_name(3, artist, "files", "");
    poll("files' name", || {
        q::<String>(&db, "SELECT title FROM item WHERE kind = 'artist'").filter(|t| t == "Rosalia")
    });

    // A second album spelled "Rosalía" joins the same artist.
    flac(
        &m.join("Rosalía/Lux/01.flac"),
        &[
            ("artist", "Rosalía"),
            ("album", "Lux"),
            ("title", "Two"),
            ("track", "1"),
        ],
    );
    core.rescan(source);
    poll("second album", || {
        q::<i64>(&db, "SELECT count(*) FROM item WHERE kind = 'album'").filter(|n| *n == 2)
    });
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        q::<i64>(&db, "SELECT count(*) FROM item WHERE kind = 'artist'").unwrap(),
        1
    );
    assert_eq!(
        q::<String>(&db, "SELECT title FROM item WHERE kind = 'artist'").unwrap(),
        "Rosalia",
        "kept as chosen"
    );
}

/// Issue #149: two artists merged by hand stay merged through a rescan; Undo brings the other
/// back with its album.
#[test]
fn merge_by_hand_survives_a_rescan_and_undoes() {
    if !ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    // SAFETY: set before the core starts any thread.
    unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("music");
    flac(
        &m.join("A/One/01.flac"),
        &[
            ("artist", "Beyonce"),
            ("album", "One"),
            ("title", "A"),
            ("track", "1"),
        ],
    );
    flac(
        &m.join("B/Two/01.flac"),
        &[
            ("artist", "Beyoncé Knowles"),
            ("album", "Two"),
            ("title", "B"),
            ("track", "1"),
        ],
    );
    let data = d.path().join("data");
    let db = data.join("library.db");
    let (tx, rx) = unbounded();
    let core = Core::open(&data, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    core.set_music_online(1, false);
    core.add_library(2, &m.to_string_lossy(), "music");
    poll("scan", || {
        q::<i64>(&db, "SELECT count(*) FROM item WHERE kind = 'artist'").filter(|n| *n == 2)
    });
    let keep: i64 = q(
        &db,
        "SELECT id FROM item WHERE kind = 'artist' AND title = 'Beyonce'",
    )
    .unwrap();
    let drop: i64 = q(
        &db,
        "SELECT id FROM item WHERE kind = 'artist' AND title = 'Beyoncé Knowles'",
    )
    .unwrap();
    core.artist_merge(3, keep, drop);
    poll("merged", || {
        q::<i64>(&db, "SELECT count(*) FROM item WHERE kind = 'artist'").filter(|n| *n == 1)
    });
    // A new album tagged with the merged artist's old name joins the kept artist.
    flac(
        &m.join("B/Three/01.flac"),
        &[
            ("artist", "Beyoncé Knowles"),
            ("album", "Three"),
            ("title", "C"),
            ("track", "1"),
        ],
    );
    let source: i64 = q(&db, "SELECT id FROM source WHERE content = 'music'").unwrap();
    core.rescan(source);
    poll("rescanned", || {
        q::<i64>(&db, "SELECT count(*) FROM item WHERE kind = 'album'").filter(|n| *n == 3)
    });
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        q::<i64>(&db, "SELECT count(*) FROM item WHERE kind = 'artist'").unwrap(),
        1,
        "still merged"
    );
    assert_eq!(
        q::<i64>(
            &db,
            &format!("SELECT count(*) FROM item WHERE kind = 'album' AND parent_id = {keep}")
        )
        .unwrap(),
        3
    );
    core.artist_merge_undo(4);
    poll("undone", || {
        q::<i64>(&db, "SELECT count(*) FROM item WHERE kind = 'artist'").filter(|n| *n == 2)
    });
    assert_eq!(
        q::<i64>(
            &db,
            &format!("SELECT count(*) FROM item WHERE kind = 'album' AND parent_id = {drop}")
        )
        .unwrap(),
        1
    );
    drop_events(rx);
}

fn drop_events(rx: crossbeam_channel::Receiver<huehuecoyotl_core::Event>) {
    while rx.try_recv().is_ok() {}
}
