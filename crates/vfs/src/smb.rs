//! SMB 2 and 3 through the pure-Rust `smb` crate (SPEC 6). No Samba, no GPL.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use smb::{
    Client, ClientConfig, Directory, FileAccessMask, FileCreateArgs, FileDirectoryInformation,
    GetLen, UncPath,
};
use tokio::sync::Mutex;

use crate::remote::{RemoteConfig, RemoteSource, err, rel, retryable};
use crate::{Caps, Entry, Result, Source, VPath, VfsError};

const READ_BLOCK: usize = 1 << 20;
const MAX_OPEN: usize = 8;

pub struct SmbSource {
    cfg: RemoteConfig,
    client: Mutex<Option<Arc<Client>>>,
    files: Mutex<HashMap<String, (Arc<smb::File>, u64)>>,
}

impl SmbSource {
    pub fn new(cfg: RemoteConfig) -> Self {
        Self {
            cfg,
            client: Mutex::new(None),
            files: Mutex::new(HashMap::new()),
        }
    }

    fn share_unc(&self) -> Result<UncPath> {
        UncPath::from_str(&format!(
            r"\\{}\{}",
            self.cfg.host,
            self.cfg.share.trim_matches(['/', '\\'])
        ))
        .map_err(|e| VfsError::Unsupported(format!("invalid SMB address: {e:?}")))
    }

    fn unc(&self, path: &VPath) -> Result<UncPath> {
        let p = rel(path).replace('/', "\\");
        Ok(if p.is_empty() {
            self.share_unc()?
        } else {
            self.share_unc()?.with_path(&p)
        })
    }

    async fn client(&self) -> Result<Arc<Client>> {
        let mut g = self.client.lock().await;
        if let Some(c) = g.as_ref() {
            return Ok(c.clone());
        }
        let mut cc = ClientConfig {
            dfs: false,
            ..Default::default()
        };
        cc.connection.port = self.cfg.port;
        cc.connection.smb2_only_negotiate = true;
        let c = Arc::new(Client::new(cc));
        let user = if self.cfg.user.is_empty() {
            "guest"
        } else {
            &self.cfg.user
        };
        c.share_connect(&self.share_unc()?, user, self.cfg.password.clone())
            .await
            .map_err(err)?;
        *g = Some(c.clone());
        Ok(c)
    }

    async fn reset(&self) {
        self.client.lock().await.take();
        self.files.lock().await.clear();
    }

    async fn list_once(&self, dir: &VPath) -> Result<Vec<Entry>> {
        let c = self.client().await?;
        let args =
            FileCreateArgs::make_open_existing(FileAccessMask::new().with_generic_read(true));
        let res = c.create_file(&self.unc(dir)?, &args).await.map_err(err)?;
        if !res.is_dir() {
            return Err(VfsError::Unsupported(format!("{dir} is not a folder")));
        }
        let d = Arc::new(res.unwrap_dir());
        let mut out = Vec::new();
        {
            let mut stream = Directory::query::<FileDirectoryInformation>(&d, "*")
                .await
                .map_err(err)?;
            while let Some(item) = stream.next().await {
                let info = item.map_err(err)?;
                let name = String::try_from(info.file_name.clone()).unwrap_or_default();
                if name == "." || name == ".." || name.is_empty() || name.starts_with('.') {
                    continue;
                }
                let mtime = SystemTime::from(info.last_write_time)
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs() as i64);
                let is_dir = info.file_attributes.directory();
                out.push(Entry {
                    path: dir.join(&name),
                    name,
                    is_dir,
                    size: if is_dir { 0 } else { info.end_of_file },
                    mtime,
                });
            }
        }
        let _ = d.close().await;
        Ok(out)
    }

    async fn file(&self, path: &VPath) -> Result<(Arc<smb::File>, u64)> {
        let key = path.to_string();
        if let Some(f) = self.files.lock().await.get(&key) {
            return Ok(f.clone());
        }
        let c = self.client().await?;
        let args =
            FileCreateArgs::make_open_existing(FileAccessMask::new().with_generic_read(true));
        let res = c.create_file(&self.unc(path)?, &args).await.map_err(err)?;
        if res.is_dir() {
            return Err(VfsError::Unsupported(format!("{path} is a folder")));
        }
        let f = Arc::new(res.unwrap_file());
        let len = f.get_len().await.map_err(err)?;
        let mut files = self.files.lock().await;
        if files.len() >= MAX_OPEN {
            files.clear(); // handles close when the last Arc drops
        }
        files.insert(key, (f.clone(), len));
        Ok((f, len))
    }

    async fn read_once(&self, path: &VPath, offset: u64, len: u32) -> Result<Bytes> {
        let (f, size) = self.file(path).await?;
        if offset >= size {
            return Ok(Bytes::new());
        }
        let want = (len as u64).min(size - offset) as usize;
        // Parallel 1 MiB requests (SMB credits permitting): one round trip for a whole chunk.
        let mut parts = Vec::new();
        let mut o = 0;
        while o < want {
            let n = READ_BLOCK.min(want - o);
            let (f, at) = (f.clone(), offset + o as u64);
            parts.push(tokio::spawn(async move {
                let mut buf = vec![0u8; n];
                let mut got = 0;
                while got < n {
                    let r = f
                        .read_block(&mut buf[got..], at + got as u64, None, false)
                        .await?;
                    if r == 0 {
                        break;
                    }
                    got += r;
                }
                buf.truncate(got);
                Ok::<_, std::io::Error>(buf)
            }));
            o += n;
        }
        let mut out = Vec::with_capacity(want);
        for p in parts {
            let b = p
                .await
                .map_err(|e| VfsError::Internal(e.to_string()))?
                .map_err(err)?;
            let short = b.len() < READ_BLOCK;
            out.extend_from_slice(&b);
            if short {
                break;
            }
        }
        Ok(Bytes::from(out))
    }
}

#[async_trait]
impl Source for SmbSource {
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
        if rel(path).is_empty() {
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
            .find(|e| e.name.eq_ignore_ascii_case(&name))
            .ok_or_else(|| VfsError::NotFound(path.to_string()))
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

    fn capabilities(&self) -> Caps {
        Caps {
            range_reads: true,
            ..Default::default()
        }
    }
}

impl RemoteSource for SmbSource {}
