//! NFS v3 and v4.x through the pure-Rust `nfs-rs` crate (SPEC 6).
//!
//! Servers that only accept privileged source ports need the `insecure` export option (or the
//! app needs elevated rights); the error message says so.

use std::collections::HashMap;

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use nfs_rs::Mount;
use tokio::sync::Mutex;

use crate::remote::{RemoteConfig, RemoteSource, rel, retryable, write_err};
use crate::{Caps, Chunks, Entry, Result, Source, VPath, VfsError};

const NF_DIR: u32 = 2;

pub struct NfsSource {
    cfg: RemoteConfig,
    mount: Mutex<Option<std::sync::Arc<Box<dyn Mount>>>>,
    handles: Mutex<HashMap<String, (Bytes, u64)>>,
}

fn nfs_err(e: nfs_rs::NfsError) -> VfsError {
    let msg = e.to_string();
    match e.kind() {
        std::io::ErrorKind::NotFound => VfsError::NotFound(msg),
        std::io::ErrorKind::PermissionDenied => VfsError::Auth(format!(
            "{msg}. If the server only allows privileged ports, add the `insecure` option to the export."
        )),
        std::io::ErrorKind::ConnectionRefused
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::TimedOut
        | std::io::ErrorKind::BrokenPipe
        | std::io::ErrorKind::UnexpectedEof
        | std::io::ErrorKind::NotConnected => VfsError::Unreachable(msg),
        // NFSv4 status errors come without an io::ErrorKind; classify their text.
        _ => crate::remote::err(msg),
    }
}

impl NfsSource {
    pub fn new(cfg: RemoteConfig) -> Self {
        Self {
            cfg,
            mount: Mutex::new(None),
            handles: Mutex::new(HashMap::new()),
        }
    }

    fn url(&self) -> String {
        let export = format!("/{}", self.cfg.share.trim_matches('/'));
        let port = match self.cfg.port {
            Some(p) if p != 2049 => format!("&nfsport={p}"),
            _ => String::new(),
        };
        // Prefer v4.1, fall back to v4.0 and v3. AUTH_SYS as an ordinary user, from an ordinary
        // (non-reserved) source port: the app does not run as root, so exports need `insecure`.
        format!(
            "nfs://{}{export}?version=4.1,4.0,3&uid=1000&gid=1000&noresvport=true{port}",
            self.cfg.host
        )
    }

    async fn mount(&self) -> Result<std::sync::Arc<Box<dyn Mount>>> {
        let mut g = self.mount.lock().await;
        if let Some(m) = g.as_ref() {
            return Ok(m.clone());
        }
        let m = std::sync::Arc::new(
            nfs_rs::parse_url_and_mount(&self.url())
                .await
                .map_err(nfs_err)?,
        );
        *g = Some(m.clone());
        Ok(m)
    }

    async fn reset(&self) {
        if let Some(m) = self.mount.lock().await.take() {
            let _ = m.umount().await;
        }
        self.handles.lock().await.clear();
    }

    async fn list_once(&self, dir: &VPath) -> Result<Vec<Entry>> {
        let m = self.mount().await?;
        let mut stream = m.readdirplus_path(rel(dir)).await.map_err(nfs_err)?;
        let mut out = Vec::new();
        while let Some(item) = stream.next().await {
            let e = item.map_err(nfs_err)?;
            if e.file_name == "." || e.file_name == ".." || e.file_name.starts_with('.') {
                continue;
            }
            let (is_dir, size, mtime) = match &e.attr {
                Some(a) => (a.type_ == NF_DIR, a.filesize, i64::from(a.mtime.seconds)),
                None => (false, 0, 0),
            };
            out.push(Entry {
                path: dir.join(&e.file_name),
                name: e.file_name,
                is_dir,
                size: if is_dir { 0 } else { size },
                mtime,
            });
        }
        Ok(out)
    }

