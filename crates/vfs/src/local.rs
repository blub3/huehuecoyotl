//! Local disks and OS-mounted shares (SPEC 6, M1).

use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::{Caps, Entry, Result, Source, VPath, VfsError};

#[derive(Default)]
pub struct LocalSource;

fn to_entry(path: VPath, meta: &std::fs::Metadata) -> Entry {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64);
    let name = path.file_name().to_string();
    Entry {
        path,
        name,
        is_dir: meta.is_dir(),
        size: if meta.is_dir() { 0 } else { meta.len() },
        mtime,
    }
}

#[async_trait]
impl Source for LocalSource {
    async fn list(&self, dir: &VPath) -> Result<Vec<Entry>> {
        let mut rd = tokio::fs::read_dir(dir.as_str()).await?;
        let mut out = Vec::new();
        while let Some(de) = rd.next_entry().await? {
            let name = de.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue; // hidden files
            }
            // Follow symlinks; skip entries we cannot stat (dangling links, no permission).
            let Ok(meta) = tokio::fs::metadata(de.path()).await else {
                continue;
            };
            out.push(to_entry(dir.join(&name), &meta));
        }
        out.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| natord(&a.name, &b.name))
        });
        Ok(out)
    }

    async fn stat(&self, path: &VPath) -> Result<Entry> {
        let meta = tokio::fs::metadata(path.as_str()).await?;
        Ok(to_entry(path.clone(), &meta))
    }

    async fn read_at(&self, path: &VPath, offset: u64, len: u32) -> Result<Bytes> {
        let mut f = tokio::fs::File::open(path.as_str()).await?;
        f.seek(std::io::SeekFrom::Start(offset)).await?;
        let mut buf = vec![0u8; len as usize];
        let mut got = 0;
        while got < buf.len() {
            let n = f.read(&mut buf[got..]).await?;
            if n == 0 {
                break;
            }
            got += n;
        }
        buf.truncate(got);
        Ok(Bytes::from(buf))
    }

    fn capabilities(&self) -> Caps {
        Caps {
            range_reads: true,
            change_notifications: false,
            stable_ids: false,
            native_paths: true,
        }
    }
}

impl LocalSource {
    /// Starting points for browsing: home, common media folders and mounted volumes.
    pub fn places() -> Vec<Entry> {
        let mut out = Vec::new();
        let mut add = |p: std::path::PathBuf, label: Option<String>| {
            if let Ok(meta) = std::fs::metadata(&p) {
                if meta.is_dir() {
                    let mut e = to_entry(VPath::new(p.to_string_lossy()), &meta);
                    if let Some(l) = label {
                        e.name = l;
                    }
                    if !out.iter().any(|o: &Entry| o.path == e.path) {
                        out.push(e);
                    }
                }
            }
        };
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(std::path::PathBuf::from);
        if let Some(h) = &home {
            add(h.clone(), Some("Home".into()));
            for d in ["Videos", "Movies", "Downloads"] {
                add(h.join(d), None);
            }
        }
        #[cfg(windows)]
        for letter in b'A'..=b'Z' {
            add(
                format!("{}:/", letter as char).into(),
                Some(format!("Drive {}:", letter as char)),
            );
        }
        #[cfg(not(windows))]
        {
            let user = std::env::var("USER").unwrap_or_default();
            for base in [
                format!("/media/{user}"),
                format!("/run/media/{user}"),
                "/mnt".into(),
            ] {
                if let Ok(rd) = std::fs::read_dir(&base) {
                    for de in rd.flatten() {
                        add(de.path(), None);
                    }
                }
            }
            add("/".into(), Some("Computer".into()));
        }
        out
    }
}

/// Natural order: "Episode 2" before "Episode 10". Case-insensitive.
pub fn natord(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = a.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    a.next();
                }
                let mut nb = String::new();
                while let Some(c) = b.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    b.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                let o = x.to_lowercase().cmp(y.to_lowercase());
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
                a.next();
                b.next();
            }
        }
    }
}

impl From<tokio::task::JoinError> for VfsError {
    fn from(e: tokio::task::JoinError) -> Self {
        VfsError::Internal(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["Ep 10.mkv", "ep 2.mkv", "Ep 1.mkv", "Ep 02b.mkv"];
        v.sort_by(|a, b| natord(a, b));
        assert_eq!(v, ["Ep 1.mkv", "ep 2.mkv", "Ep 02b.mkv", "Ep 10.mkv"]);
    }

    #[tokio::test]
    async fn list_stat_read() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("Season 1")).unwrap();
        std::fs::write(d.path().join("b.mkv"), b"0123456789").unwrap();
        std::fs::write(d.path().join(".hidden"), b"x").unwrap();
        let src = LocalSource;
        let root = VPath::new(d.path().to_string_lossy());
        let l = src.list(&root).await.unwrap();
        let names: Vec<_> = l.iter().map(|e| (e.name.as_str(), e.is_dir)).collect();
        assert_eq!(names, [("Season 1", true), ("b.mkv", false)]);
        let f = root.join("b.mkv");
        assert_eq!(src.stat(&f).await.unwrap().size, 10);
        assert_eq!(&src.read_at(&f, 3, 4).await.unwrap()[..], b"3456");
        assert_eq!(&src.read_at(&f, 8, 100).await.unwrap()[..], b"89");
        assert!(matches!(
            src.list(&root.join("nope")).await,
            Err(VfsError::NotFound(_))
        ));
    }
}
