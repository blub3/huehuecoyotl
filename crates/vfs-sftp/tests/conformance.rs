//! SFTP against the rclone test server (tests/servers): listing, ranged and sequential reads,
//! wrong password, trust-on-first-use host key.
//! `COYOTL_TEST_SERVERS=1 cargo test --manifest-path crates/vfs-sftp/Cargo.toml`
use huehuecoyotl_vfs::{Protocol, RemoteConfig, RemoteSource, Source, VPath, VfsError};
use huehuecoyotl_vfs_sftp::SftpSource;

const MOVIE: &str = "/Movies/Alien (1979)/Alien.1979.1080p.mkv";

fn cfg() -> RemoteConfig {
    RemoteConfig {
        protocol: Protocol::Sftp,
        host: "172.29.0.12".into(),
        port: Some(2022),
        share: String::new(),
        user: "coyotl".into(),
        password: "coyotl-test".into(),
        tls: false,
        host_key: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn sftp_conformance() {
    if std::env::var_os("COYOTL_TEST_SERVERS").is_none() {
        return;
    }
    let reference = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/servers/fixtures/Movies/Alien (1979)/Alien.1979.1080p.mkv"
    ))
    .unwrap();
    let src = SftpSource::new(cfg());
    let mut names: Vec<_> = src
        .list(&VPath::new("/"))
        .await
        .unwrap()
        .into_iter()
        .map(|e| (e.name, e.is_dir))
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            ("Movies".to_string(), true),
            ("Shows".to_string(), true),
            ("notes.txt".to_string(), false)
        ]
    );
    assert_eq!(
        src.stat(&VPath::new(MOVIE)).await.unwrap().size,
        reference.len() as u64
    );
    let b = src
        .read_at(&VPath::new(MOVIE), 1_000_003, 65_536)
        .await
        .unwrap();
    assert!(b[..] == reference[1_000_003..1_000_003 + 65_536]);
    let mut got = Vec::new();
    loop {
        let b = src
            .read_at(&VPath::new(MOVIE), got.len() as u64, 256 * 1024)
            .await
            .unwrap();
        if b.is_empty() {
            break;
        }
        got.extend_from_slice(&b);
    }
    assert!(got == reference);
    assert!(matches!(
        src.list(&VPath::new("/Nope")).await,
        Err(VfsError::NotFound(_))
    ));

    // Trust on first use: the key seen is reported; a different expected key is refused.
    let key = src.host_key().expect("host key recorded");
    assert!(key.starts_with("SHA256:"), "{key}");
    let mut pinned = cfg();
    pinned.host_key = Some(key);
    assert!(SftpSource::new(pinned).list(&VPath::new("/")).await.is_ok());
    let mut wrong_key = cfg();
    wrong_key.host_key = Some("SHA256:not-the-key".into());
    assert!(
        SftpSource::new(wrong_key)
            .list(&VPath::new("/"))
            .await
            .is_err()
    );

    let mut bad = cfg();
    bad.password = "wrong".into();
    let e = SftpSource::new(bad)
        .list(&VPath::new("/"))
        .await
        .unwrap_err();
    assert!(matches!(e, VfsError::Auth(_)), "{e:?}");
}