    async fn stat_once(&self, path: &VPath) -> Result<Entry> {
        let m = self.mount().await?;
        let a = m.getattr_path(rel(path)).await.map_err(nfs_err)?;
        let is_dir = a.type_ == NF_DIR;
        Ok(Entry {
            path: path.clone(),
            name: path.file_name().to_string(),
            is_dir,
            size: if is_dir { 0 } else { a.filesize },
            mtime: i64::from(a.mtime.seconds),
        })
    }

    async fn read_once(&self, path: &VPath, offset: u64, len: u32) -> Result<Bytes> {
        let m = self.mount().await?;
        let key = path.to_string();
        let cached = self.handles.lock().await.get(&key).cloned();
        let (fh, size) = match cached {
            Some(h) => h,
            None => {
                let o = m.lookup_path(rel(path)).await.map_err(nfs_err)?;
                let size = o.attr.as_ref().map_or(u64::MAX, |a| a.filesize);
                let mut h = self.handles.lock().await;
                if h.len() > 64 {
                    h.clear();
                }
                h.insert(key, (o.fh.clone(), size));
                (o.fh, size)
            }
        };
        if offset >= size {
            return Ok(Bytes::new());
        }
        let max = m.get_max_read_size().max(64 * 1024);
        let want = (len as u64).min(size.saturating_sub(offset)) as usize;
        let mut out = Vec::with_capacity(want);
        while out.len() < want {
            let n = ((want - out.len()) as u32).min(max);
            let b = m
                .read(fh.clone(), offset + out.len() as u64, n)
                .await
                .map_err(nfs_err)?;
            if b.is_empty() {
                break;
            }
            out.extend_from_slice(&b);
        }
        Ok(Bytes::from(out))
    }
}

#[async_trait]
impl Source for NfsSource {
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
        self.handles.lock().await.remove(path.as_str());
        let m = self.mount().await?;
        let o = m
            .create_path(rel(path), Some(0o644))
            .await
            .map_err(|e| write_err(nfs_err(e)))?;
        let max = (m.get_max_write_size().max(64 * 1024)) as usize;
        let mut pos = 0u64;
        while let Some(chunk) = data.recv().await {
            let chunk = chunk?;
            let mut at = 0;
            while at < chunk.len() {
                let part = chunk.slice(at..(at + max).min(chunk.len()));
                let w = m
                    .write(o.fh.clone(), pos, part)
                    .await
                    .map_err(|e| write_err(nfs_err(e)))?;
                if w.count == 0 {
                    return Err(VfsError::Internal("the server wrote nothing".into()));
                }
                at += w.count as usize;
                pos += u64::from(w.count);
            }
        }
        m.commit(o.fh.clone(), 0, 0)
            .await
            .map_err(|e| write_err(nfs_err(e)))?;
        Ok(pos)
    }

    async fn create_dir(&self, path: &VPath) -> Result<()> {
        let m = self.mount().await?;
        m.mkdir_path(rel(path), 0o755)
            .await
            .map(|_| ())
            .map_err(|e| write_err(nfs_err(e)))
    }

    async fn rename(&self, from: &VPath, to: &VPath) -> Result<()> {
        self.handles.lock().await.clear();
        let m = self.mount().await?;
        m.rename_path(rel(from), rel(to))
            .await
            .map_err(|e| write_err(nfs_err(e)))
    }

    async fn remove(&self, path: &VPath) -> Result<()> {
        self.handles.lock().await.remove(path.as_str());
        let m = self.mount().await?;
        m.remove_path(rel(path))
            .await
            .map_err(|e| write_err(nfs_err(e)))
    }

    async fn remove_dir(&self, path: &VPath) -> Result<()> {
        let m = self.mount().await?;
        m.rmdir_path(rel(path))
            .await
            .map_err(|e| write_err(nfs_err(e)))
    }

    fn capabilities(&self) -> Caps {
        Caps {
            range_reads: true,
            stable_ids: true,
            ..Default::default()
        }
    }
}

impl RemoteSource for NfsSource {}
