//! Issue #11: music folders are looked up online through the music provider (a fake here, the
//! network is never used): an album tagged with its release id costs exactly one lookup; a
//! well-named album is matched by a search; an uncertain one goes to review with its tags kept;
//! covers come from the provider only for albums without one; with music information limited to
//! file tags, the provider is never asked.
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use crossbeam_channel::unbounded;
use huehuecoyotl_core::{Core, Event};
use huehuecoyotl_library::rusqlite::Connection;
use huehuecoyotl_metadata::Result;
use huehuecoyotl_metadata::music::{
    ArtistCredit, Medium, MusicProvider, Release, ReleaseHit, Track,
};

#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<String>>,
}

fn release(id: &str, title: &str, artist: &str, n: u32) -> Release {
    Release {
        id: id.into(),
        title: title.into(),
        date: Some("1999-03-01".into()),
        artists: vec![ArtistCredit {
            name: artist.into(),
            id: format!("artist-{id}"),
            join: String::new(),
        }],
        group_id: Some(format!("group-{id}")),
        release_type: Some("album".into()),
        compilation: false,
        media: vec![Medium {
            position: 1,
            tracks: (1..=n)
                .map(|p| Track {
                    position: p,
                    title: format!("Song {p}"),
                    recording_id: format!("rec-{id}-{p}"),
                    length_ms: None,
                })
                .collect(),
        }],
    }
}

#[async_trait]
impl MusicProvider for Fake {
    async fn release(&self, id: &str) -> Result<Release> {
        self.calls.lock().unwrap().push(format!("release {id}"));
        Ok(match id {
            "tagged-release" => release(id, "Tagged", "Band", 2),
            _ => release(id, "Named", "Singer", 2),
        })
    }
    async fn search_releases(
        &self,
        artist: &str,
        title: &str,
        tracks: Option<u32>,
    ) -> Result<Vec<ReleaseHit>> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("search {artist} / {title} / {tracks:?}"));
        let hit = |id: &str, group: &str| ReleaseHit {
            id: id.into(),
            title: title.into(),
            artist: artist.into(),
            track_count: tracks,
            group_id: Some(group.into()),
            score: 100,
            ..Default::default()
        };
        Ok(match title {
            "Named" => vec![hit("named-release", "g1")],
            // Two different albums by that name: for review.
            _ => vec![hit("live-a", "g1"), hit("live-b", "g2")],
        })
    }
    async fn front_cover(&self, release: &str, _group: Option<&str>) -> Result<Option<Bytes>> {
        self.calls.lock().unwrap().push(format!("cover {release}"));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::new(600, 600))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        Ok(Some(Bytes::from(png)))
    }
}

fn ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn flac(dir: &Path, rel: &str, tags: &[(&str, &str)]) {
    let out = dir.join(rel);
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
        cmd.arg("-metadata").arg(format!("{k}={v}"));
    }
    assert!(cmd.arg(&out).status().unwrap().success());
}

