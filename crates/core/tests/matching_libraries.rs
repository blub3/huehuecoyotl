//! Issues #94 to #96: matching the TV shows, music and audiobook libraries again, through the
//! public core API with fake providers (the network is never used). Shows keep their episodes'
//! watch state when their match changes; music runs skip albums the user chose and send nothing
//! while online information is off; book runs never replace the book's own metadata.
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event};
use huehuecoyotl_library::rusqlite::Connection;
use huehuecoyotl_metadata::books::{BookHit, BookProvider, BookQuery, Work};
use huehuecoyotl_metadata::music::{
    ArtistCredit, Medium, MusicProvider, Release, ReleaseHit, Track,
};
use huehuecoyotl_metadata::{
    Candidate, Details, MediaType, MetadataProvider, Result, SeasonDetails, SeasonRef,
};
use serde_json::Value;

fn ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn audio(out: &Path, codec: &str, tags: &[(&str, &str)]) {
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
        codec,
    ]);
    for (k, v) in tags {
        cmd.arg("-metadata").arg(format!("{k}={v}"));
    }
    assert!(cmd.arg(out).status().unwrap().success());
}

fn png() -> Bytes {
    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::new(60, 60))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    Bytes::from(png)
}

fn answer(rx: &Receiver<Event>, id: u64) -> Event {
    let deadline = Instant::now() + Duration::from_secs(20);
    while let Ok(e) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        match &e {
            Event::MusicDetail { request_id, .. } | Event::Error { request_id, .. }
                if *request_id == id =>
            {
                return e;
            }
            _ => {}
        }
    }
    panic!("no answer to request {id}");
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

fn finished_run(core: &Core, rx: &Receiver<Event>, library: &str, next: &mut u64) -> Value {
    poll("run finished", || {
        *next += 1;
        core.matching_status(*next);
        let Event::MusicDetail { json, .. } = answer(rx, *next) else {
            return None;
        };
        let v: Value = serde_json::from_str(&json).unwrap();
        let l = v["libraries"]
            .as_array()?
            .iter()
            .find(|l| l["library"] == library)?
            .clone();
        let state = l["run"]["state"].as_str().unwrap_or_default().to_string();
        (state == "done" || state == "stopped").then_some(l)
    })
}

fn query<T: huehuecoyotl_library::rusqlite::types::FromSql>(db: &Path, sql: &str) -> Option<T> {
    Connection::open(db)
        .ok()?
        .query_row(sql, [], |r| r.get(0))
        .ok()
}

fn open(data: &Path) -> (Core, Receiver<Event>) {
    // SAFETY: set before any thread of this test process reads the environment.
    unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
    let (tx, rx) = unbounded();
    let core = Core::open(data, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    (core, rx)
}

// ---- shows -------------------------------------------------------------------------------------

#[derive(Default)]
struct Tmdb {
    moved: AtomicBool,
}

#[async_trait]
impl MetadataProvider for Tmdb {
    async fn validate(&self) -> Result<()> {
        Ok(())
    }
    async fn search(&self, ty: MediaType, title: &str, _y: Option<u16>) -> Result<Vec<Candidate>> {
        let id = if self.moved.load(Ordering::SeqCst) {
            1438
        } else {
            1437
        };
        Ok(match (ty, title.to_lowercase().as_str()) {
            (MediaType::Show, "firefly") => vec![Candidate {
                id,
                title: "Firefly".into(),
                original_title: "Firefly".into(),
                year: Some(2002),
                popularity: 10.0,
                ..Default::default()
            }],
            _ => vec![],
        })
    }
    async fn details(&self, _ty: MediaType, id: i64) -> Result<Details> {
        Ok(Details {
            id,
            title: "Firefly".into(),
            year: Some(2002),
            seasons: vec![SeasonRef {
                season_no: 1,
                name: "Season 1".into(),
                episode_count: 2,
                ..Default::default()
            }],
            ..Default::default()
        })
    }
    async fn season(&self, _show: i64, season_no: u16) -> Result<SeasonDetails> {
        Ok(SeasonDetails {
            season_no,
            ..Default::default()
        })
    }
    async fn image(&self, _path: &str, _size: &str) -> Result<Bytes> {
        Ok(png())
    }
}

#[test]
fn a_show_keeps_its_watch_state_when_its_match_changes() {
    let d = tempfile::tempdir().unwrap();
    let media = d.path().join("media");
    for f in [
        "Firefly/Season 1/Firefly.S01E01.mkv",
        "Firefly/Season 1/Firefly.S01E02.mkv",
    ] {
        let p = media.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, f.as_bytes()).unwrap();
    }
    let data = d.path().join("data");
    let db = data.join("library.db");
    let (core, rx) = open(&data);
    let tmdb = Arc::new(Tmdb::default());
    core.set_provider(tmdb.clone());
    core.add_library(1, &media.to_string_lossy(), "video");
    poll("show matched", || {
        query::<i64>(
            &db,
            "SELECT tmdb_id FROM item WHERE kind = 'show' AND match_state = 'matched'",
        )
        .filter(|id| *id == 1437)
    });
    // The first episode was watched.
    let ep: i64 = query(
        &db,
        "SELECT id FROM item WHERE kind = 'episode' AND episode_no = 1",
    )
    .unwrap();
    Connection::open(&db)
        .unwrap()
        .execute(
            "INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at) VALUES (?1, 1, 0, 1000, 1, 1)",
            [ep],
        )
        .unwrap();
    tmdb.moved.store(true, Ordering::SeqCst);
    let mut next = 100;
    core.rematch_library(next, "shows", true, false);
    answer(&rx, next);
    let l = finished_run(&core, &rx, "shows", &mut next);
    assert_eq!(
        (l["run"]["total"].as_i64(), l["run"]["changed"].as_i64()),
        (Some(1), Some(1))
    );
    assert_eq!(l["episodes"], 2);
    assert_eq!(
        query::<i64>(&db, "SELECT tmdb_id FROM item WHERE kind = 'show'"),
        Some(1438)
    );
    let watched: Option<i64> = Connection::open(&db)
        .unwrap()
        .query_row(
            "SELECT watched FROM playstate WHERE item_id = ?1",
            [ep],
            |r| r.get(0),
        )
        .ok();
    assert_eq!(watched, Some(1), "the episode keeps its watch state");
}

