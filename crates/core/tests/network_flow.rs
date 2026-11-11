//! M3 end to end against the test servers (tests/servers): add a share as a library, scan it,
//! open a movie and read it through the mpv stream callbacks, byte for byte.
//! `COYOTL_TEST_SERVERS=1 cargo test -p huehuecoyotl-core --test network_flow`
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, ErrorKind, Event};
use huehuecoyotl_stream::{MpvStreamCbInfo, coyotl_stream_open};

const MOVIE: &str = "tests/servers/fixtures/Movies/Alien (1979)/Alien.1979.1080p.mkv";

/// Progress on stderr (the test runs with --nocapture in CI), so a hang shows where it is.
fn step(spec: &str, what: &str) {
    let proto = spec
        .split("\"protocol\":\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap_or("?");
    eprintln!(
        "[network_flow {:>7.1}s] {proto}: {what}",
        started().elapsed().as_secs_f32()
    );
}

fn started() -> Instant {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn wait(rx: &Receiver<Event>, what: &str, mut f: impl FnMut(&Event) -> bool) -> Event {
    let end = Instant::now() + Duration::from_secs(60);
    while Instant::now() < end {
        if let Ok(e) = rx.recv_timeout(Duration::from_millis(200)) {
            if f(&e) {
                return e;
            }
        }
    }
    panic!("timed out waiting for {what}");
}

fn read_all(url: &str) -> Vec<u8> {
    let uri = std::ffi::CString::new(url).unwrap();
    let mut info = MpvStreamCbInfo {
        cookie: std::ptr::null_mut(),
        read_fn: None,
        seek_fn: None,
        size_fn: None,
        close_fn: None,
        cancel_fn: None,
    };
    assert_eq!(
        unsafe { coyotl_stream_open(std::ptr::null_mut(), uri.as_ptr() as *mut _, &mut info) },
        0,
        "open {url}"
    );
    let (read, seek, close) = (
        info.read_fn.unwrap(),
        info.seek_fn.unwrap(),
        info.close_fn.unwrap(),
    );
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = unsafe { read(info.cookie, buf.as_mut_ptr().cast(), buf.len() as u64) };
        assert!(n >= 0, "read error");
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    // A seek back (index probing) reads the right bytes too.
    assert_eq!(unsafe { seek(info.cookie, 1000) }, 1000);
    let n = unsafe { read(info.cookie, buf.as_mut_ptr().cast(), 100) };
    assert_eq!(&buf[..n as usize], &out[1000..1000 + n as usize]);
    unsafe { close(info.cookie) };
    out
}

fn flow(spec: &str) {
    // SAFETY: set before any core opens; nothing reads it concurrently.
    unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
    let reference =
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../").to_string() + MOVIE)
            .expect("fixtures: run tests/servers/up.sh");
    step(spec, "open core");
    let d = tempfile::tempdir().unwrap();
    let (tx, rx) = unbounded();
    let core = Core::open(d.path(), "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();

    // Wrong password: a typed error, nothing added.
    if !spec.contains("\"nfs\"") {
        step(spec, "wrong password");
        core.browse_network(1, spec, "wrong", "");
        let e = wait(&rx, "auth error", |e| {
            matches!(e, Event::Error { request_id: 1, .. })
        });
        assert!(
            matches!(
                e,
                Event::Error {
                    kind: ErrorKind::Auth,
                    ..
                }
            ),
            "{spec}: {e:?}"
        );
    }

    // The folder picker.
    step(spec, "folder picker");
    core.browse_network(2, spec, "coyotl-test", "/");
    let Event::Folder { entries, .. } = wait(&rx, "folder", |e| {
        matches!(e, Event::Folder { request_id: 2, .. })
            || matches!(e, Event::Error { request_id: 2, .. })
    }) else {
        panic!("{spec}: listing failed")
    };
    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Movies", "Shows"], "{spec}");
    // SFTP: the dialog shows the host key it is about to trust.
    let key = core.network_host_key();
    assert_eq!(
        key.starts_with("SHA256:"),
        spec.contains("\"sftp\""),
        "{spec}: {key}"
    );

    // Add the Movies folder and wait for its scan.
    let with_path = spec.replace('}', r#","path":"/Movies"}"#);
    step(spec, "add library");
    core.add_network_library(3, &with_path, "coyotl-test");
    let Event::Libraries { libraries, .. } = wait(&rx, "libraries", |e| {
        matches!(e, Event::Libraries { request_id: 3, .. })
            || matches!(e, Event::Error { request_id: 3, .. })
    }) else {
        panic!("{spec}: add failed")
    };
    let lib = &libraries[0];
    assert!(lib.root.ends_with("/Movies"), "{}", lib.root);
    assert!(lib.online && lib.kind != "local");
    wait(
        &rx,
        "scan end",
        |e| matches!(e, Event::ScanState { running: false, source_id, .. } if *source_id == lib.id),
    );

    step(spec, "browse movies");
    core.browse_items(4, "movie", 0, "title", false, 0, 50);
    let Event::Page { items, .. } = wait(&rx, "page", |e| {
        matches!(e, Event::Page { request_id: 4, .. })
    }) else {
        unreachable!()
    };
    assert_eq!(items.len(), 1, "{spec}: {items:?}");

    step(spec, "open item");
    core.open_item(5, items[0].id);
    let ev = wait(&rx, "ticket", |e| {
        matches!(e, Event::Playback { request_id: 5, .. })
            || matches!(e, Event::Error { request_id: 5, .. })
    });
    let Event::Playback { ticket, .. } = ev else {
        panic!("{spec}: {ev:?}")
    };
    assert!(ticket.is_network);
    assert_eq!(
        ticket.url,
        format!(
            "coyotl://{}/Movies/Alien%20(1979)/Alien.1979.1080p.mkv",
            lib.id
        )
    );
    assert_eq!(ticket.subtitles.len(), 1);
    assert_eq!(ticket.subtitles[0].lang, "eng");

    step(spec, "stream the movie");
    assert!(
        read_all(&ticket.url) == reference,
        "{spec}: streamed bytes differ"
    );
    assert_eq!(
        read_all(&ticket.subtitles[0].url).len(),
        std::fs::metadata(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/servers/fixtures/Movies/Alien (1979)/Alien.1979.1080p.en.srt"
        ))
        .unwrap()
        .len() as usize
    );

    step(spec, "stream the subtitle done; remove library");
    core.remove_library(6, lib.id);
    let Event::Libraries { libraries, .. } = wait(&rx, "removed", |e| {
        matches!(e, Event::Libraries { request_id: 6, .. })
    }) else {
        unreachable!()
    };
    assert!(libraries.is_empty());
    step(spec, "close core");
    drop(core);
    step(spec, "done");
}

fn enabled() -> bool {
    std::env::var_os("COYOTL_TEST_SERVERS").is_some()
}

/// One test: the stream resolver is process-wide, so the cores must not overlap.
#[test]
fn network_libraries() {
    if !enabled() {
        return;
    }
    // A hang anywhere fails the test after three minutes with the last step printed above,
    // instead of running into the CI step's 20-minute timeout.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(180));
        eprintln!("network_flow: not finished after 3 minutes; the last step above is where it hangs");
        std::process::exit(101);
    });
    for spec in [
        r#"{"protocol":"smb","host":"172.29.0.10","share":"media","user":"coyotl"}"#,
        r#"{"protocol":"nfs","host":"172.29.0.11","share":"/media"}"#,
        r#"{"protocol":"webdav","host":"172.29.0.12","port":8080,"user":"coyotl"}"#,
        r#"{"protocol":"ftp","host":"172.29.0.12","user":"coyotl"}"#,
        r#"{"protocol":"sftp","host":"172.29.0.12","port":2022,"user":"coyotl"}"#,
    ] {
        flow(spec);
    }
}
