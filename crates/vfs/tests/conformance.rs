//! Protocol conformance suite (SPEC 6, Testing): the same checks against every backend,
//! compared byte for byte with the fixture files, plus wrong credentials, missing paths,
//! unreachable hosts and connections dropped mid-stream.
//!
//! Needs the test servers: `tests/servers/up.sh`, then
//! `COYOTL_TEST_SERVERS=1 cargo test -p huehuecoyotl-vfs --test conformance`.
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use huehuecoyotl_vfs::{Protocol, RemoteConfig, VPath, VfsError, remote};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};

const MOVIE: &str = "/Movies/Alien (1979)/Alien.1979.1080p.mkv";

fn enabled() -> bool {
    std::env::var_os("COYOTL_TEST_SERVERS").is_some()
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/servers/fixtures")
}

fn cfg(protocol: Protocol) -> RemoteConfig {
    let (host, port, share) = match protocol {
        Protocol::Smb => ("172.29.0.10", None, "media"),
        Protocol::Nfs => ("172.29.0.11", None, "media"),
        Protocol::Webdav => ("172.29.0.12", Some(8080), ""),
        Protocol::Ftp => ("172.29.0.12", None, ""),
        Protocol::Sftp => ("172.29.0.12", Some(2022), ""),
    };
    RemoteConfig {
        protocol,
        host: host.into(),
        port,
        share: share.into(),
        user: if protocol == Protocol::Nfs {
            String::new()
        } else {
            "coyotl".into()
        },
        password: "coyotl-test".into(),
        tls: false,
        host_key: None,
    }
}

