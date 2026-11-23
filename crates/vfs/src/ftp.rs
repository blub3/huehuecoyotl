//! FTP through `suppaftp` on tokio (SPEC 6). FTPS is not offered yet.
//!
//! FTP has no ranged reads: a read is REST + RETR on a data connection. To stream efficiently
//! the backend keeps the last data connection open and continues on it while reads are
//! sequential, and only aborts and restarts the transfer on a seek.

use std::str::FromStr;
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use bytes::Bytes;
use suppaftp::list::File as ListFile;
use suppaftp::tokio::{AsyncFtpStream, AsyncNoTlsStream};
use suppaftp::types::FileType;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::remote::{RemoteConfig, RemoteSource, err, join, retryable, write_err};
use crate::{Caps, Chunks, Entry, Result, Source, VPath, VfsError};

type Transfer = suppaftp::tokio::TransferStream<AsyncNoTlsStream>;

struct Reader {
    path: String,
    pos: u64,
    ctrl: AsyncFtpStream,
    data: Transfer,
}

pub struct FtpSource {
    cfg: RemoteConfig,
    meta: Mutex<Option<AsyncFtpStream>>,
    reader: Mutex<Option<Reader>>,
}

impl FtpSource {
    pub fn new(cfg: RemoteConfig) -> Self {
        Self {
            cfg,
            meta: Mutex::new(None),
            reader: Mutex::new(None),
        }
    }

    async fn login(&self) -> Result<AsyncFtpStream> {
        let mut s = AsyncFtpStream::connect((self.cfg.host.as_str(), self.cfg.port()))
            .await
            .map_err(err)?;
        let user = if self.cfg.user.is_empty() {
            "anonymous"
        } else {
            &self.cfg.user
        };
        s.login(user, &self.cfg.password).await.map_err(|e| {
            let m = e.to_string();
            if m.contains("530") {
                VfsError::Auth(m)
            } else {
                err(m)
            }
        })?;
        s.transfer_type(FileType::Binary).await.map_err(err)?;
        Ok(s)
    }

    fn server_path(&self, p: &VPath) -> String {
        join(&self.cfg.share, p)
    }

    async fn list_once(&self, dir: &VPath) -> Result<Vec<Entry>> {
        let mut g = self.meta.lock().await;
        if g.is_none() {
            *g = Some(self.login().await?);
        }
        let s = g.as_mut().expect("connected");
        let path = self.server_path(dir);
        let lines = match s.list(Some(&path)).await {
            Ok(l) => l,
            Err(e) => {
                g.take();
                return Err(err(e));
            }
        };
        // Some servers (vsftpd) answer LIST on a missing folder with an empty listing.
        if lines.is_empty() && path != "/" {
            if s.cwd(&path).await.is_err() {
                return Err(VfsError::NotFound(dir.to_string()));
            }
            let _ = s.cwd("/").await;
        }
        let mut out = Vec::new();
        for line in lines {
            let Ok(f) = ListFile::from_str(&line) else {
                continue;
            };
            let name = f.name().to_string();
            if name == "." || name == ".." || name.starts_with('.') {
                continue;
            }
            let mtime = f
                .modified()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64);
            out.push(Entry {
                path: dir.join(&name),
                name,
                is_dir: f.is_directory(),
                size: f.size() as u64,
                mtime,
            });
        }
        Ok(out)
    }

    /// One command that changes something, on the metadata connection; a refusal is read-only.
    async fn change<F>(&self, f: F) -> Result<()>
    where
        F: for<'a> FnOnce(
            &'a mut AsyncFtpStream,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = suppaftp::FtpResult<()>> + Send + 'a>,
        >,
    {
        self.reader.lock().await.take();
        let mut g = self.meta.lock().await;
        if g.is_none() {
            *g = Some(self.login().await?);
        }
        let s = g.as_mut().expect("connected");
        f(s).await.map_err(|e| write_err(err(e)))
    }

    async fn read_once(&self, path: &VPath, offset: u64, len: u32) -> Result<Bytes> {
        let sp = self.server_path(path);
        let mut g = self.reader.lock().await;
        let reusable = g.as_ref().is_some_and(|r| r.path == sp && r.pos == offset);
        if !reusable {
            // Seek: abort the running transfer (or drop the connection if that fails).
            if let Some(mut r) = g.take()
                && r.ctrl.abort(r.data).await.is_ok()
            {
                r.ctrl.resume_transfer(offset as usize).await.map_err(err)?;
                let data = r.ctrl.retr_as_stream(&sp).await.map_err(err)?;
                *g = Some(Reader {
                    path: sp.clone(),
                    pos: offset,
                    ctrl: r.ctrl,
                    data,
                });
            }
            if g.is_none() {
                let mut ctrl = self.login().await?;
                ctrl.resume_transfer(offset as usize).await.map_err(err)?;
                let data = ctrl.retr_as_stream(&sp).await.map_err(err)?;
                *g = Some(Reader {
                    path: sp.clone(),
                    pos: offset,
                    ctrl,
                    data,
                });
            }
        }
        let r = g.as_mut().expect("reader");
        let mut buf = vec![0u8; len as usize];
        let mut got = 0;
        while got < buf.len() {
            let n = match r.data.read(&mut buf[got..]).await {
                Ok(n) => n,
                Err(e) => {
                    g.take();
                    return Err(err(e));
                }
            };
            if n == 0 {
                break;
            }
            got += n;
        }
        r.pos += got as u64;
        buf.truncate(got);
        Ok(Bytes::from(buf))
    }
}

