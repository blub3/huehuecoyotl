//! Issue #12: TheAudioDB and Fanart.tv in the core, against a mock server (never the real
//! services). Without keys nothing is sent to either; a wrong key is reported when it is entered
//! and not kept; with keys an artist gets its image (Fanart.tv's first), background, logo and
//! biography, and an album without a cover gets one plus disc art; afterwards neither the logs
//! nor the database contain a key.
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event};
use huehuecoyotl_library::rusqlite::Connection;
use huehuecoyotl_metadata::Result;
use huehuecoyotl_metadata::music::{ArtistCredit, Medium, MusicProvider, Release, Track};
use serde_json::Value;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ARTIST: &str = "cc197bad-dc9c-440d-a5b5-d52ba2e14234";
const GROUP: &str = "1dc4c347-a1db-32aa-b14f-bc9cc507b843";
const TADB_KEY: &str = "tadb-secret-5e1f";
const FANART_KEY: &str = "fanart-secret-9b2c";

/// MusicBrainz stand-in: the tagged release, by the artist and in the release group above.
struct Mb;

#[async_trait]
impl MusicProvider for Mb {
    async fn release(&self, id: &str) -> Result<Release> {
        Ok(Release {
            id: id.into(),
            title: "Example Album".into(),
            date: Some("2000".into()),
            artists: vec![ArtistCredit {
                name: "Example Band".into(),
                id: ARTIST.into(),
                join: String::new(),
            }],
            group_id: Some(GROUP.into()),
            release_type: Some("album".into()),
            compilation: false,
            media: vec![Medium {
                position: 1,
                tracks: vec![Track {
                    position: 1,
                    title: "Song".into(),
                    recording_id: "rec-1".into(),
                    length_ms: None,
                    ..Default::default()
                }],
            }],
            ..Default::default()
        })
    }
}

fn ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(image::RgbaImage::new(w, h))
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn fixture(name: &str, base: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/../metadata/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
    .replace("{BASE}", base)
}

fn keys(rx: &Receiver<Event>, request: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Event::MusicKeys { request_id, json }) if request_id == request => {
                return serde_json::from_str(&json).unwrap();
            }
            Ok(Event::Error {
                request_id,
                message,
                ..
            }) if request_id == request => {
                panic!("{message}")
            }
            Ok(_) => {}
            Err(_) => panic!("no answer"),
        }
    }
}

