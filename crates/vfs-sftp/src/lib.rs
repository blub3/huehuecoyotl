//! SFTP for Huehuecoyotl (separate crate, see crates/vfs/Cargo.toml). Pure Rust: `russh` + `russh-sftp`, pure Rust on the ring backend (SPEC 6 names these as
//! the fallback when OpenDAL's SFTP does not fit; OpenDAL's needs the system `ssh` binary).
//!
//! Host keys: trust on first use. The first fingerprint is reported through
//! [`RemoteSource::host_key`] and stored with the source; a different key later is refused.
//!
//! Login with a password, or with a private key file (`RemoteConfig::key_file`, OpenSSH format;
//! Ed25519 and ECDSA, since RSA support is not built in) whose passphrase is the `password`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, UNIX_EPOCH};

use async_trait::async_trait;
use bytes::Bytes;
use russh::client;
use russh::keys::HashAlg;
use russh_sftp::client::SftpSession;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;

use huehuecoyotl_vfs::remote::{RemoteConfig, RemoteSource, err, join, retryable, write_err};
use huehuecoyotl_vfs::{Caps, Chunks, Entry, Result, Source, VPath, VfsError};

struct Handler {
    expected: Option<String>,
    seen: Arc<StdMutex<Option<String>>>,
}

impl client::Handler for Handler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &russh::keys::PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        let fp = key.public_key().fingerprint(HashAlg::Sha256).to_string();
        let ok = self.expected.as_ref().is_none_or(|e| *e == fp);
        *self.seen.lock().unwrap() = Some(fp);
        Ok(ok)
    }
}

struct Conn {
    _session: client::Handle<Handler>,
    sftp: SftpSession,
}

/// Read a private key file; `~/` is the home folder. Errors name the problem, never the
/// passphrase.
fn load_key(file: &str, passphrase: &str) -> Result<russh::keys::PrivateKey> {
    let path = match file.strip_prefix("~/") {
        Some(rest) => std::env::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| file.into()),
        None => file.into(),
    };
    let pass = (!passphrase.is_empty()).then_some(passphrase);
    russh::keys::load_secret_key(&path, pass).map_err(|e| match e {
        russh::keys::Error::KeyIsEncrypted => {
            VfsError::Auth("the key file needs its passphrase".into())
        }
        russh::keys::Error::IO(e) => {
            VfsError::NotFound(format!("the key file cannot be read: {e}"))
        }
        russh::keys::Error::UnsupportedKeyType { .. } => VfsError::Unsupported(
            "this key type is not supported; use an Ed25519 or ECDSA key".into(),
        ),
        _ => VfsError::Auth("the key file could not be opened (wrong passphrase?)".into()),
    })
}

/// Connect and check that the root can be listed (the SFTP counterpart of
/// `huehuecoyotl_vfs::remote::connect`, which cannot depend on this crate).
pub async fn connect(cfg: RemoteConfig) -> Result<Arc<dyn RemoteSource>> {
    let src = Arc::new(SftpSource::new(cfg));
    src.list(&VPath::new("/")).await?;
    Ok(src)
}

pub struct SftpSource {
    cfg: RemoteConfig,
    conn: Mutex<Option<Arc<Conn>>>,
    files: Mutex<HashMap<String, Arc<Mutex<russh_sftp::client::fs::File>>>>,
    seen_key: Arc<StdMutex<Option<String>>>,
}

impl SftpSource {
    pub fn new(cfg: RemoteConfig) -> Self {
        Self {
            cfg,
            conn: Mutex::new(None),
            files: Mutex::new(HashMap::new()),
            seen_key: Arc::default(),
        }
    }

