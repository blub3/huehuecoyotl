//! Issues #101 to #107 through the public core API with a fake MusicBrainz (the network is never
//! used): an album's match brings its unmatched artist along, a matched artist its other albums,
//! never a locked artist; a song matched on its own makes its own unmatched artist follow but not
//! the album artist; artists of one name are told apart by the user's albums; an album is
//! compared with a release track by track.
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event};
use huehuecoyotl_library::rusqlite::Connection;
use huehuecoyotl_metadata::Result;
use huehuecoyotl_metadata::music::{
    Artist, ArtistCredit, Medium, MusicProvider, Recording, Release, ReleaseHit, Track,
};
use serde_json::Value;

#[derive(Default)]
struct Mb {
    calls: Mutex<Vec<String>>,
}

fn credit(name: &str, id: &str) -> ArtistCredit {
    ArtistCredit {
        name: name.into(),
        id: id.into(),
        join: String::new(),
    }
}

#[async_trait]
impl MusicProvider for Mb {
    async fn release(&self, id: &str) -> Result<Release> {
        self.calls.lock().unwrap().push(format!("release {id}"));
        let (title, artist, aid) = match id {
            "winter" => ("Winter Rooms", "Halden Quartet", "a1"),
            _ => ("Locked Songs", "Locked Band", "lb"),
        };
        Ok(Release {
            id: id.into(),
            title: title.into(),
            date: Some("2019".into()),
            artists: vec![credit(artist, aid)],
            group_id: Some(format!("g-{id}")),
            release_type: Some("album".into()),
            media: vec![Medium {
                position: 1,
                tracks: vec![
                    Track {
                        position: 1,
                        title: "Northern Lights".into(),
                        recording_id: format!("{id}-r1"),
                        length_ms: Some(300),
                        ..Default::default()
                    },
                    Track {
                        position: 2,
                        title: "Thaw".into(),
                        recording_id: format!("{id}-r2"),
                        length_ms: Some(300),
                        ..Default::default()
                    },
                    Track {
                        position: 3,
                        title: "Bonus".into(),
                        recording_id: format!("{id}-r3"),
                        length_ms: Some(300),
                        ..Default::default()
                    },
                ],
            }],
            ..Default::default()
        })
    }
    async fn search_releases(
        &self,
        artist: &str,
        title: &str,
        tracks: Option<u32>,
    ) -> Result<Vec<ReleaseHit>> {
        self.calls.lock().unwrap().push(format!("search {title}"));
        let id = match title {
            "Winter Rooms" => "winter",
            "Locked Songs" => "locked",
            _ => return Ok(vec![]),
        };
        Ok(vec![ReleaseHit {
            id: id.into(),
            title: title.into(),
            artist: artist.into(),
            track_count: tracks,
            group_id: Some(format!("g-{id}")),
            score: 100,
            ..Default::default()
        }])
    }
    async fn search_recordings(&self, title: &str, _artist: &str) -> Result<Vec<Recording>> {
        Ok(if title == "Odd Song" {
            vec![Recording {
                id: "odd-rec".into(),
                title: "Odd Song".into(),
                artists: vec![credit("Guest Singer", "g1")],
                length_ms: Some(300),
                score: 100,
                ..Default::default()
            }]
        } else {
            vec![]
        })
    }
    async fn recording(&self, id: &str) -> Result<Recording> {
        Ok(Recording {
            id: id.into(),
            title: "Odd Song".into(),
            artists: vec![credit("Guest Singer", "g1")],
            isrcs: vec!["XX0000000001".into()],
            length_ms: Some(300),
            score: 100,
            ..Default::default()
        })
    }
    async fn search_artists(&self, name: &str) -> Result<Vec<Artist>> {
        Ok(if name == "Halden Quartet" {
            ["a1", "a2"]
                .iter()
                .map(|id| Artist {
                    id: (*id).into(),
                    name: name.into(),
                    disambiguation: if *id == "a1" {
                        "string quartet".into()
                    } else {
                        "jazz quartet".into()
                    },
                    score: 100,
                    ..Default::default()
                })
                .collect()
        } else {
            vec![]
        })
    }
    async fn artist_groups(&self, id: &str) -> Result<Vec<String>> {
        Ok(if id == "a1" {
            vec!["Winter Rooms".into()]
        } else {
            vec![]
        })
    }
    async fn front_cover(&self, _r: &str, _g: Option<&str>) -> Result<Option<Bytes>> {
        Ok(None)
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
        cmd.arg("-metadata").arg(format!("{k}={v}"));
    }
    assert!(cmd.arg(out).status().unwrap().success());
}

