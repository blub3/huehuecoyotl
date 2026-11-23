//! Issue #118: network shares through the public core API. Without servers: a share's password
//! moves from its folders' old keystore entries to the share's own at start-up (the in-memory
//! keystore of this process; the real one is never used). With the test servers (tests/servers,
//! `COYOTL_TEST_SERVERS=1`): two folders of one SMB share, added one after the other, become one
//! share with one password; removing a folder keeps the password; removing the share removes it.
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event, secrets};
use huehuecoyotl_library::rusqlite::Connection;
use serde_json::Value;

/// The tests share this process's in-memory keystore, and every library numbers its shares from
/// 1, so they run one at a time.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // Never the real keystore: the test's in-memory one, before anything touches it.
    // SAFETY: set while holding the lock, before this test opens a core.
    unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
    // What an earlier test left in the keystore.
    for i in 1..=8 {
        secrets::delete_password(&format!("share:{i}"));
        secrets::delete_password(&format!("source:{i}"));
    }
    guard
}

fn open(data: &std::path::Path) -> (Core, Receiver<Event>) {
    // SAFETY: set before any thread of this test process reads the environment.
    unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
    let (tx, rx) = unbounded();
    let core = Core::open(data, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    (core, rx)
}

fn answer(rx: &Receiver<Event>, id: u64) -> Event {
    let deadline = Instant::now() + Duration::from_secs(60);
    while let Ok(e) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        match &e {
            Event::MusicDetail { request_id, .. }
            | Event::Error { request_id, .. }
            | Event::Libraries { request_id, .. }
                if *request_id == id =>
            {
                return e;
            }
            _ => {}
        }
    }
    panic!("no answer to {id}");
}

fn shares(core: &Core, rx: &Receiver<Event>, id: u64) -> Value {
    core.shares(id);
    match answer(rx, id) {
        Event::MusicDetail { json, .. } => serde_json::from_str(&json).unwrap(),
        e => panic!("{e:?}"),
    }
}

