//! Safe writes (issue #151, SPEC 6). A file is written to a temporary name in its folder, checked,
//! and only then renamed over the old one, so a failure at any point leaves the old file as it
//! was. A file that changed on the server since it was read is not overwritten.

use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use tokio::sync::mpsc;

use crate::{Result, Source, VPath, VfsError};

/// A file's content, chunk by chunk. A sender that reports an error stops the write.
pub type Chunks = mpsc::Receiver<Result<Bytes>>;

/// The file as it was read. `None` for a new file: then nothing may exist at the path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expected {
    pub size: u64,
    pub mtime: i64,
}

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A hidden name next to `path`, which listings and scans skip.
pub fn temp_path(path: &VPath) -> Result<VPath> {
    let dir = path
        .parent()
        .ok_or_else(|| VfsError::Unsupported(format!("{path} has no folder")))?;
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    Ok(dir.join(&format!(".coyotl-tmp-{}-{t:x}-{n}", std::process::id())))
}

async fn check_unchanged<S: Source + ?Sized>(
    src: &S,
    path: &VPath,
    expected: Option<Expected>,
) -> Result<()> {
    match (src.stat(path).await, expected) {
        (Ok(e), Some(x)) if e.size == x.size && e.mtime == x.mtime => Ok(()),
        (Ok(e), Some(x)) => Err(VfsError::Changed(format!(
            "{path}: {} bytes at {}, expected {} bytes at {}",
            e.size, e.mtime, x.size, x.mtime
        ))),
        (Ok(_), None) => Err(VfsError::Changed(format!("{path} already exists"))),
        (Err(VfsError::NotFound(_)), Some(_)) => Err(VfsError::Changed(format!("{path} is gone"))),
        (Err(VfsError::NotFound(_)), None) => Ok(()),
        (Err(e), _) => Err(e),
    }
}

/// Replace `path`, or create it when `expected` is `None`. Returns the bytes written.
pub async fn safe_write<S: Source + ?Sized>(
    src: &S,
    path: &VPath,
    data: Chunks,
    expected: Option<Expected>,
) -> Result<u64> {
    check_unchanged(src, path, expected).await?;
    let tmp = temp_path(path)?;
    let result = async {
        let written = src.put(&tmp, data).await?;
        let on_server = src.size_of(&tmp).await?;
        if on_server != written {
            return Err(VfsError::Internal(format!(
                "{path}: wrote {written} bytes, the server has {on_server}"
            )));
        }
        // Once more: the upload may have taken a while.
        check_unchanged(src, path, expected).await?;
        replace(src, &tmp, path, expected.is_some()).await?;
        Ok(written)
    }
    .await;
    if result.is_err() {
        let _ = src.remove(&tmp).await;
    }
    result
}

/// Rename over the target. Servers that refuse to replace (plain SFTP) get the old file moved
/// aside first and put back if the rename then fails.
async fn replace<S: Source + ?Sized>(
    src: &S,
    tmp: &VPath,
    path: &VPath,
    exists: bool,
) -> Result<()> {
    let refused = match src.rename(tmp, path).await {
        Ok(()) => return Ok(()),
        Err(e) if !exists => return Err(e),
        Err(e) => e,
    };
    if matches!(refused, VfsError::Unreachable(_)) {
        return Err(refused);
    }
    let aside = temp_path(path)?;
    src.rename(path, &aside).await?;
    match src.rename(tmp, path).await {
        Ok(()) => {
            let _ = src.remove(&aside).await;
            Ok(())
        }
        Err(e) => {
            let _ = src.rename(&aside, path).await;
            Err(e)
        }
    }
}

/// Whether files can be created, renamed and removed in `dir`, by doing it with a small file.
/// `ReadOnly` when they cannot.
pub async fn check_writable<S: Source + ?Sized>(src: &S, dir: &VPath) -> Result<()> {
    let tmp = temp_path(&dir.join("x"))?;
    let moved = temp_path(&dir.join("x"))?;
    let (tx, rx) = mpsc::channel(1);
    tx.send(Ok(Bytes::from_static(b"coyotl")))
        .await
        .map_err(|e| VfsError::Internal(e.to_string()))?;
    drop(tx);
    match src.put(&tmp, rx).await {
        Ok(_) => {}
        // Some servers answer "not found" for a write they refuse; the folder is there.
        Err(VfsError::NotFound(m)) if src.stat(dir).await.is_ok() => {
            return Err(VfsError::ReadOnly(m));
        }
        Err(e) => return Err(e),
    }
    let renamed = src.rename(&tmp, &moved).await;
    let left = if renamed.is_ok() { &moved } else { &tmp };
    let removed = src.remove(left).await;
    renamed?;
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LocalSource;

    async fn chunks(parts: Vec<Result<Bytes>>) -> Chunks {
        let (tx, rx) = mpsc::channel(parts.len().max(1));
        for p in parts {
            tx.send(p).await.unwrap();
        }
        rx
    }

    fn only_the_file(dir: &std::path::Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[tokio::test]
    async fn safe_writes_on_a_local_folder() {
        let d = tempfile::tempdir().unwrap();
        let src = LocalSource;
        let path = VPath::new(d.path().join("a.bin").to_string_lossy());
        let rx = chunks(vec![Ok(Bytes::from_static(b"old"))]).await;
        assert_eq!(safe_write(&src, &path, rx, None).await.unwrap(), 3);
        let st = src.stat(&path).await.unwrap();
        let x = Expected {
            size: st.size,
            mtime: st.mtime,
        };
        // A new file does not replace an existing one.
        let rx = chunks(vec![Ok(Bytes::from_static(b"no"))]).await;
        assert!(matches!(
            safe_write(&src, &path, rx, None).await,
            Err(VfsError::Changed(_))
        ));
        // A failed write leaves the old file and no temporary one.
        let rx = chunks(vec![
            Ok(Bytes::from_static(b"half")),
            Err(VfsError::Unreachable("gone".into())),
        ])
        .await;
        assert!(safe_write(&src, &path, rx, Some(x)).await.is_err());
        assert_eq!(std::fs::read(path.as_str()).unwrap(), b"old");
        assert_eq!(only_the_file(d.path()), ["a.bin"]);
        // A file changed meanwhile is not overwritten.
        let stale = Expected { size: 99, ..x };
        let rx = chunks(vec![Ok(Bytes::from_static(b"new"))]).await;
        assert!(matches!(
            safe_write(&src, &path, rx, Some(stale)).await,
            Err(VfsError::Changed(_))
        ));
        assert_eq!(std::fs::read(path.as_str()).unwrap(), b"old");
        let rx = chunks(vec![Ok(Bytes::from_static(b"newer"))]).await;
        assert_eq!(safe_write(&src, &path, rx, Some(x)).await.unwrap(), 5);
        assert_eq!(std::fs::read(path.as_str()).unwrap(), b"newer");
        assert_eq!(only_the_file(d.path()), ["a.bin"]);
        check_writable(&src, &VPath::new(d.path().to_string_lossy()))
            .await
            .unwrap();
        assert_eq!(only_the_file(d.path()), ["a.bin"]);
    }
}
