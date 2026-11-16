//! Issue #3 end to end against the test servers (tests/servers): save a movie from every
//! protocol and compare it byte for byte, drop the connection in the middle of a download, and
//! stop the core in the middle of one and start it again.
//! `COYOTL_TEST_SERVERS=1 cargo test -p huehuecoyotl-core --test downloads`
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const MOVIE: &str = "Movies/Alien (1979)/Alien.1979.1080p.mkv";
const SUBS: &str = "Movies/Alien (1979)/Alien.1979.1080p.en.srt";

fn enabled() -> bool {
    std::env::var_os("COYOTL_TEST_SERVERS").is_some()
}

fn fixture(rel: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/servers/fixtures")
            .join(rel),
    )
    .expect("fixtures: run tests/servers/up.sh")
}

fn wait(rx: &Receiver<Event>, what: &str, secs: u64, mut f: impl FnMut(&Event) -> bool) -> Event {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if let Ok(e) = rx.recv_timeout(Duration::from_millis(200)) {
            if f(&e) {
                return e;
            }
        }
    }
    panic!("timed out waiting for {what}");
}

fn open(dir: &Path) -> (Core, Receiver<Event>) {
    // SAFETY: set before any core opens; nothing reads it concurrently.
    unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
    let (tx, rx) = unbounded();
    let core = Core::open(dir, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    (core, rx)
}

/// Add the share's Movies folder, scan it, and return the movie's item id.
fn add_movies(core: &Core, rx: &Receiver<Event>, spec: &str) -> i64 {
    let with_path = spec.replace('}', r#","path":"/Movies"}"#);
    core.add_network_library(1, &with_path, "coyotl-test");
    let Event::Libraries { libraries, .. } = wait(rx, "library", 60, |e| {
        matches!(
            e,
            Event::Libraries { request_id: 1, .. } | Event::Error { request_id: 1, .. }
        )
    }) else {
        panic!("{spec}: adding the library failed")
    };
    let id = libraries[0].id;
    wait(
        rx,
        "scan",
        60,
        |e| matches!(e, Event::ScanState { running: false, source_id, .. } if *source_id == id),
    );
    core.browse_items(2, "movie", 0, "title", "", 0, 10);
    let Event::Page { items, .. } = wait(rx, "page", 10, |e| {
        matches!(e, Event::Page { request_id: 2, .. })
    }) else {
        unreachable!()
    };
    assert_eq!(items.len(), 1, "{spec}");
    items[0].id
}

/// Save the item and wait until the movie and its subtitles are done; return the movie's path.
fn save_and_wait(core: &Core, rx: &Receiver<Event>, item: i64, secs: u64) -> PathBuf {
    core.save_item(3, item, 0);
    let Event::Downloads { items, .. } = wait(rx, "save", 30, |e| {
        matches!(
            e,
            Event::Downloads { request_id: 3, .. } | Event::Error { request_id: 3, .. }
        )
    }) else {
        panic!("saving failed")
    };
    let id = items[0].id;
    finish(core, rx, id, secs)
}

/// Wait until copy `id` and its companions are done.
fn finish(core: &Core, rx: &Receiver<Event>, id: i64, secs: u64) -> PathBuf {
    let end = Instant::now() + Duration::from_secs(secs);
    let mut req = 1000;
    loop {
        assert!(Instant::now() < end, "download {id} did not finish");
        req += 1;
        core.downloads(req);
        let Event::Downloads { items, .. } = wait(
            rx,
            "list",
            10,
            |e| matches!(e, Event::Downloads { request_id, .. } if *request_id == req),
        ) else {
            unreachable!()
        };
        let d = items.iter().find(|d| d.id == id).expect("row");
        assert_ne!(d.state, "failed", "{d:?}");
        if d.state == "done" {
            return PathBuf::from(&d.path);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn check_copy(path: &Path, what: &str) {
    assert!(
        std::fs::read(path).unwrap() == fixture(MOVIE),
        "{what}: copy differs"
    );
    let subs = path.with_file_name("Alien.1979.1080p.en.srt");
    assert_eq!(
        std::fs::read(&subs).unwrap(),
        fixture(SUBS),
        "{what}: subtitles"
    );
    assert!(
        path.ends_with("Alien (1979)/Alien.1979.1080p.mkv"),
        "{}",
        path.display()
    );
    assert!(!path.with_extension("mkv.part").exists());
}

fn every_protocol() {
    for spec in [
        r#"{"protocol":"smb","host":"172.29.0.10","share":"media","user":"coyotl"}"#,
        r#"{"protocol":"nfs","host":"172.29.0.11","share":"/media"}"#,
        r#"{"protocol":"webdav","host":"172.29.0.12","port":8080,"user":"coyotl"}"#,
        r#"{"protocol":"ftp","host":"172.29.0.12","user":"coyotl"}"#,
        r#"{"protocol":"sftp","host":"172.29.0.12","port":2022,"user":"coyotl"}"#,
    ] {
        let d = tempfile::tempdir().unwrap();
        let (core, rx) = open(d.path());
        let item = add_movies(&core, &rx, spec);
        let path = save_and_wait(&core, &rx, item, 60);
        check_copy(&path, spec);
        assert!(path.starts_with(d.path().join("Saved")));
        // Playing the item now plays the saved copy.
        core.open_item(4, item);
        let Event::Playback { ticket, .. } = wait(&rx, "ticket", 30, |e| {
            matches!(
                e,
                Event::Playback { request_id: 4, .. } | Event::Error { request_id: 4, .. }
            )
        }) else {
            panic!("{spec}: no ticket")
        };
        assert!(!ticket.is_network, "{spec}");
        assert_eq!(PathBuf::from(&ticket.url), path);
        assert_eq!(ticket.subtitles.len(), 1);
        // Deleting removes the copy, never the server's file.
        core.delete_saved_item(5, item);
        wait(&rx, "deleted", 10, |e| {
            matches!(e, Event::Downloads { request_id: 5, .. })
        });
        assert!(!path.exists() && !path.parent().unwrap().exists());
        // Save as… from the network, then again from a saved copy.
        export(&core, &rx, item, d.path(), "network", spec);
        save_and_wait(&core, &rx, item, 60);
        export(&core, &rx, item, d.path(), "copy", spec);
        // After a restart the app holds no record of the exports.
        drop(core);
        let (core, rx) = open(d.path());
        core.downloads(9);
        let Event::Downloads { items, .. } = wait(&rx, "list", 10, |e| {
            matches!(e, Event::Downloads { request_id: 9, .. })
        }) else {
            unreachable!()
        };
        assert!(items.iter().all(|d| d.kind == "saved"), "{items:?}");
        assert!(
            d.path().join("out-copy/My Alien.mkv").exists(),
            "the user's file stays"
        );
    }
}

/// "Save as…" into `dir/out-<tag>/My Alien.mkv`: byte-identical, subtitles renamed alongside,
/// and a second export to the same place asks first.
fn export(core: &Core, rx: &Receiver<Event>, item: i64, dir: &Path, tag: &str, spec: &str) {
    let dest = dir.join(format!("out-{tag}/My Alien.mkv"));
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    core.export_item(6, item, 0, &dest.to_string_lossy(), false);
    let Event::Downloads { items, .. } = wait(rx, "export", 30, |e| {
        matches!(
            e,
            Event::Downloads { request_id: 6, .. } | Event::Error { request_id: 6, .. }
        )
    }) else {
        panic!("{spec}: export failed")
    };
    let id = items
        .iter()
        .find(|d| d.kind == "export" && Path::new(&d.path) == dest)
        .expect("export row")
        .id;
    finish(core, rx, id, 60);
    assert!(
        std::fs::read(&dest).unwrap() == fixture(MOVIE),
        "{spec} {tag}: export differs"
    );
    assert_eq!(
        std::fs::read(dest.with_file_name("My Alien.en.srt")).unwrap(),
        fixture(SUBS),
        "{spec} {tag}: subtitles"
    );
    core.export_item(7, item, 0, &dest.to_string_lossy(), false);
    let ev = wait(rx, "conflict", 10, |e| {
        matches!(
            e,
            Event::ExportConflict { request_id: 7, .. } | Event::Downloads { request_id: 7, .. }
        )
    });
    assert!(
        matches!(ev, Event::ExportConflict { ref paths, .. } if paths.len() == 1),
        "{ev:?}"
    );
    if tag != "network" {
        return; // left for the restart to forget
    }
    // Dismissing a finished export forgets it and keeps the file.
    core.delete_download(8, id);
    wait(rx, "dismissed", 10, |e| {
        matches!(e, Event::Downloads { request_id: 8, .. })
    });
    assert!(dest.exists());
}

/// A TCP proxy that counts what the server sends, can be slowed down, and can drop every
/// connection and refuse new ones for a while (a NAS rebooting).
struct Proxy {
    port: u16,
    down_until: Arc<std::sync::Mutex<Option<Instant>>>,
    generation: Arc<AtomicU64>,
    bytes: Arc<AtomicU64>,
    /// Bytes per second from the server, 0 = unlimited.
    rate: Arc<AtomicU64>,
    _stop: Arc<AtomicBool>,
}

impl Proxy {
    fn drop_for(&self, d: Duration) {
        *self.down_until.lock().unwrap() = Some(Instant::now() + d);
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

async fn pump(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    count: Option<(Arc<AtomicU64>, Arc<AtomicU64>)>,
) {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = match from.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        if let Some((bytes, rate)) = &count {
            bytes.fetch_add(n as u64, Ordering::SeqCst);
            if let Some(us) = (n as u64 * 1_000_000).checked_div(rate.load(Ordering::SeqCst)) {
                tokio::time::sleep(Duration::from_micros(us)).await;
            }
        }
        if to.write_all(&buf[..n]).await.is_err() {
            return;
        }
    }
}

fn proxy(rt: &tokio::runtime::Runtime, target: &'static str) -> Proxy {
    let l = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = l.local_addr().unwrap().port();
    let down_until: Arc<std::sync::Mutex<Option<Instant>>> = Arc::default();
    let generation = Arc::new(AtomicU64::new(0));
    let bytes = Arc::new(AtomicU64::new(0));
    let rate = Arc::new(AtomicU64::new(0));
    let (du, g, b, r) = (
        down_until.clone(),
        generation.clone(),
        bytes.clone(),
        rate.clone(),
    );
    rt.spawn(async move {
        loop {
            let Ok((inbound, _)) = l.accept().await else { return };
            if du.lock().unwrap().is_some_and(|t| Instant::now() < t) {
                drop(inbound); // refused while "down"
                continue;
            }
            let (g, b, r) = (g.clone(), b.clone(), r.clone());
            tokio::spawn(async move {
                let Ok(out) = TcpStream::connect(target).await else { return };
                let mine = g.load(Ordering::SeqCst);
                let (ir, iw) = inbound.into_split();
                let (or, ow) = out.into_split();
                tokio::select! {
                    _ = pump(ir, ow, None) => {}
                    _ = pump(or, iw, Some((b, r))) => {}
                    _ = async { while g.load(Ordering::SeqCst) == mine { tokio::time::sleep(Duration::from_millis(10)).await; } } => {}
                }
            });
        }
    });
    Proxy {
        port,
        down_until,
        generation,
        bytes,
        rate,
        _stop: Arc::default(),
    }
}

fn dropped_connection(
    rt: &tokio::runtime::Runtime,
    protocol: &str,
    target: &'static str,
    extra: &str,
) {
    let p = proxy(rt, target);
    let spec = format!(
        r#"{{"protocol":"{protocol}","host":"127.0.0.1","port":{},"user":"coyotl"{extra}}}"#,
        p.port
    );
    let d = tempfile::tempdir().unwrap();
    let (core, rx) = open(d.path());
    let item = add_movies(&core, &rx, &spec);
    let size = fixture(MOVIE).len() as u64;
    p.rate.store(1_500_000, Ordering::SeqCst);
    let before = p.bytes.load(Ordering::SeqCst);
    core.save_item(3, item, 0);
    // Halfway through, the server goes away for three seconds.
    let ev = wait(
        &rx,
        "progress",
        30,
        |e| matches!(e, Event::DownloadProgress { bytes_done, id, main_id, .. } if id == main_id && *bytes_done > size / 3),
    );
    let Event::DownloadProgress { id, .. } = ev else {
        unreachable!()
    };
    p.drop_for(Duration::from_secs(3));
    p.rate.store(0, Ordering::SeqCst);
    let path = finish(&core, &rx, id, 120);
    check_copy(&path, &spec);
    let sent = p.bytes.load(Ordering::SeqCst) - before;
    assert!(
        sent < size + size / 2,
        "{protocol}: {sent} bytes for a {size} byte file: it started over"
    );
}

fn restart(rt: &tokio::runtime::Runtime) {
    let p = proxy(rt, "172.29.0.10:445");
    let spec = format!(
        r#"{{"protocol":"smb","host":"127.0.0.1","port":{},"share":"media","user":"coyotl"}}"#,
        p.port
    );
    let d = tempfile::tempdir().unwrap();
    let size = fixture(MOVIE).len() as u64;
    let (id, kept) = {
        let (core, rx) = open(d.path());
        let item = add_movies(&core, &rx, &spec);
        p.rate.store(1_000_000, Ordering::SeqCst);
        core.save_item(3, item, 0);
        let ev = wait(
            &rx,
            "progress",
            30,
            |e| matches!(e, Event::DownloadProgress { bytes_done, id, main_id, .. } if id == main_id && *bytes_done > size / 3),
        );
        let Event::DownloadProgress { id, .. } = ev else {
            unreachable!()
        };
        drop(core); // the process goes away mid-download
        let part = d
            .path()
            .join("Saved/Alien (1979)/Alien.1979.1080p.mkv.part");
        (
            id,
            std::fs::metadata(&part)
                .expect("the partial file stays")
                .len(),
        )
    };
    assert!(kept > 0 && kept < size, "{kept}");
    p.rate.store(0, Ordering::SeqCst);
    let before = p.bytes.load(Ordering::SeqCst);
    let (core, rx) = open(d.path());
    let first = wait(
        &rx,
        "resumed",
        60,
        |e| matches!(e, Event::DownloadProgress { id: i, state, .. } if *i == id && state == "running"),
    );
    let path = finish(&core, &rx, id, 60);
    check_copy(&path, "restart");
    if let Event::DownloadProgress { bytes_done, .. } = first {
        assert!(
            bytes_done == 0 || bytes_done >= kept.min(size) - (1 << 20),
            "{bytes_done} < {kept}"
        );
    }
    let sent = p.bytes.load(Ordering::SeqCst) - before;
    assert!(
        sent < size - kept + size / 4,
        "{sent} bytes after the restart, {} were missing",
        size - kept
    );
}

/// One test: the stream resolver is process-wide, so the cores must not overlap.
#[test]
fn downloads() {
    if !enabled() {
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    every_protocol();
    dropped_connection(&rt, "smb", "172.29.0.10:445", r#","share":"media""#);
    dropped_connection(&rt, "webdav", "172.29.0.12:8080", "");
    restart(&rt);
}