fn poll<T>(db: &Path, what: &str, f: impl Fn(&Connection) -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(v) = f(&Connection::open(db).unwrap()) {
            return v;
        }
        if Instant::now() > deadline {
            let c = Connection::open(db).unwrap();
            let jobs: Vec<String> = c
                .prepare("SELECT kind || ' ' || state || ' ' || coalesce(last_error, '') FROM job")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap();
            let artists: Vec<String> = c
                .prepare("SELECT id || ' ' || coalesce(overview, '-') || ' ' || coalesce(meta_fetched_at, '-') FROM item WHERE kind = 'artist'")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap();
            panic!("waited in vain for {what}; jobs: {jobs:?}; artists: {artists:?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn every_file_under(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            every_file_under(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn optional_providers_with_and_without_keys() {
    if !ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(MockServer::start());
    let base = server.uri();
    rt.block_on(async {
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/json/{TADB_KEY}/artist-mb.php")))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(fixture("tadb_artist.json", &base)),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/json/{TADB_KEY}/album-mb.php")))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(fixture("tadb_album.json", &base)),
            )
            .mount(&server)
            .await;
        // Any other key: TheAudioDB's 404 page.
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/v1/json/.*"))
            .respond_with(ResponseTemplate::new(404).set_body_string("<html>404</html>"))
            .with_priority(10)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/v3/music/{ARTIST}")))
            .and(wiremock::matchers::header("api-key", FANART_KEY))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(fixture("fanart_artist.json", &base)),
            )
            .mount(&server)
            .await;
        // Fanart.tv's art for this album lacks a cover, so TheAudioDB's is used; its own disc art
        // comes from Fanart.tv.
        Mock::given(method("GET"))
            .and(path("/img/fa/thumb-best.jpg"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(png(300, 300)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/img/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(png(200, 200)))
            .with_priority(10)
            .mount(&server)
            .await;
    });

    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("music/Example Band/Example Album");
    std::fs::create_dir_all(&m).unwrap();
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
                "flac"
            ])
            .args([
                "-metadata",
                "title=Song",
                "-metadata",
                "artist=Example Band"
            ])
            .args(["-metadata", "album=Example Album", "-metadata", "track=1"])
            .args(["-metadata", "MUSICBRAINZ_ALBUMID=release-1"])
            .arg(m.join("01.flac"))
            .status()
            .unwrap()
            .success()
    );
    let data = d.path().join("data");
    let db = data.join("library.db");
    let (tx, rx) = unbounded();
    let core = Core::open(&data, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    core.set_music_provider(Arc::new(Mb));
    core.set_extra_bases(&format!("{base}/v3"), &format!("{base}/api/v1/json"));
    core.add_library(1, &d.path().join("music").to_string_lossy(), "music");

    // Without keys: the album is matched (the artist gets its MusicBrainz id), and nothing goes
    // to either optional service.
    poll(&db, "the match", |c| {
        c.query_row(
            "SELECT 1 FROM external_id WHERE kind = 'artist' AND value = ?1",
            [ARTIST],
            |r| r.get::<_, i64>(0),
        )
        .ok()
    });
    std::thread::sleep(Duration::from_millis(500));
    assert!(rt.block_on(server.received_requests()).unwrap().is_empty());
    let c = Connection::open(&db).unwrap();
    let artist_jobs: i64 = c
        .query_row(
            "SELECT count(*) FROM job WHERE kind = 'music_artist'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(artist_jobs, 0);
    drop(c);
    core.music_keys(2);
    let k = keys(&rx, 2);
    assert_eq!(
        (k["theaudiodb"].as_str(), k["fanarttv"].as_str()),
        (Some("none"), Some("none"))
    );

    // A wrong key: reported now, not kept.
    core.set_music_key(3, "theaudiodb", "wrong-key");
    let k = keys(&rx, 3);
    assert_eq!(k["theaudiodb"], "none");
    assert_eq!(k["message"], "TheAudioDB did not accept this key.");

    // Both keys: the artist is looked up.
    core.set_music_key(4, "fanarttv", FANART_KEY);
    assert_eq!(keys(&rx, 4)["fanarttv"], "keystore");
    core.set_music_key(5, "theaudiodb", TADB_KEY);
    let k = keys(&rx, 5);
    assert_eq!(k["theaudiodb"], "keystore");
    assert_eq!(k["message"], "");
    let bio: String = poll(&db, "the biography", |c| {
        c.query_row(
            "SELECT overview FROM item WHERE kind = 'artist' AND overview IS NOT NULL AND meta_fetched_at IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .ok()
    });
    assert_eq!(bio, "Example Band are a band from the test fixtures.");
    let c = Connection::open(&db).unwrap();
    let roles = |kind: &str| -> Vec<(String, i64)> {
        c.prepare(&format!(
            "SELECT w.role, w.width FROM artwork w JOIN item i ON i.id = w.item_id
             WHERE i.kind = '{kind}' AND w.local_path IS NOT NULL ORDER BY w.role"
        ))
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap()
    };
    let artist = roles("artist");
    // Fanart.tv's thumb (300 px) before TheAudioDB's; background and logo.
    assert!(artist.contains(&("poster".into(), 300)), "{artist:?}");
    assert!(artist.iter().any(|(r, _)| r == "backdrop"), "{artist:?}");
    assert!(artist.iter().any(|(r, _)| r == "logo"), "{artist:?}");
    let album = roles("album");
    assert!(album.iter().any(|(r, _)| r == "poster"), "{album:?}");
    assert!(album.iter().any(|(r, _)| r == "disc"), "{album:?}");
    drop(c);
    drop(core);

    // No key anywhere in the logs or the database.
    let mut files = Vec::new();
    every_file_under(&data, &mut files);
    for f in files {
        let bytes = std::fs::read(&f).unwrap();
        for key in [TADB_KEY, FANART_KEY] {
            assert!(
                !bytes.windows(key.len()).any(|w| w == key.as_bytes()),
                "{} contains a key",
                f.display()
            );
        }
    }
}
