//! Issue #157: Edit tags… against the test servers (`COYOTL_TEST_SERVERS=1`): one album on a
//! writable share and a read-only one. The writable files are updated and the others reported;
//! a file changed on the server fails without damage; nothing is renamed; the library follows.
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event};
use huehuecoyotl_library::rusqlite::Connection;
use huehuecoyotl_vfs::{LocalSource, VPath};
use serde_json::{Value, json};

fn song(dir: &Path, n: u32) {
    std::fs::create_dir_all(dir).unwrap();
    let (t, tr) = (format!("title=Song {n}"), format!("track={n}"));
    let p = dir.join(format!("0{n}.flac"));
    let src = format!("sine=frequency={}:duration=1", 220 + n * 30);
    assert!(
        Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                &src,
                "-c:a",
                "flac",
                "-metadata",
                &t,
                "-metadata",
                &tr,
                "-metadata",
                "album=Lux",
                "-metadata",
                "artist=Rosalia",
                "-metadata",
                "album_artist=Rosalia",
                "-metadata",
                "date=2025",
                "-metadata",
                "REPLAYGAIN_TRACK_GAIN=-3.21 dB",
                "-metadata",
                "MUSICBRAINZ_ALBUMID=bbbbbbbb-0000-4000-8000-000000000001"
            ])
            .arg(&p)
            .status()
            .unwrap()
            .success()
    );
}

struct App {
    core: Core,
    rx: Receiver<Event>,
    next: std::cell::Cell<u64>,
}

impl App {
    fn answer(&self, id: u64) -> Result<Value, String> {
        let deadline = Instant::now() + Duration::from_secs(90);
        while let Ok(e) = self
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            match e {
                Event::MusicDetail {
                    request_id, json, ..
                } if request_id == id => return Ok(serde_json::from_str(&json).unwrap()),
                Event::Error {
                    request_id,
                    message,
                    ..
                } if request_id == id => return Err(message),
                _ => {}
            }
        }
        panic!("no answer");
    }
    fn call(&self, action: &str, args: Value) -> Value {
        let id = self.next.get();
        self.next.set(id + 1);
        self.core.library_call(id, action, &args.to_string());
        self.answer(id).unwrap_or_else(|e| panic!("{action}: {e}"))
    }
    fn share(&self, name: &str, folder: &str) {
        let id = self.next.get();
        self.next.set(id + 1);
        let spec = format!(
            r#"{{"protocol":"smb","host":"172.29.0.10","share":"{name}","user":"coyotl"}}"#
        );
        self.core.share_create(
            id,
            &spec,
            "coyotl-test",
            &json!([{ "path": folder, "content": "music" }]).to_string(),
        );
        self.answer(id).unwrap();
    }
}

async fn title(p: &Path) -> huehuecoyotl_scanner::tags::TrackTags {
    let size = std::fs::metadata(p).unwrap().len();
    huehuecoyotl_scanner::tags::read(&LocalSource, &VPath::new(p.to_string_lossy()), size)
        .await
        .unwrap()
        .tags
}