// ---- music -------------------------------------------------------------------------------------

#[derive(Default)]
struct Mb {
    calls: Mutex<Vec<String>>,
    /// "Odd" is found once this is set.
    found: AtomicBool,
}

fn release(id: &str, title: &str, artist: &str) -> Release {
    Release {
        id: id.into(),
        title: title.into(),
        date: Some("1999".into()),
        artists: vec![ArtistCredit {
            name: artist.into(),
            id: format!("artist-{id}"),
            join: String::new(),
        }],
        group_id: Some(format!("group-{id}")),
        release_type: Some("album".into()),
        media: vec![Medium {
            position: 1,
            tracks: vec![Track {
                position: 1,
                title: "Song".into(),
                recording_id: format!("rec-{id}"),
                length_ms: None,
            }],
        }],
        ..Default::default()
    }
}

#[async_trait]
impl MusicProvider for Mb {
    async fn release(&self, id: &str) -> Result<Release> {
        self.calls.lock().unwrap().push(format!("release {id}"));
        Ok(if id == "odd-release" {
            release(id, "Odd", "Band")
        } else {
            release(id, "Named", "Singer")
        })
    }
    async fn search_releases(
        &self,
        artist: &str,
        title: &str,
        tracks: Option<u32>,
    ) -> Result<Vec<ReleaseHit>> {
        self.calls.lock().unwrap().push(format!("search {title}"));
        let hit = |id: &str| ReleaseHit {
            id: id.into(),
            title: title.into(),
            artist: artist.into(),
            track_count: tracks,
            group_id: Some(format!("group-{id}")),
            score: 100,
            ..Default::default()
        };
        Ok(match title {
            "Named" => vec![hit("named-release")],
            "Odd" if self.found.load(Ordering::SeqCst) => vec![hit("odd-release")],
            _ => vec![],
        })
    }
    async fn front_cover(&self, _release: &str, _group: Option<&str>) -> Result<Option<Bytes>> {
        Ok(Some(png()))
    }
}

