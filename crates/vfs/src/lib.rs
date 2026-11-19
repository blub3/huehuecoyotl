//! Sources: one small trait for every protocol (SPEC section 6).
//!
//! Nothing above this crate knows which protocol a file lives on. M1 ships the local backend;
//! SMB, NFS, WebDAV, SFTP and FTP follow in M3 behind Cargo features.

#[cfg(feature = "ftp")]
mod ftp;
mod local;
mod media;
#[cfg(feature = "nfs")]
mod nfs;
pub mod remote;
#[cfg(feature = "smb")]
mod smb;
#[cfg(feature = "smb")]
mod srvsvc;
#[cfg(feature = "webdav")]
mod webdav;

pub use local::LocalSource;
pub use media::{MediaKind, find_sidecar_subtitles, media_kind};
pub use remote::{Protocol, RemoteConfig, RemoteSource};
#[cfg(feature = "smb")]
pub use smb::list_shares as smb_shares;

use async_trait::async_trait;
use bytes::Bytes;

/// A path inside a source, always with `/` separators. For the local source it is the
/// absolute OS path (on Windows `C:/Users/...`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VPath(String);

impl VPath {
    pub fn new(p: impl Into<String>) -> Self {
        Self(p.into().replace('\\', "/"))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn file_name(&self) -> &str {
        self.0
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
    }
    pub fn parent(&self) -> Option<VPath> {
        let t = self.0.trim_end_matches('/');
        let i = t.rfind('/')?;
        Some(VPath(if i == 0 {
            "/".into()
        } else {
            t[..i].to_string()
        }))
    }
    pub fn join(&self, name: &str) -> VPath {
        if self.0.ends_with('/') {
            VPath(format!("{}{}", self.0, name))
        } else {
            VPath(format!("{}/{}", self.0, name))
        }
    }
}

impl std::fmt::Display for VPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: VPath,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// Seconds since the Unix epoch; 0 if unknown.
    pub mtime: i64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Caps {
    pub range_reads: bool,
    pub change_notifications: bool,
    pub stable_ids: bool,
    /// mpv can open the path itself, so no stream callbacks are needed.
    pub native_paths: bool,
}

/// Typed error categories the UI can present (SPEC 8, rule 6).
#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("unreachable: {0}")]
    Unreachable(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("{0}")]
    Internal(String),
}

impl From<std::io::Error> for VfsError {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind::*;
        match e.kind() {
            NotFound => VfsError::NotFound(e.to_string()),
            PermissionDenied => VfsError::Auth(e.to_string()),
            TimedOut | ConnectionRefused | ConnectionReset | HostUnreachable
            | NetworkUnreachable => VfsError::Unreachable(e.to_string()),
            _ => VfsError::Internal(e.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, VfsError>;

#[async_trait]
pub trait Source: Send + Sync {
    async fn list(&self, dir: &VPath) -> Result<Vec<Entry>>;
    async fn stat(&self, path: &VPath) -> Result<Entry>;
    async fn read_at(&self, path: &VPath, offset: u64, len: u32) -> Result<Bytes>;
    fn capabilities(&self) -> Caps;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vpath_ops() {
        let p = VPath::new(r"C:\Movies\Alien (1979)\Alien.mkv");
        assert_eq!(p.as_str(), "C:/Movies/Alien (1979)/Alien.mkv");
        assert_eq!(p.file_name(), "Alien.mkv");
        assert_eq!(p.parent().unwrap().as_str(), "C:/Movies/Alien (1979)");
        assert_eq!(VPath::new("/a").parent().unwrap().as_str(), "/");
        assert_eq!(VPath::new("/").join("home").as_str(), "/home");
        assert_eq!(VPath::new("/home").join("x").as_str(), "/home/x");
    }
}
