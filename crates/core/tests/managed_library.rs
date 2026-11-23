//! Issue #150: the managed library through the public core API. On local folders: choosing a
//! place, its four folders, moving the copies, starting empty and the refusals. With the test
//! servers (`COYOTL_TEST_SERVERS=1`): a library on the writable share, a read-only share refused,
//! and a move that survives a dropped connection without losing a file.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event};
use huehuecoyotl_library::rusqlite::Connection;
use serde_json::{Value, json};

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

struct Lib {
    core: Core,
    rx: Receiver<Event>,
    data: PathBuf,
    next: std::cell::Cell<u64>,
}

impl Lib {
    fn new(data: &Path) -> Self {
        let (core, rx) = open(data);
        Lib {
            core,
            rx,
            data: data.to_path_buf(),
            next: std::cell::Cell::new(100),
        }
    }

    fn call(&self, action: &str, args: Value) -> Result<Value, String> {
        let id = self.next.get();
        self.next.set(id + 1);
        self.core.library_call(id, action, &args.to_string());
        let deadline = Instant::now() + Duration::from_secs(60);
        while let Ok(e) = self
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            match e {
                Event::MusicDetail {
                    request_id, json, ..
                } if request_id == id => {
                    return Ok(serde_json::from_str(&json).unwrap());
                }
                Event::Error {
                    request_id,
                    message,
                    ..
                } if request_id == id => {
                    return Err(message);
                }
                _ => {}
            }
        }
        panic!("no answer to {action}");
    }

    fn db(&self) -> Connection {
        Connection::open(self.data.join("library.db")).unwrap()
    }

    fn wait_moved(&self, secs: u64) {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let s = self.call("status", json!({})).unwrap();
            if s["moving"] == false {
                return;
            }
            assert!(Instant::now() < deadline, "still moving: {s}");
            std::thread::sleep(Duration::from_millis(150));
        }
    }

    fn wait_files(&self, n: i64) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let s = self.call("status", json!({})).unwrap();
            if s["copies"] == n {
                return;
            }
            assert!(Instant::now() < deadline, "copies: {s}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn local(path: &Path) -> Value {
    json!({ "where": "local", "path": path.to_string_lossy() })
}

fn with(mut v: Value, mode: &str) -> Value {
    v["mode"] = json!(mode);
    v
}

fn blob(seed: u32, len: usize) -> Vec<u8> {
    (0..len as u32)
        .map(|i| (i.wrapping_mul(seed) % 251) as u8)
        .collect()
}