#[test]
fn passwords_move_to_the_share() {
    let _serial = serial();
    let d = tempfile::tempdir().unwrap();
    let data = d.path().join("data");
    drop(open(&data));
    // A share with two folders from before shares: the newer folder's password wins.
    let c = Connection::open(data.join("library.db")).unwrap();
    c.execute_batch(
        r#"INSERT INTO share(id, kind, name, config_json, credential_ref)
             VALUES (1, 'smb', 'media on nas', '{"protocol":"smb","host":"nas","share":"media","user":"me"}', 'share:1');
           INSERT INTO source(id, kind, name, config_json, root, content, share_id, credential_ref) VALUES
             (2, 'smb', 'Movies', '{}', '/Movies', 'video', 1, 'share:1'),
             (3, 'smb', 'Shows', '{}', '/Shows', 'video', 1, 'share:1');"#,
    )
    .unwrap();
    drop(c);
    secrets::store_password("source:2", "old-pass").unwrap();
    secrets::store_password("source:3", "new-pass").unwrap();
    let (core, rx) = open(&data);
    // The start-up has run once a request is answered after it.
    let v = shares(&core, &rx, 1);
    assert_eq!(v["shares"][0]["folders"].as_array().unwrap().len(), 2);
    let deadline = Instant::now() + Duration::from_secs(10);
    while secrets::load_password("share:1").is_empty() {
        assert!(Instant::now() < deadline, "password moved");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(secrets::load_password("share:1"), "new-pass");
    assert_eq!(
        secrets::load_password("source:3"),
        "",
        "the same password's old entry goes"
    );
    assert_eq!(
        secrets::load_password("source:2"),
        "old-pass",
        "a different one is kept until the share's works"
    );
}

#[test]
fn two_folders_one_share() {
    let _serial = serial();
    if std::env::var_os("COYOTL_TEST_SERVERS").is_none() {
        eprintln!("skipped: set COYOTL_TEST_SERVERS=1 with tests/servers running");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let (core, rx) = open(&d.path().join("data"));
    let spec = r#"{"protocol":"smb","host":"172.29.0.10","share":"media","user":"coyotl","path":"/Movies"}"#;
    core.add_network_library(1, spec, "coyotl-test");
    assert!(matches!(answer(&rx, 1), Event::Libraries { .. }));
    // The same server and share, written in capitals, joins the share.
    let spec2 = r#"{"protocol":"smb","host":"172.29.0.10","share":"MEDIA","user":"coyotl","path":"/Shows","name":"Shows"}"#;
    core.add_network_library(2, spec2, "coyotl-test");
    assert!(matches!(answer(&rx, 2), Event::Libraries { .. }));
    let v = shares(&core, &rx, 3);
    let list = v["shares"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{v}");
    let share = list[0]["id"].as_i64().unwrap();
    let folders = list[0]["folders"].as_array().unwrap();
    assert_eq!(folders.len(), 2);
    assert_eq!(
        secrets::load_password(&format!("share:{share}")),
        "coyotl-test"
    );

    // Issue #120: browsing the share shows both folders as in the library, with a guess; a new
    // share's settings browse the same way, and a folder inside an added one says so.
    core.share_browse(40, &format!(r#"{{"share":{share}}}"#), "", "/");
    let b = match answer(&rx, 40) {
        Event::MusicDetail { kind, json, .. } if kind == "shareFolders" => {
            serde_json::from_str::<Value>(&json).unwrap()
        }
        e => panic!("{e:?}"),
    };
    let names: Vec<_> = b["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Movies", "Shows"], "{b}");
    for f in b["folders"].as_array().unwrap() {
        assert_eq!(f["guess"], "video", "{f}");
        assert_eq!(f["inLibrary"], serde_json::json!(["video"]), "{f}");
        assert!(f["looked"].as_bool().unwrap());
    }
    assert_eq!(b["folders"][0]["videos"], 1);
    core.share_browse(41, spec, "coyotl-test", "/Shows");
    let b = match answer(&rx, 41) {
        Event::MusicDetail { json, .. } => serde_json::from_str::<Value>(&json).unwrap(),
        e => panic!("{e:?}"),
    };
    assert_eq!(
        b["share"], share,
        "a new share's settings find the share in the library"
    );
    assert_eq!(b["parent"], "/");
    assert_eq!(b["folders"][0]["name"], "Firefly");
    assert_eq!(b["folders"][0]["inside"], "Shows", "{b}");
    assert_eq!(b["folders"][0]["videos"], 2);

    // Adding a folder that is not there fails; adding one twice adds nothing.
    core.share_add_folders(4, share, r#"[{"path":"/Nothing","content":"video"}]"#);
    assert!(matches!(answer(&rx, 4), Event::Error { .. }));
    core.share_add_folders(5, share, r#"[{"path":"/Movies","content":"video"}]"#);
    answer(&rx, 5);
    assert_eq!(
        shares(&core, &rx, 6)["shares"][0]["folders"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // Removing a folder keeps the share's password; removing the share takes it along.
    let first = folders[0]["sourceId"].as_i64().unwrap();
    core.remove_library(7, first);
    answer(&rx, 7);
    assert_eq!(
        secrets::load_password(&format!("share:{share}")),
        "coyotl-test"
    );
    core.share_remove(8, share);
    let v = match answer(&rx, 8) {
        Event::MusicDetail { json, .. } => serde_json::from_str::<Value>(&json).unwrap(),
        e => panic!("{e:?}"),
    };
    assert!(v["shares"].as_array().unwrap().is_empty());
    assert_eq!(secrets::load_password(&format!("share:{share}")), "");
}

/// Issue #121 against the test servers: signing in to the SMB server lists its shares; a share
/// is created with its first folders in one call; signing in again finds it; a create whose
/// folders fail keeps neither a share nor a password.
#[test]
fn add_a_share_in_steps() {
    let _serial = serial();
    if std::env::var_os("COYOTL_TEST_SERVERS").is_none() {
        eprintln!("skipped: set COYOTL_TEST_SERVERS=1 with tests/servers running");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let (core, rx) = open(&d.path().join("data"));
    let detail = |e: Event| match e {
        Event::MusicDetail { json, .. } => serde_json::from_str::<Value>(&json).unwrap(),
        e => panic!("{e:?}"),
    };
    let server = r#"{"protocol":"smb","host":"172.29.0.10","user":"coyotl"}"#;
    core.share_sign_in(1, server, "wrong");
    assert!(matches!(answer(&rx, 1), Event::Error { .. }));
    core.share_sign_in(2, server, "coyotl-test");
    let v = detail(answer(&rx, 2));
    assert_eq!(
        v["shares"],
        serde_json::json!([
            { "name": "media", "existing": null },
            { "name": "write", "existing": null },
            { "name": "write-ro", "existing": null }
        ])
    );

    // A failing folder keeps nothing.
    let spec = r#"{"protocol":"smb","host":"172.29.0.10","share":"media","user":"coyotl"}"#;
    core.share_create(
        3,
        spec,
        "coyotl-test",
        r#"[{"path":"/Nothing","content":"video"}]"#,
    );
    assert!(matches!(answer(&rx, 3), Event::Error { .. }));
    assert!(
        shares(&core, &rx, 4)["shares"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(secrets::load_password("share:1"), "");

    core.share_sign_in(5, spec, "coyotl-test");
    let v = detail(answer(&rx, 5));
    assert_eq!(v["connected"], true);
    assert_eq!(v["folders"], 2);
    assert!(v["existing"].is_null());
    core.share_create(
        6,
        spec,
        "coyotl-test",
        r#"[{"path":"/Movies","content":"video"},{"path":"/Shows","content":"video"}]"#,
    );
    let v = detail(answer(&rx, 6));
    let list = v["shares"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{v}");
    assert_eq!(list[0]["folders"].as_array().unwrap().len(), 2);
    let id = list[0]["id"].as_i64().unwrap();
    assert_eq!(
        secrets::load_password(&format!("share:{id}")),
        "coyotl-test"
    );

    core.share_sign_in(7, server, "coyotl-test");
    assert_eq!(detail(answer(&rx, 7))["shares"][0]["existing"], id);
    core.share_sign_in(8, spec, "coyotl-test");
    assert_eq!(detail(answer(&rx, 8))["existing"], id);
}

/// Issue #122 against the test servers: a share's address changes and its folder, items and
/// watched state stay; a wrong password keeps the old one; folders missing at the new place are
/// named and nothing is saved; a share changed to another's settings takes over its folders;
/// changing a folder's type moves its items.
#[test]
fn edit_a_share() {
    let _serial = serial();
    if std::env::var_os("COYOTL_TEST_SERVERS").is_none() {
        eprintln!("skipped: set COYOTL_TEST_SERVERS=1 with tests/servers running");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let (core, rx) = open(&d.path().join("data"));
    let detail = |e: Event| match e {
        Event::MusicDetail { json, .. } => serde_json::from_str::<Value>(&json).unwrap(),
        e => panic!("{e:?}"),
    };
    let scanned = |source: i64| {
        let end = Instant::now() + Duration::from_secs(60);
        while Instant::now() < end {
            if let Ok(Event::ScanState {
                running: false,
                source_id,
                ..
            }) = rx.recv_timeout(Duration::from_millis(200))
                && source_id == source
            {
                return;
            }
        }
        panic!("no scan of {source}");
    };
    let movies = |id: u64| {
        core.browse_items(id, "movie", 0, "title", "", 0, 50);
        let end = Instant::now() + Duration::from_secs(30);
        while Instant::now() < end {
            if let Ok(Event::Page {
                request_id, items, ..
            }) = rx.recv_timeout(Duration::from_millis(200))
                && request_id == id
            {
                return items;
            }
        }
        panic!("no page");
    };

    let plain = r#"{"protocol":"smb","host":"172.29.0.10","share":"media","user":"coyotl"}"#;
    let port =
        r#"{"protocol":"smb","host":"172.29.0.10","port":445,"share":"media","user":"coyotl"}"#;
    core.add_network_library(
        1,
        &plain.replace('}', r#","path":"/Movies"}"#),
        "coyotl-test",
    );
    let Event::Libraries { libraries, .. } = answer(&rx, 1) else {
        panic!()
    };
    let folder = libraries[0].id;
    scanned(folder);
    let items = movies(2);
    assert_eq!(items.len(), 1);
    let movie = items[0].id;
    core.set_watched(movie, true);
    let share = shares(&core, &rx, 3)["shares"][0]["id"].as_i64().unwrap();
    let pass = || secrets::load_password(&format!("share:{share}"));

    // A wrong password: an error, the old password stays.
    core.share_edit(4, share, port, "wrong", true);
    assert!(matches!(answer(&rx, 4), Event::Error { .. }));
    assert_eq!(pass(), "coyotl-test");

    // Folders missing at the new place: named, nothing saved.
    let dav = r#"{"protocol":"webdav","host":"172.29.0.12","port":8080,"share":"/Shows","user":"coyotl"}"#;
    core.share_edit(5, share, dav, "", true);
    let v = detail(answer(&rx, 5));
    assert_eq!(v["ok"], false);
    assert_eq!(v["saved"], false);
    assert_eq!(v["missing"].as_array().unwrap().len(), 1, "{v}");
    assert_eq!(shares(&core, &rx, 6)["shares"][0]["protocol"], "smb");

    // A test, then the new address saved: the same folder, the movie still watched.
    core.share_edit(7, share, port, "", false);
    let v = detail(answer(&rx, 7));
    assert_eq!(
        (v["ok"].as_bool(), v["saved"].as_bool()),
        (Some(true), Some(false)),
        "{v}"
    );
    core.share_edit(8, share, port, "", true);
    assert_eq!(detail(answer(&rx, 8))["saved"], true);
    let v = shares(&core, &rx, 9);
    assert_eq!(v["shares"][0]["port"], 445);
    assert_eq!(v["shares"][0]["folders"][0]["sourceId"], folder);
    let items = movies(10);
    assert_eq!(items[0].id, movie);
    assert!(items[0].watched);
    assert_eq!(pass(), "coyotl-test");

    // A second share of the same server without the port; changing the first back joins them.
    core.add_network_library(
        11,
        &plain.replace('}', r#","path":"/Shows","name":"Shows"}"#),
        "coyotl-test",
    );
    answer(&rx, 11);
    let v = shares(&core, &rx, 12);
    assert_eq!(v["shares"].as_array().unwrap().len(), 2, "{v}");
    let other = v["shares"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] != share)
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    core.share_edit(13, share, plain, "", true);
    let v = detail(answer(&rx, 13));
    assert_eq!(v["joins"]["id"], other, "{v}");
    let v = shares(&core, &rx, 14);
    let list = v["shares"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{v}");
    assert_eq!(list[0]["id"], share);
    assert_eq!(list[0]["folders"].as_array().unwrap().len(), 2);
    assert_eq!(secrets::load_password(&format!("share:{other}")), "");

    // Movies holds music now: its movie goes, the folder is scanned as music.
    core.share_folder_content(15, folder, "music");
    let v = detail(answer(&rx, 15));
    let f = v["shares"][0]["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["root"] == "/Movies")
        .unwrap()
        .clone();
    assert_eq!(f["content"], "music");
    assert_ne!(f["sourceId"], folder);
    scanned(f["sourceId"].as_i64().unwrap());
    assert!(movies(16).is_empty());
}