fn answer(rx: &Receiver<Event>, id: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    while let Ok(e) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        match e {
            Event::MusicDetail {
                request_id, json, ..
            } if request_id == id => return serde_json::from_str(&json).unwrap(),
            Event::Error {
                request_id,
                message,
                ..
            } if request_id == id => panic!("error: {message}"),
            _ => {}
        }
    }
    panic!("no answer to {id}");
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

fn mbid(db: &Path, artist: &str) -> Option<String> {
    q(
        db,
        &format!(
            "SELECT e.value FROM external_id e JOIN item a ON a.id = e.item_id WHERE a.kind = 'artist' AND a.title = '{artist}' AND e.provider = 'musicbrainz' AND e.kind = 'artist'"
        ),
    )
}

#[test]
fn songs_artists_and_following() {
    if !ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    // SAFETY: set before any thread of this test process reads the environment.
    unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("music");
    for (n, t) in [(1, "Northern Lights"), (2, "Thaw")] {
        flac(
            &m.join(format!("Halden Quartet/Winter Rooms/0{n}.flac")),
            &[
                ("artist", "Halden Quartet"),
                ("album", "Winter Rooms"),
                ("title", t),
                ("track", &n.to_string()),
            ],
        );
    }
    flac(
        &m.join("Halden Quartet/First Light/01.flac"),
        &[
            ("artist", "Halden Quartet"),
            ("album", "First Light"),
            ("title", "Dawn"),
            ("track", "1"),
        ],
    );
    flac(
        &m.join("Locked Band/Locked Songs/01.flac"),
        &[
            ("artist", "Locked Band"),
            ("album", "Locked Songs"),
            ("title", "Northern Lights"),
            ("track", "1"),
        ],
    );
    flac(
        &m.join("Band/Odd/01.flac"),
        &[
            ("artist", "Guest Singer"),
            ("album_artist", "Band"),
            ("album", "Odd"),
            ("title", "Odd Song"),
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
    let mb = Arc::new(Mb::default());
    core.set_music_provider(mb.clone());
    // Online off while scanning, so the user can lock an artist before anything is looked up.
    core.set_music_online(1, false);
    std::thread::sleep(Duration::from_millis(300));
    core.add_library(2, &m.to_string_lossy(), "music");
    poll("scan", || {
        q::<i64>(&db, "SELECT count(*) FROM item WHERE kind = 'album'").filter(|n| *n == 4)
    });
    Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE item SET locked = 1 WHERE kind = 'artist' AND title = 'Locked Band'",
            [],
        )
        .unwrap();
    core.set_music_online(3, true);

    // #106: Winter Rooms' release names Halden Quartet: the artist follows, through the album,
    // and its other album is looked up again. The locked artist does not follow.
    poll("artist follows", || {
        mbid(&db, "Halden Quartet").filter(|v| v == "a1")
    });
    poll("Locked Songs matched", || {
        q::<String>(
            &db,
            "SELECT match_state FROM item WHERE kind = 'album' AND title = 'Locked Songs'",
        )
        .filter(|s| s == "matched")
    });
    assert_eq!(
        mbid(&db, "Locked Band"),
        None,
        "a locked artist is never changed by an album"
    );
    let halden: i64 = q(
        &db,
        "SELECT id FROM item WHERE kind = 'artist' AND title = 'Halden Quartet'",
    )
    .unwrap();
    core.artist_detail(10, halden);
    let a = answer(&rx, 10);
    assert_eq!(a["matched"], true);
    assert_eq!(a["matchedVia"]["kind"], "album");
    assert_eq!(a["matchedVia"]["title"], "Winter Rooms");
    // #102: Winter Rooms against its release: 2 of 2 tracks agree, 1 extra on the release.
    let winter: i64 = q(
        &db,
        "SELECT id FROM item WHERE kind = 'album' AND title = 'Winter Rooms'",
    )
    .unwrap();
    core.match_preview(11, winter, r#"{"id":"winter","kind":"release"}"#);
    let p = answer(&rx, 11);
    assert_eq!(
        (
            p["agree"].as_i64(),
            p["total"].as_i64(),
            p["extra"].as_i64()
        ),
        (Some(2), Some(2), Some(1))
    );

    // #103 and #107: the song on the unmatched album finds its recording; its own artist follows,
    // the album artist does not.
    let song: i64 = q(
        &db,
        "SELECT id FROM item WHERE kind = 'track' AND title = 'Odd Song'",
    )
    .unwrap();
    core.match_again(12, song, false);
    assert_eq!(answer(&rx, 12)["result"], "newly");
    assert_eq!(
        q::<String>(
            &db,
            &format!("SELECT value FROM external_id WHERE item_id = {song} AND kind = 'recording'")
        )
        .as_deref(),
        Some("odd-rec")
    );
    assert_eq!(mbid(&db, "Guest Singer").as_deref(), Some("g1"));
    assert_eq!(mbid(&db, "Band"), None);
    // #104: the search compares lengths; the choice is locked.
    core.match_search(13, song, r#"{"text":""}"#);
    let s = answer(&rx, 13);
    assert_eq!(s["candidates"][0]["current"], true);
    assert!(
        s["candidates"][0]["fit"]
            .as_str()
            .unwrap()
            .starts_with("lengthAgrees")
    );
    core.match_choose(14, song, r#"{"id":"odd-rec","kind":"recording"}"#);
    answer(&rx, 14);
    assert_eq!(
        q::<i64>(&db, &format!("SELECT locked FROM item WHERE id = {song}")),
        Some(1)
    );

    // #105: two artists share the name; the one with the user's album is the match.
    core.match_again(15, halden, false);
    assert_eq!(answer(&rx, 15)["result"], "unchanged");
    core.match_search(16, halden, r#"{"text":""}"#);
    let s = answer(&rx, 16);
    let c = s["candidates"].as_array().unwrap();
    assert_eq!(c.len(), 2);
    assert_eq!(
        (
            c[0]["albums"].as_i64(),
            c[0]["ofAlbums"].as_i64(),
            c[0]["current"].as_bool()
        ),
        (Some(1), Some(2), Some(true))
    );
    assert_eq!(c[1]["albums"], 0);
    // First Light was tried once before; a matched artist brings it along again.
    poll("First Light tried", || {
        q::<String>(
            &db,
            "SELECT match_state FROM item WHERE kind = 'album' AND title = 'First Light'",
        )
        .filter(|s| s == "unmatched")
    });
    let before = mb
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| *c == "search First Light")
        .count();
    core.match_choose(17, halden, r#"{"id":"a2","kind":"artist"}"#);
    answer(&rx, 17);
    poll("First Light looked up again", || {
        (mb.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| *c == "search First Light")
            .count()
            > before)
            .then_some(())
    });
    assert_eq!(mbid(&db, "Halden Quartet").as_deref(), Some("a2"));
    core.artist_detail(18, halden);
    let a = answer(&rx, 18);
    assert_eq!(
        (a["locked"].as_bool(), a["matchedVia"].is_null()),
        (Some(true), true)
    );
}