#[async_trait]
impl Source for FtpSource {
    async fn list(&self, dir: &VPath) -> Result<Vec<Entry>> {
        match self.list_once(dir).await {
            Err(e) if retryable(&e) => self.list_once(dir).await,
            r => r,
        }
    }

    async fn stat(&self, path: &VPath) -> Result<Entry> {
        if path.as_str().trim_matches('/').is_empty() {
            return Ok(Entry {
                path: VPath::new("/"),
                name: String::new(),
                is_dir: true,
                size: 0,
                mtime: 0,
            });
        }
        let parent = path.parent().unwrap_or_else(|| VPath::new("/"));
        let name = path.file_name().to_string();
        self.list(&parent)
            .await?
            .into_iter()
            .find(|e| e.name == name)
            .ok_or_else(|| VfsError::NotFound(path.to_string()))
    }

    async fn read_at(&self, path: &VPath, offset: u64, len: u32) -> Result<Bytes> {
        match self.read_once(path, offset, len).await {
            Err(e) if retryable(&e) => {
                self.reader.lock().await.take();
                self.read_once(path, offset, len).await
            }
            r => r,
        }
    }

    async fn size_of(&self, path: &VPath) -> Result<u64> {
        let mut g = self.meta.lock().await;
        if g.is_none() {
            *g = Some(self.login().await?);
        }
        let s = g.as_mut().expect("connected");
        s.size(self.server_path(path))
            .await
            .map(|n| n as u64)
            .map_err(err)
    }

    async fn put(&self, path: &VPath, mut data: Chunks) -> Result<u64> {
        self.reader.lock().await.take();
        // A connection of its own: the upload keeps it busy, listings go on.
        let mut ctrl = self.login().await?;
        let sp = self.server_path(path);
        let mut up = ctrl
            .put_with_stream(&sp)
            .await
            .map_err(|e| write_err(err(e)))?;
        let mut n = 0u64;
        while let Some(chunk) = data.recv().await {
            let chunk = chunk?;
            up.write_all(&chunk).await.map_err(err)?;
            n += chunk.len() as u64;
        }
        up.flush().await.map_err(err)?;
        up.finish().await.map_err(|e| write_err(err(e)))?;
        let _ = ctrl.quit().await;
        Ok(n)
    }

    async fn create_dir(&self, path: &VPath) -> Result<()> {
        let p = self.server_path(path);
        self.change(move |s| Box::pin(async move { s.mkdir(&p).await }))
            .await
    }

    async fn rename(&self, from: &VPath, to: &VPath) -> Result<()> {
        let (a, b) = (self.server_path(from), self.server_path(to));
        self.change(move |s| Box::pin(async move { s.rename(&a, &b).await }))
            .await
    }

    async fn remove(&self, path: &VPath) -> Result<()> {
        let p = self.server_path(path);
        self.change(move |s| Box::pin(async move { s.rm(&p).await }))
            .await
    }

    async fn remove_dir(&self, path: &VPath) -> Result<()> {
        let p = self.server_path(path);
        self.change(move |s| Box::pin(async move { s.rmdir(&p).await }))
            .await
    }

    fn capabilities(&self) -> Caps {
        Caps {
            range_reads: true,
            ..Default::default()
        }
    }
}

impl RemoteSource for FtpSource {}