/// (title, match state) of the albums, polled until `done` says so.
fn albums(db: &Path, done: impl Fn(&[(String, String)]) -> bool) -> Vec<(String, String)> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let c = Connection::open(db).unwrap();
        let rows: Vec<(String, String)> = c
            .prepare("SELECT title, coalesce(match_state, '') FROM item WHERE kind = 'album' ORDER BY title")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        if done(&rows) || Instant::now() > deadline {
            return rows;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn open(data: &Path, fake: Arc<Fake>) -> (Core, crossbeam_channel::Receiver<Event>) {
    let (tx, rx) = unbounded();
    let core = Core::open(data, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    core.set_music_provider(fake);
    (core, rx)
}

#[test]
fn albums_are_looked_up_online() {
    if !ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("music");
    for n in 1..=2 {
        flac(
            &m,
            &format!("Band/Tagged/0{n}.flac"),
            &[
                ("title", &format!("Song {n}")),
                ("artist", "Band"),
                ("album_artist", "Band"),
                ("album", "Tagged"),
                ("track", &n.to_string()),
                ("MUSICBRAINZ_ALBUMID", "tagged-release"),
            ],
        );
        flac(
            &m,
            &format!("Singer/Named (1999)/0{n} - Song {n}.flac"),
            &[
                ("title", &format!("Song {n}")),
                ("artist", "Singer"),
                ("album", "Named"),
                ("track", &n.to_string()),
            ],
        );
        flac(
            &m,
            &format!("Band/Live/0{n}.flac"),
            &[
                ("title", &format!("Live {n}")),
                ("artist", "Band"),
                ("album", "Live"),
                ("track", &n.to_string()),
            ],
        );
    }
    // "Named" has a cover of its own: the provider is not asked for one.
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::new(64, 64));
    img.save(m.join("Singer/Named (1999)/cover.png")).unwrap();

    let fake = Arc::new(Fake::default());
    let data = d.path().join("data");
    let (core, _rx) = open(&data, fake.clone());
    core.add_library(1, &m.to_string_lossy(), "music");
    let db = data.join("library.db");
    let rows = albums(&db, |r| {
        r.len() == 3 && r.iter().all(|(_, s)| s != "unmatched")
    });
    assert_eq!(
        rows,
        [
            ("Live".to_string(), "needs_review".to_string()),
            ("Named".into(), "matched".into()),
            ("Tagged".into(), "matched".into()),
        ]
    );
    // Let the covers land.
    std::thread::sleep(Duration::from_millis(500));
    drop(core);
    let calls = fake.calls.lock().unwrap().clone();
    // The tagged album: one lookup, no search.
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.contains("tagged-release") && c.starts_with("release"))
            .count(),
        1
    );
    assert!(!calls.iter().any(|c| c.starts_with("search Band / Tagged")));
    // The named album: a search with its artist, title and track count, then the lookup.
    assert!(
        calls.contains(&"search Singer / Named / Some(2)".to_string()),
        "{calls:?}"
    );
    assert!(calls.contains(&"release named-release".to_string()));
    // Covers: only for albums without one.
    assert!(calls.contains(&"cover tagged-release".to_string()));
    assert!(!calls.contains(&"cover named-release".to_string()));

    let c = Connection::open(&db).unwrap();
    let q = |sql: &str| -> Vec<String> {
        c.prepare(sql)
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    };
    // What the release added: its type, date, group, the artist's and the tracks' ids.
    assert_eq!(
        q(
            "SELECT m.release_type || ' ' || m.release_date FROM item a JOIN music m ON m.item_id = a.id WHERE a.title = 'Tagged'"
        ),
        ["album 1999-03-01"]
    );
    assert_eq!(
        q(
            "SELECT e.value FROM external_id e JOIN item a ON a.id = e.item_id WHERE a.title = 'Tagged' AND e.kind = 'release_group'"
        ),
        ["group-tagged-release"]
    );
    assert_eq!(
        q(
            "SELECT e.value FROM external_id e JOIN item a ON a.id = e.item_id WHERE a.title = 'Band' AND e.kind = 'artist'"
        ),
        ["artist-tagged-release"]
    );
    assert_eq!(
        q(
            "SELECT e.value FROM external_id e JOIN item t ON t.id = e.item_id JOIN item a ON a.id = t.parent_id WHERE a.title = 'Tagged' AND e.kind = 'recording' ORDER BY e.value"
        ),
        ["rec-tagged-release-1", "rec-tagged-release-2"]
    );
    assert_eq!(
        q(
            "SELECT count(*) || '' FROM artwork w JOIN item a ON a.id = w.item_id WHERE a.title = 'Tagged' AND w.local_path IS NOT NULL"
        ),
        ["3"]
    );
    // The uncertain album keeps its tags and has two candidates for review.
    assert_eq!(
        q("SELECT title FROM item WHERE kind = 'track' AND title LIKE 'Live%' ORDER BY title"),
        ["Live 1", "Live 2"]
    );
    assert_eq!(
        q(
            "SELECT count(*) || '' FROM match_candidate c JOIN item a ON a.id = c.item_id WHERE a.title = 'Live'"
        ),
        ["2"]
    );
}

#[test]
fn tags_only_sends_nothing() {
    if !ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("music");
    flac(
        &m,
        "Band/Album/01.flac",
        &[
            ("title", "One"),
            ("artist", "Band"),
            ("album", "Album"),
            ("MUSICBRAINZ_ALBUMID", "tagged-release"),
        ],
    );
    let fake = Arc::new(Fake::default());
    let data = d.path().join("data");
    let (core, rx) = open(&data, fake.clone());
    core.set_music_online(5, false);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(Event::MusicOnline {
            request_id: 5,
            online,
        }) = rx.recv_timeout(Duration::from_millis(200))
        {
            assert!(!online);
            break;
        }
    }
    core.add_library(1, &m.to_string_lossy(), "music");
    let rows = albums(&data.join("library.db"), |r| !r.is_empty());
    assert_eq!(rows.len(), 1);
    std::thread::sleep(Duration::from_secs(2));
    drop(core);
    assert!(
        fake.calls.lock().unwrap().is_empty(),
        "{:?}",
        fake.calls.lock().unwrap()
    );
}