    async fn conn(&self) -> Result<Arc<Conn>> {
        let mut g = self.conn.lock().await;
        if let Some(c) = g.as_ref() {
            return Ok(c.clone());
        }
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(300)),
            ..Default::default()
        });
        let handler = Handler {
            expected: self.cfg.host_key.clone(),
            seen: self.seen_key.clone(),
        };
        let mut session = client::connect(config, (self.cfg.host.as_str(), self.cfg.port()), handler).await.map_err(|e| {
            if self.cfg.host_key.is_some() && matches!(e, russh::Error::UnknownKey) {
                VfsError::Auth("the server's host key changed; remove and re-add the source if this is expected".into())
            } else {
                err(e)
            }
        })?;
        let auth = match &self.cfg.key_file {
            Some(file) => {
                let key = load_key(file, &self.cfg.password)?;
                session
                    .authenticate_publickey(
                        &self.cfg.user,
                        russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), None),
                    )
                    .await
                    .map_err(err)?
            }
            None => session
                .authenticate_password(&self.cfg.user, &self.cfg.password)
                .await
                .map_err(err)?,
        };
        if !auth.success() {
            return Err(VfsError::Auth(if self.cfg.key_file.is_some() {
                "SFTP login with the key file failed".into()
            } else {
                "SFTP login failed".into()
            }));
        }
        let channel = session.channel_open_session().await.map_err(err)?;
        channel.request_subsystem(true, "sftp").await.map_err(err)?;
        let sftp = SftpSession::new(channel.into_stream()).await.map_err(err)?;
        let c = Arc::new(Conn {
            _session: session,
            sftp,
        });
        *g = Some(c.clone());
        Ok(c)
    }

    async fn reset(&self) {
        self.conn.lock().await.take();
        self.files.lock().await.clear();
    }

    fn server_path(&self, p: &VPath) -> String {
        join(&self.cfg.share, p)
    }

    async fn list_once(&self, dir: &VPath) -> Result<Vec<Entry>> {
        let c = self.conn().await?;
        let rd = c.sftp.read_dir(self.server_path(dir)).await.map_err(err)?;
        let mut out = Vec::new();
        for e in rd {
            let name = e.file_name();
            if name == "." || name == ".." || name.starts_with('.') {
                continue;
            }
            let m = e.metadata();
            let is_dir = m.is_dir();
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            out.push(Entry {
                path: dir.join(&name),
                name,
                is_dir,
                size: if is_dir { 0 } else { m.len() },
                mtime,
            });
        }
        Ok(out)
    }

    async fn stat_once(&self, path: &VPath) -> Result<Entry> {
        let c = self.conn().await?;
        let m = c.sftp.metadata(self.server_path(path)).await.map_err(err)?;
        let mtime = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64);
        Ok(Entry {
            path: path.clone(),
            name: path.file_name().to_string(),
            is_dir: m.is_dir(),
            size: m.len(),
            mtime,
        })
    }

    async fn read_once(&self, path: &VPath, offset: u64, len: u32) -> Result<Bytes> {
        let key = path.to_string();
        // Bind first: a guard in a `match` scrutinee lives for the whole match and would
        // deadlock with the insert below.
        let cached = self.files.lock().await.get(&key).cloned();
        let file = match cached {
            Some(f) => f,
            None => {
                let c = self.conn().await?;
                let f = Arc::new(Mutex::new(
                    c.sftp.open(self.server_path(path)).await.map_err(err)?,
                ));
                let mut files = self.files.lock().await;
                if files.len() >= 8 {
                    files.clear();
                }
                files.insert(key, f.clone());
                f
            }
        };
        let mut f = file.lock().await;
        f.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(err)?;
        let mut buf = vec![0u8; len as usize];
        let mut got = 0;
        while got < buf.len() {
            let n = f.read(&mut buf[got..]).await.map_err(err)?;
            if n == 0 {
                break;
            }
            got += n;
        }
        buf.truncate(got);
        Ok(Bytes::from(buf))
    }
}

#[async_trait]
impl Source for SftpSource {
    async fn list(&self, dir: &VPath) -> Result<Vec<Entry>> {
        match self.list_once(dir).await {
            Err(e) if retryable(&e) => {
                self.reset().await;
                self.list_once(dir).await
            }
            r => r,
        }
    }

    async fn stat(&self, path: &VPath) -> Result<Entry> {
        match self.stat_once(path).await {
            Err(e) if retryable(&e) => {
                self.reset().await;
                self.stat_once(path).await
            }
            r => r,
        }
    }

    async fn read_at(&self, path: &VPath, offset: u64, len: u32) -> Result<Bytes> {
        match self.read_once(path, offset, len).await {
            Err(e) if retryable(&e) => {
                self.reset().await;
                self.read_once(path, offset, len).await
            }
            r => r,
        }
    }

    async fn put(&self, path: &VPath, mut data: Chunks) -> Result<u64> {
        self.files.lock().await.remove(path.as_str());
        let c = self.conn().await?;
        let mut f = c
            .sftp
            .create(self.server_path(path))
            .await
            .map_err(|e| write_err(err(e)))?;
        let mut n = 0u64;
        while let Some(chunk) = data.recv().await {
            let chunk = chunk?;
            f.write_all(&chunk).await.map_err(|e| write_err(err(e)))?;
            n += chunk.len() as u64;
        }
        f.shutdown().await.map_err(|e| write_err(err(e)))?;
        Ok(n)
    }

    async fn create_dir(&self, path: &VPath) -> Result<()> {
        let c = self.conn().await?;
        c.sftp
            .create_dir(self.server_path(path))
            .await
            .map_err(|e| write_err(err(e)))
    }

    async fn rename(&self, from: &VPath, to: &VPath) -> Result<()> {
        {
            let mut files = self.files.lock().await;
            files.remove(from.as_str());
            files.remove(to.as_str());
        }
        let c = self.conn().await?;
        c.sftp
            .rename(self.server_path(from), self.server_path(to))
            .await
            .map_err(|e| write_err(err(e)))
    }

    async fn remove(&self, path: &VPath) -> Result<()> {
        self.files.lock().await.remove(path.as_str());
        let c = self.conn().await?;
        c.sftp
            .remove_file(self.server_path(path))
            .await
            .map_err(|e| write_err(err(e)))
    }

    async fn remove_dir(&self, path: &VPath) -> Result<()> {
        let c = self.conn().await?;
        c.sftp
            .remove_dir(self.server_path(path))
            .await
            .map_err(|e| write_err(err(e)))
    }

    fn capabilities(&self) -> Caps {
        Caps {
            range_reads: true,
            ..Default::default()
        }
    }
}

impl RemoteSource for SftpSource {
    fn host_key(&self) -> Option<String> {
        self.seen_key.lock().unwrap().clone()
    }
}
