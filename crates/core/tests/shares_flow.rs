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