async fn check(protocol: Protocol) {
    let name = protocol.as_str();
    let reference = std::fs::read(fixtures().join(MOVIE.trim_start_matches('/')))
        .expect("fixtures: run tests/servers/up.sh");
    let src = remote::connect(cfg(protocol))
        .await
        .unwrap_or_else(|e| panic!("{name}: connect: {e}"));

    // Listing
    let root = src.list(&VPath::new("/")).await.unwrap();
    let mut names: Vec<_> = root.iter().map(|e| (e.name.as_str(), e.is_dir)).collect();
    names.sort();
    assert_eq!(
        names,
        [("Movies", true), ("Shows", true), ("notes.txt", false)],
        "{name}: root listing"
    );
    let dir = src.list(&VPath::new("/Movies/Alien (1979)")).await.unwrap();
    let movie = dir
        .iter()
        .find(|e| e.name == "Alien.1979.1080p.mkv")
        .unwrap_or_else(|| panic!("{name}: movie missing in {dir:?}"));
    assert_eq!(
        movie.size,
        reference.len() as u64,
        "{name}: size in listing"
    );
    assert_eq!(movie.path.as_str(), MOVIE);
    assert!(movie.mtime > 1_600_000_000, "{name}: mtime {}", movie.mtime);

    // Stat
    let st = src.stat(&VPath::new(MOVIE)).await.unwrap();
    assert_eq!(
        (st.size, st.is_dir),
        (reference.len() as u64, false),
        "{name}: stat"
    );

    // Ranged reads: start, middle, across the end, past the end
    for (off, len) in [
        (0u64, 4096u32),
        (1_000_003, 65_536),
        (reference.len() as u64 - 100, 4096),
    ] {
        let b = src.read_at(&VPath::new(MOVIE), off, len).await.unwrap();
        let end = (off as usize + len as usize).min(reference.len());
        assert!(
            b[..] == reference[off as usize..end],
            "{name}: read {off}+{len}"
        );
    }
    let past = src
        .read_at(&VPath::new(MOVIE), reference.len() as u64 + 10, 100)
        .await
        .unwrap();
    assert!(past.is_empty(), "{name}: read past EOF");

    // Sequential read of the whole file in player-sized pieces
    let mut got = Vec::new();
    let mut off = 0u64;
    loop {
        let b = src
            .read_at(&VPath::new(MOVIE), off, 256 * 1024)
            .await
            .unwrap();
        if b.is_empty() {
            break;
        }
        off += b.len() as u64;
        got.extend_from_slice(&b);
    }
    assert!(got == reference, "{name}: sequential read differs");

    // Errors are typed
    let missing = src.list(&VPath::new("/Nope")).await.unwrap_err();
    assert!(
        matches!(missing, VfsError::NotFound(_)),
        "{name}: missing folder gave {missing:?}"
    );
    if protocol != Protocol::Nfs {
        let mut bad = cfg(protocol);
        bad.password = "wrong".into();
        let e = remote::connect(bad)
            .await
            .err()
            .expect("wrong password accepted");
        assert!(
            matches!(e, VfsError::Auth(_)),
            "{name}: wrong password gave {e:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn smb_conformance() {
    if enabled() {
        check(Protocol::Smb).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn nfs_conformance() {
    if enabled() {
        check(Protocol::Nfs).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn webdav_conformance() {
    if enabled() {
        check(Protocol::Webdav).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn ftp_conformance() {
    if enabled() {
        check(Protocol::Ftp).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn unreachable_host_is_typed() {
    let mut c = cfg(Protocol::Webdav);
    c.host = "127.0.0.1".into();
    c.port = Some(9); // discard port: nothing listens
    let e = remote::connect(c)
        .await
        .err()
        .expect("connected to nothing");
    assert!(matches!(e, VfsError::Unreachable(_)), "{e:?}");
}

/// A TCP proxy whose connections can all be cut at once (a NAS rebooting, Wi-Fi dropping).
struct Proxy {
    port: u16,
    cut: Arc<AtomicBool>,
}

async fn proxy(target: &'static str) -> Proxy {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let cut = Arc::new(AtomicBool::new(false));
    let generation = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (c2, g2) = (cut.clone(), generation.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut inbound, _)) = l.accept().await else {
                return;
            };
            let (cut, gen_now) = (c2.clone(), g2.load(Ordering::SeqCst));
            let g = g2.clone();
            tokio::spawn(async move {
                let Ok(mut out) = TcpStream::connect(target).await else {
                    return;
                };
                tokio::select! {
                    _ = copy_bidirectional(&mut inbound, &mut out) => {}
                    _ = async {
                        while !cut.load(Ordering::SeqCst) || g.load(Ordering::SeqCst) != gen_now {
                            if g.load(Ordering::SeqCst) != gen_now { return; }
                            tokio::time::sleep(Duration::from_millis(20)).await;
                        }
                    } => {}
                }
            });
        }
    });
    // Cutting bumps the generation: existing connections die, new ones are accepted again.
    let (c3, g3) = (cut.clone(), generation.clone());
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if c3.swap(false, Ordering::SeqCst) {
                g3.fetch_add(1, Ordering::SeqCst);
            }
        }
    });
    Proxy { port, cut }
}

async fn survives_dropped_connection(protocol: Protocol, target: &'static str) {
    let reference = std::fs::read(fixtures().join(MOVIE.trim_start_matches('/'))).unwrap();
    let p = proxy(target).await;
    let mut c = cfg(protocol);
    c.host = "127.0.0.1".into();
    c.port = Some(p.port);
    let src = remote::connect(c).await.unwrap();
    let a = src.read_at(&VPath::new(MOVIE), 0, 100_000).await.unwrap();
    assert!(a[..] == reference[..100_000]);
    p.cut.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(100)).await;
    // The backend reconnects once by itself.
    let b = src
        .read_at(&VPath::new(MOVIE), 100_000, 100_000)
        .await
        .unwrap_or_else(|e| panic!("{}: after drop: {e}", protocol.as_str()));
    assert!(
        b[..] == reference[100_000..200_000],
        "{}: data after reconnect",
        protocol.as_str()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn smb_survives_dropped_connection() {
    if enabled() {
        survives_dropped_connection(Protocol::Smb, "172.29.0.10:445").await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn webdav_survives_dropped_connection() {
    if enabled() {
        survives_dropped_connection(Protocol::Webdav, "172.29.0.12:8080").await;
    }
}