#[test]
fn a_bulk_edit_on_writable_and_read_only_shares() {
    if std::env::var_os("COYOTL_TEST_SERVERS").is_none() {
        return;
    }
    // SAFETY: set before any thread of this test process reads the environment.
    unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
    let writable =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/servers/writable");
    let name = format!("t-tag-{}", std::process::id());
    let root = writable.join(&name);
    let (a, b) = (root.join("a"), root.join("b"));
    for n in 1..=3 {
        song(&a, n);
    }
    song(&b, 1);
    {
        use std::os::unix::fs::PermissionsExt;
        let open =
            |p: &Path, m| std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
        open(&root, 0o777);
        open(&a, 0o777);
        open(&b, 0o777);
        for f in std::fs::read_dir(&a)
            .unwrap()
            .chain(std::fs::read_dir(&b).unwrap())
        {
            open(&f.unwrap().path(), 0o666);
        }
    }
    let d = tempfile::tempdir().unwrap();
    let (tx, rx) = unbounded();
    let core = Core::open(d.path(), "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    core.set_music_online(1, false);
    let app = App {
        core,
        rx,
        next: std::cell::Cell::new(100),
    };
    app.share("write", &format!("/{name}/a"));
    app.share("write-ro", &format!("/{name}/b"));
    let db = Connection::open(d.path().join("library.db")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(40);
    while db
        .query_row(
            "SELECT count(*) FROM file WHERE item_id IS NOT NULL",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
        < 4
    {
        assert!(Instant::now() < deadline, "scan");
        std::thread::sleep(Duration::from_millis(100));
    }
    let album: i64 = db
        .query_row("SELECT id FROM item WHERE kind = 'album'", [], |r| r.get(0))
        .unwrap();
    let track: i64 = db
        .query_row(
            "SELECT id FROM item WHERE kind = 'track' AND title = 'Song 2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    db.execute(
        "INSERT INTO playstate(item_id, profile_id, updated_at, play_count) VALUES (?1, 1, 1, 5)",
        [track],
    )
    .unwrap();

    let p = app.call("tagPlan", json!({ "item": album }));
    assert_eq!(p["scopes"]["albumFiles"], 4, "{p}");
    let v = app.call("tagValues", json!({ "scope": "album", "id": album }));
    assert_eq!(v["fields"]["album"]["value"], "Lux", "{v}");
    assert_eq!(v["fields"]["title"]["several"], true);
    assert_eq!(
        v["fields"]["title"]["allowed"], false,
        "a song's title is not an album's"
    );

    let edit = json!({ "scope": "album", "id": album, "changes": { "album": "LUX", "albumArtist": "Rosalía" } });
    let pv = app.call("tagPreview", edit.clone());
    assert_eq!(pv["willChange"], 4, "{pv}");
    assert!(
        pv["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["field"] == "album" && r["before"] == "Lux" && r["after"] == "LUX")
    );
    let rt = tokio::runtime::Runtime::new().unwrap();
    assert_eq!(
        rt.block_on(title(&a.join("01.flac"))).album.as_deref(),
        Some("Lux"),
        "the preview writes nothing"
    );

    // The third file changes on the server after the preview.
    let third = a.join("03.flac");
    let mut other = std::fs::read(&third).unwrap();
    other.extend_from_slice(&[0u8; 16]);
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(&third, &other).unwrap();

    let r = app.call("tagWrite", edit.clone());
    assert_eq!(r["updated"], 2, "{r}");
    assert_eq!(r["failed"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(r["failed"][0]["reason"], "changed", "{r}");
    assert_eq!(r["readOnly"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(
        std::fs::read(&third).unwrap(),
        other,
        "the changed file is left as it was"
    );
    let t = rt.block_on(title(&a.join("01.flac")));
    assert_eq!(t.album.as_deref(), Some("LUX"));
    assert_eq!(t.album_artists[0].name, "Rosalía");
    assert!(t.rg_track_gain.is_some(), "ReplayGain stays");
    assert_eq!(
        rt.block_on(title(&b.join("01.flac"))).album.as_deref(),
        Some("Lux"),
        "read-only stays"
    );
    let mut names: Vec<String> = std::fs::read_dir(&a)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["01.flac", "02.flac", "03.flac"],
        "nothing renamed, no temporary file"
    );

    // The library shows it at once, with the same items.
    let (t2, plays): (String, i64) = db.query_row(
        "SELECT a.title, p.play_count FROM item a, playstate p WHERE a.id = ?1 AND p.item_id = ?2",
        [album, track], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((t2.as_str(), plays), ("LUX", 5));
    let artist: String = db
        .query_row(
            "SELECT p.title FROM item a JOIN item p ON p.id = a.parent_id WHERE a.id = ?1",
            [album],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(artist, "Rosalía");

    // A second preview knows the read-only share.
    let pv = app.call("tagPreview", edit);
    assert_eq!(pv["readOnly"], 1, "{pv}");
    drop(app);
    let _ = std::fs::remove_dir_all(&root);
}