#[test]
fn a_library_on_this_computer() {
    let d = tempfile::tempdir().unwrap();
    let l = Lib::new(&d.path().join("data"));
    let (a, b, c) = (d.path().join("a"), d.path().join("b"), d.path().join("c"));
    for p in [&a, &b, &c] {
        std::fs::create_dir_all(p).unwrap();
    }
    assert_eq!(l.call("status", json!({})).unwrap()["configured"], false);

    // A folder that does not exist fails its check and changes nothing.
    let v = l.call("check", local(&d.path().join("nope"))).unwrap();
    assert_eq!(v["ok"], false);
    assert!(
        l.call("apply", with(local(&d.path().join("nope")), "move"))
            .is_err()
    );

    let v = l.call("check", local(&a)).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(
        std::fs::read_dir(&a).unwrap().count(),
        0,
        "the check leaves nothing"
    );
    let v = l.call("apply", with(local(&a), "move")).unwrap();
    assert_eq!(v["configured"], true);
    for f in ["Movies", "TV Shows", "Music", "Audiobooks"] {
        assert!(a.join(f).is_dir(), "{f}");
    }

    // Settings per type are kept.
    let v = l
        .call(
            "setType",
            json!({ "type": "music", "changes": { "import": "auto" } }),
        )
        .unwrap();
    assert_eq!(v["types"]["music"]["import"], "auto");
    assert_eq!(v["types"]["movies"]["import"], "manual");

    // The library's own folders cannot be removed as sources.
    let src: i64 = l
        .db()
        .query_row("SELECT id FROM source WHERE managed = 'movies'", [], |r| {
            r.get(0)
        })
        .unwrap();
    l.core.remove_library(90, src);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut refused = false;
    while let Ok(e) =
        l.rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        if let Event::Error { request_id: 90, .. } = e {
            refused = true;
            break;
        }
    }
    assert!(refused);

    // Two copies, scanned as a library folder.
    let dir = a.join("Movies/Film (2000)");
    std::fs::create_dir_all(&dir).unwrap();
    let (one, two) = (blob(7, 2_500_000), blob(11, 1000));
    std::fs::write(dir.join("Film (2000).mkv"), &one).unwrap();
    std::fs::create_dir_all(a.join("Movies/Other (2001)")).unwrap();
    std::fs::write(a.join("Movies/Other (2001)/Other (2001).mkv"), &two).unwrap();
    l.core.rescan(src);
    l.wait_files(2);
    let file: i64 = l
        .db()
        .query_row("SELECT id FROM file WHERE path LIKE 'Film%'", [], |r| {
            r.get(0)
        })
        .unwrap();

    // Move: the same rows, the files at the new place, nothing left at the old one.
    assert_eq!(l.call("check", local(&b)).unwrap()["copies"], 2);
    l.call("apply", with(local(&b), "move")).unwrap();
    l.wait_moved(30);
    assert_eq!(
        std::fs::read(b.join("Movies/Film (2000)/Film (2000).mkv")).unwrap(),
        one
    );
    assert_eq!(
        std::fs::read(b.join("Movies/Other (2001)/Other (2001).mkv")).unwrap(),
        two
    );
    assert!(!a.join("Movies/Film (2000)").exists());
    let (again, managed): (i64, String) = l
        .db()
        .query_row(
            "SELECT f.id, s.managed FROM file f JOIN source s ON s.id = f.source_id WHERE f.path LIKE 'Film%'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((again, managed.as_str()), (file, "movies"));
    assert_eq!(
        l.db()
            .query_row(
                "SELECT count(*) FROM source WHERE root LIKE ?1",
                [format!("{}%", a.display())],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );

    // The same place, and a place inside an ordinary folder, are refused.
    assert_eq!(l.call("check", local(&b)).unwrap()["ok"], false);

    // Start empty: the old copies stay where they are, as an ordinary folder.
    l.call("apply", with(local(&c), "empty")).unwrap();
    assert!(b.join("Movies/Film (2000)/Film (2000).mkv").is_file());
    let (m, n): (Option<String>, i64) = l
        .db()
        .query_row(
            "SELECT s.managed, count(*) FROM file f JOIN source s ON s.id = f.source_id WHERE f.path LIKE 'Film%'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((m, n), (None, 1));
    assert_eq!(l.call("status", json!({})).unwrap()["copies"], 0);
    assert_eq!(
        l.call("check", local(&b.join("Movies"))).unwrap()["ok"],
        false
    );
}

#[test]
fn originals_can_go_and_copies_stay() {
    let d = tempfile::tempdir().unwrap();
    let l = Lib::new(&d.path().join("data"));
    let (lib, nas) = (d.path().join("lib"), d.path().join("nas"));
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::create_dir_all(nas.join("Kept (2000)")).unwrap();
    std::fs::create_dir_all(nas.join("Only (2001)")).unwrap();
    std::fs::write(nas.join("Kept (2000)/Kept (2000).mkv"), blob(3, 900)).unwrap();
    std::fs::write(nas.join("Only (2001)/Only (2001).mkv"), blob(5, 800)).unwrap();
    l.call("apply", with(local(&lib), "move")).unwrap();
    l.core.add_library(91, &nas.to_string_lossy(), "video");
    let source = {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let got: Option<i64> = l
                .db()
                .query_row(
                    "SELECT source_id FROM file WHERE path LIKE 'Only%' LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .ok();
            if let Some(s) = got {
                let n: i64 = l
                    .db()
                    .query_row("SELECT count(*) FROM file WHERE source_id = ?1", [s], |r| {
                        r.get(0)
                    })
                    .unwrap();
                if n == 2 {
                    break s;
                }
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    // A library copy of the first (what Add to Library will make): the same item.
    let c = l.db();
    let (lib_source, item): (i64, i64) = c
        .query_row(
            "SELECT (SELECT id FROM source WHERE managed = 'movies'), item_id FROM file WHERE path LIKE 'Kept%'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    std::fs::create_dir_all(lib.join("Movies/Kept (2000)")).unwrap();
    std::fs::write(lib.join("Movies/Kept (2000)/Kept (2000).mkv"), blob(3, 900)).unwrap();
    c.execute(
        "INSERT INTO file(source_id, path, size, item_id) VALUES (?1, 'Kept (2000)/Kept (2000).mkv', 900, ?2)",
        [lib_source, item],
    )
    .unwrap();
    c.execute(
        "INSERT INTO playstate(item_id, profile_id, updated_at, play_count) VALUES (?1, 1, 1, 4)",
        [item],
    )
    .unwrap();
    let p = l
        .call("removePreview", json!({ "source": source }))
        .unwrap();
    assert_eq!(
        (p["stay"].as_i64(), p["leave"].as_i64()),
        (Some(1), Some(1))
    );

    // Both originals are deleted; the rescan keeps the one with a library copy.
    std::fs::remove_dir_all(nas.join("Kept (2000)")).unwrap();
    std::fs::remove_dir_all(nas.join("Only (2001)")).unwrap();
    std::fs::create_dir_all(nas.join("Third (2002)")).unwrap();
    std::fs::write(nas.join("Third (2002)/Third (2002).mkv"), blob(9, 700)).unwrap();
    l.core.rescan(source);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let g = l.call("gone", json!({ "source": source })).unwrap();
        if g["total"] == 2 {
            assert_eq!((g["kept"].as_i64(), g["left"].as_i64()), (Some(1), Some(1)));
            break;
        }
        assert!(Instant::now() < deadline, "{g}");
        std::thread::sleep(Duration::from_millis(100));
    }
    let plays: i64 = c
        .query_row(
            "SELECT play_count FROM playstate WHERE item_id = ?1",
            [item],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(plays, 4);
    let o = l.call("itemOrigin", json!({ "item": item })).unwrap();
    assert_eq!(o["libraryOnly"], true, "{o}");
    let only: i64 = c
        .query_row("SELECT count(*) FROM item WHERE title = 'Only'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(only, 0);

    // Removing the source keeps library items, and deletes no file on it.
    l.core.remove_library(92, source);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if c.query_row("SELECT count(*) FROM source WHERE id = ?1", [source], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
            == 0
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let kept: i64 = c
        .query_row("SELECT count(*) FROM item WHERE id = ?1", [item], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(kept, 1);
    assert!(nas.join("Third (2002)/Third (2002).mkv").is_file());
}

/// A TCP relay whose connections can be cut.
fn relay(rt: &tokio::runtime::Runtime, target: &'static str, down: Arc<AtomicBool>) -> u16 {
    let l = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let port = l.local_addr().unwrap().port();
    rt.spawn(async move {
        loop {
            let Ok((mut a, _)) = l.accept().await else {
                return;
            };
            if down.load(Ordering::SeqCst) {
                continue;
            }
            let down = down.clone();
            tokio::spawn(async move {
                let Ok(mut b) = tokio::net::TcpStream::connect(target).await else {
                    return;
                };
                let copy = tokio::io::copy_bidirectional(&mut a, &mut b);
                let cut = async {
                    while !down.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                };
                tokio::select! { _ = copy => {} _ = cut => {} }
            });
        }
    });
    port
}

fn share(l: &Lib, spec: &str, seed: &str) -> i64 {
    let id = l.next.get();
    l.next.set(id + 1);
    let folders = json!([{ "path": seed, "content": "video" }]).to_string();
    l.core.share_create(id, spec, "coyotl-test", &folders);
    let deadline = Instant::now() + Duration::from_secs(30);
    while let Ok(e) =
        l.rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        match e {
            Event::MusicDetail {
                request_id, json, ..
            } if request_id == id => {
                let v: Value = serde_json::from_str(&json).unwrap();
                return v["shares"][0]["id"].as_i64().unwrap();
            }
            Event::Error {
                request_id,
                message,
                ..
            } if request_id == id => panic!("{message}"),
            _ => {}
        }
    }
    panic!("no share");
}

#[test]
fn a_library_on_a_share() {
    if std::env::var_os("COYOTL_TEST_SERVERS").is_none() {
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let down = Arc::new(AtomicBool::new(false));
    let port = relay(&rt, "172.29.0.10:445", down.clone());
    let writable = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/servers/writable");
    let name = format!("t-lib-{}", std::process::id());
    let (root, seed) = (writable.join(&name), writable.join(format!("{name}-seed")));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&seed).unwrap();
    for p in [&root, &seed] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o777)).unwrap();
    }
    let d = tempfile::tempdir().unwrap();
    let l = Lib::new(&d.path().join("data"));
    let spec = format!(
        r#"{{"protocol":"smb","host":"127.0.0.1","port":{port},"share":"write","user":"coyotl"}}"#
    );
    let sid = share(&l, &spec, &format!("/{name}-seed"));
    let ro = r#"{"protocol":"smb","host":"172.29.0.10","share":"media","user":"coyotl"}"#;
    let ro_id = {
        let id = 95;
        l.core.share_create(
            id,
            ro,
            "coyotl-test",
            r#"[{"path":"/Movies","content":"video"}]"#,
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let e =
                l.rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .unwrap();
            if let Event::MusicDetail {
                request_id: 95,
                json,
                ..
            } = e
            {
                let v: Value = serde_json::from_str(&json).unwrap();
                let list = v["shares"].as_array().unwrap();
                break list.iter().find(|s| s["share"] == "media").unwrap()["id"]
                    .as_i64()
                    .unwrap();
            }
        }
    };

    // A read-only share is refused before anything is written.
    let at = |s: i64, p: &str| json!({ "where": "share", "share": s, "path": p });
    let v = l.call("check", at(ro_id, "/Shows")).unwrap();
    assert_eq!(v["ok"], false, "{v}");
    let w = v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["key"] == "writable")
        .unwrap()
        .clone();
    assert_eq!(w["detail"], "readOnly", "{v}");
    assert!(l.call("apply", with(at(ro_id, "/Shows"), "move")).is_err());

    // Three copies on this computer move to the share.
    let here = d.path().join("here");
    std::fs::create_dir_all(&here).unwrap();
    l.call("apply", with(local(&here), "move")).unwrap();
    let src: i64 = l
        .db()
        .query_row("SELECT id FROM source WHERE managed = 'movies'", [], |r| {
            r.get(0)
        })
        .unwrap();
    let bodies: Vec<(String, Vec<u8>)> = (1..=3)
        .map(|i| (format!("Film {i} (200{i})"), blob(13 + i, 3_000_000)))
        .collect();
    for (n, b) in &bodies {
        std::fs::create_dir_all(here.join("Movies").join(n)).unwrap();
        std::fs::write(here.join("Movies").join(n).join(format!("{n}.mkv")), b).unwrap();
    }
    l.core.rescan(src);
    l.wait_files(3);
    let target = at(sid, &format!("/{name}"));
    let v = l.call("check", target.clone()).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    l.call("apply", with(target, "move")).unwrap();

    // The connection drops during the move.
    let deadline = Instant::now() + Duration::from_secs(20);
    while l.call("status", json!({})).unwrap()["moveLeft"]
        .as_i64()
        .unwrap()
        == 3
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    down.store(true, Ordering::SeqCst);
    std::thread::sleep(Duration::from_secs(2));
    // Nothing is lost: each film is complete in one of the two places.
    for (n, b) in &bodies {
        let old = here.join("Movies").join(n).join(format!("{n}.mkv"));
        let new = root.join("Movies").join(n).join(format!("{n}.mkv"));
        let ok = std::fs::read(&old).is_ok_and(|x| x == *b)
            || std::fs::read(&new).is_ok_and(|x| x == *b);
        assert!(ok, "{n} is complete nowhere");
    }
    down.store(false, Ordering::SeqCst);
    l.wait_moved(120);
    for (n, b) in &bodies {
        let new = root.join("Movies").join(n).join(format!("{n}.mkv"));
        assert!(std::fs::read(&new).unwrap() == *b, "{n}");
        assert!(!here.join("Movies").join(n).exists(), "{n} left behind");
    }
    let hidden = walk(&root)
        .into_iter()
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with('.'))
        .count();
    assert_eq!(hidden, 0, "temporary files left");
    drop(l);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&seed);
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}