#[test]
fn music_runs_skip_chosen_albums_and_send_nothing_offline() {
    if !ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("music");
    audio(
        &m.join("Singer/Named/01.flac"),
        "flac",
        &[
            ("artist", "Singer"),
            ("album", "Named"),
            ("title", "Song"),
            ("track", "1"),
        ],
    );
    audio(
        &m.join("Band/Odd/01.flac"),
        "flac",
        &[
            ("artist", "Band"),
            ("album", "Odd"),
            ("title", "Song"),
            ("track", "1"),
        ],
    );
    let data = d.path().join("data");
    let db = data.join("library.db");
    let (core, rx) = open(&data);
    let mb = Arc::new(Mb::default());
    core.set_music_provider(mb.clone());
    core.add_library(1, &m.to_string_lossy(), "music");
    poll("albums looked up", || {
        let named = query::<String>(
            &db,
            "SELECT match_state FROM item WHERE kind = 'album' AND title = 'Named'",
        )?;
        let odd = query::<String>(
            &db,
            "SELECT match_state FROM item WHERE kind = 'album' AND title = 'Odd'",
        )?;
        (named == "matched" && odd == "unmatched").then_some(())
    });
    // The user chose Named's match.
    Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE item SET locked = 1 WHERE kind = 'album' AND title = 'Named'",
            [],
        )
        .unwrap();
    mb.found.store(true, Ordering::SeqCst);
    mb.calls.lock().unwrap().clear();
    let mut next = 100;
    core.rematch_library(next, "music", true, false);
    answer(&rx, next);
    let l = finished_run(&core, &rx, "music", &mut next);
    assert_eq!(
        (l["run"]["total"].as_i64(), l["run"]["newly"].as_i64()),
        (Some(1), Some(1))
    );
    assert_eq!(
        query::<String>(
            &db,
            "SELECT match_state FROM item WHERE kind = 'album' AND title = 'Odd'"
        )
        .as_deref(),
        Some("matched")
    );
    let calls = mb.calls.lock().unwrap().clone();
    assert!(
        !calls.iter().any(|c| c.contains("Named")),
        "the chosen album is not asked for: {calls:?}"
    );

    // Online information off: refused, nothing sent.
    next += 1;
    core.set_music_online(next, false);
    std::thread::sleep(Duration::from_millis(300));
    mb.calls.lock().unwrap().clear();
    next += 1;
    core.rematch_library(next, "music", true, true);
    assert!(matches!(answer(&rx, next), Event::Error { .. }));
    next += 1;
    core.match_again(
        next,
        query(
            &db,
            "SELECT id FROM item WHERE kind = 'album' AND title = 'Odd'",
        )
        .unwrap(),
        true,
    );
    assert!(matches!(answer(&rx, next), Event::Error { .. }));
    std::thread::sleep(Duration::from_millis(300));
    assert!(mb.calls.lock().unwrap().is_empty());
}

// ---- books -------------------------------------------------------------------------------------

#[derive(Default)]
struct Ol {
    found: AtomicBool,
}

#[async_trait]
impl BookProvider for Ol {
    async fn search(&self, q: BookQuery<'_>) -> Result<Vec<BookHit>> {
        Ok(match q {
            BookQuery::TitleAuthor { title, .. } if self.found.load(Ordering::SeqCst) => {
                vec![BookHit {
                    id: "OL1W".into(),
                    title: title.into(),
                    authors: vec!["Plain Author".into()],
                    year: Some(1950),
                    ..Default::default()
                }]
            }
            _ => vec![],
        })
    }
    async fn work(&self, id: &str) -> Result<Work> {
        Ok(Work {
            id: id.into(),
            description: Some("Online text.".into()),
            year: Some(1950),
            ..Default::default()
        })
    }
    async fn cover(&self, _id: i64) -> Result<Option<Bytes>> {
        Ok(None)
    }
}

#[test]
fn book_runs_fill_only_what_the_book_lacks() {
    if !ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let b = d.path().join("books");
    audio(
        &b.join("Plain Author/Plain Title/01.mp3"),
        "libmp3lame",
        &[],
    );
    std::fs::write(
        b.join("Plain Author/Plain Title/desc.txt"),
        "My own description.",
    )
    .unwrap();
    let data = d.path().join("data");
    let db = data.join("library.db");
    let (core, rx) = open(&data);
    let ol = Arc::new(Ol::default());
    core.set_book_provider(ol.clone());
    core.add_library(1, &b.to_string_lossy(), "audiobooks");
    poll("book looked up", || {
        query::<String>(&db, "SELECT match_state FROM item WHERE kind = 'audiobook'")
            .filter(|s| s == "unmatched")
    });
    ol.found.store(true, Ordering::SeqCst);
    let mut next = 100;
    core.rematch_library(next, "books", false, false);
    answer(&rx, next);
    let l = finished_run(&core, &rx, "books", &mut next);
    assert_eq!(
        (l["run"]["total"].as_i64(), l["run"]["newly"].as_i64()),
        (Some(1), Some(1))
    );
    assert_eq!(l["authors"], 1);
    assert_eq!(
        query::<String>(&db, "SELECT overview FROM item WHERE kind = 'audiobook'").as_deref(),
        Some("My own description."),
        "the book's own description stays"
    );
    assert_eq!(
        query::<i64>(&db, "SELECT year FROM item WHERE kind = 'audiobook'"),
        Some(1950)
    );
}
