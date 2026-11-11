//! Network sources (SPEC 6). Every protocol implements [`Source`]; nothing above this module
//! knows which protocol it talks to. Paths are `/`-rooted inside the share, export or base path.
//!
//! Each backend keeps one connection and reconnects transparently: an operation that fails
//! with a transport error drops the connection, reconnects once and retries. Longer outages are
//! the caller's business (the stream layer backs off; the scanner reports the source offline).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{Result, Source, VPath, VfsError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Smb,
    Nfs,
    Webdav,
    Sftp,
    Ftp,
}

impl Protocol {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "smb" | "cifs" => Self::Smb,
            "nfs" => Self::Nfs,
            "webdav" | "dav" | "davs" | "http" | "https" => Self::Webdav,
            "sftp" | "ssh" => Self::Sftp,
            "ftp" => Self::Ftp,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Smb => "smb",
            Self::Nfs => "nfs",
            Self::Webdav => "webdav",
            Self::Sftp => "sftp",
            Self::Ftp => "ftp",
        }
    }
    pub fn default_port(self, tls: bool) -> u16 {
        match self {
            Self::Smb => 445,
            Self::Nfs => 2049,
            Self::Webdav if tls => 443,
            Self::Webdav => 80,
            Self::Sftp => 22,
            Self::Ftp => 21,
        }
    }
}

/// How to reach a network source. Stored as JSON in `source.config_json`, except the password,
/// which lives in the OS keystore and is filled in at runtime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteConfig {
    pub protocol: Protocol,
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
    /// SMB: the share name. NFS: the export path. WebDAV, SFTP, FTP: a base path ("" = root).
    #[serde(default)]
    pub share: String,
    #[serde(default)]
    pub user: String,
    #[serde(skip)]
    pub password: String,
    /// WebDAV over HTTPS.
    #[serde(default)]
    pub tls: bool,
    /// SFTP host key fingerprint, recorded on first connection (trust on first use).
    #[serde(default)]
    pub host_key: Option<String>,
    /// SFTP: a private key file (OpenSSH format) to log in with instead of a password; the
    /// `password` field then holds the key's passphrase, if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_file: Option<String>,
}

impl RemoteConfig {
    pub fn port(&self) -> u16 {
        self.port
            .unwrap_or_else(|| self.protocol.default_port(self.tls))
    }

    /// A display URL without credentials, e.g. `smb://nas/media`.
    pub fn display_url(&self) -> String {
        let port = self.port.map(|p| format!(":{p}")).unwrap_or_default();
        let scheme = match (self.protocol, self.tls) {
            (Protocol::Webdav, true) => "https",
            (Protocol::Webdav, false) => "http",
            (p, _) => p.as_str(),
        };
        let share = self.share.trim_matches('/');
        format!("{scheme}://{}{port}/{share}", self.host)
    }
}

/// Connect to a network source and check that its root can be listed.
#[allow(unreachable_code)] // with no backend features every protocol is "not built in"
pub async fn connect(cfg: RemoteConfig) -> Result<Arc<dyn RemoteSource>> {
    let src: Arc<dyn RemoteSource> = match cfg.protocol {
        #[cfg(feature = "smb")]
        Protocol::Smb => Arc::new(crate::smb::SmbSource::new(cfg)),
        #[cfg(feature = "nfs")]
        Protocol::Nfs => Arc::new(crate::nfs::NfsSource::new(cfg)),
        #[cfg(feature = "webdav")]
        Protocol::Webdav => Arc::new(crate::webdav::WebdavSource::new(cfg)?),
        #[cfg(feature = "ftp")]
        Protocol::Ftp => Arc::new(crate::ftp::FtpSource::new(cfg)),
        #[allow(unreachable_patterns)]
        p => {
            return Err(VfsError::Unsupported(format!(
                "{} support is not built in",
                p.as_str()
            )));
        }
    };
    src.list(&VPath::new("/")).await?;
    Ok(src)
}

/// A network [`Source`] that may learn something worth persisting while connecting.
pub trait RemoteSource: Source {
    /// SFTP: the server's host key fingerprint seen on connection (to store for next time).
    fn host_key(&self) -> Option<String> {
        None
    }
}

/// `"/Movies/Alien (1979)"` -> `"Movies/Alien (1979)"`.
pub fn rel(path: &VPath) -> &str {
    path.as_str().trim_start_matches('/')
}

/// Join a base path and a source path into an absolute server path with `/` separators.
pub fn join(base: &str, path: &VPath) -> String {
    let base = base.trim_matches('/');
    let p = rel(path);
    match (base.is_empty(), p.is_empty()) {
        (true, true) => "/".into(),
        (true, false) => format!("/{p}"),
        (false, true) => format!("/{base}"),
        (false, false) => format!("/{base}/{p}"),
    }
}

/// Map an error message from a protocol library to a category, for libraries without typed
/// errors. Server status names are matched in upper case.
pub fn classify(msg: &str) -> fn(String) -> VfsError {
    // "STATUS_LOGON_FAILURE" and "Logon Failure" alike.
    let m = msg.to_ascii_uppercase().replace('_', " ");
    let any = |keys: &[&str]| keys.iter().any(|k| m.contains(k));
    if any(&[
        "LOGON FAILURE",
        "ACCESS DENIED",
        "AUTHENTICATION",
        "PERMISSION",
        "401",
        "530 ",
        "NOT LOGGED",
        "LOGIN",
        "WRONG PASSWORD",
        "AUTH",
    ]) {
        VfsError::Auth
    } else if any(&[
        "NOT FOUND",
        "NO SUCH",
        "NOENT",
        "404",
        "550 ",
        "DOES NOT EXIST",
        "BAD NETWORK NAME",
        "OBJECT NAME",
    ]) {
        VfsError::NotFound
    } else if any(&[
        "CONNECT",
        "REFUSED",
        "TIMED OUT",
        "TIMEOUT",
        "RESET",
        "BROKEN PIPE",
        "UNREACHABLE",
        "EOF",
        "DISCONNECT",
        "CLOSED",
        "NETWORK",
    ]) {
        VfsError::Unreachable
    } else {
        VfsError::Internal
    }
}

pub fn err(msg: impl std::fmt::Display) -> VfsError {
    let s = msg.to_string();
    classify(&s)(s)
}

/// Worth reconnecting for: the connection itself failed, not the request.
pub fn retryable(e: &VfsError) -> bool {
    matches!(e, VfsError::Unreachable(_) | VfsError::Internal(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_urls() {
        assert_eq!(join("", &VPath::new("/")), "/");
        assert_eq!(
            join("/media/", &VPath::new("/Movies/A.mkv")),
            "/media/Movies/A.mkv"
        );
        assert_eq!(join("media", &VPath::new("/")), "/media");
        let c = RemoteConfig {
            protocol: Protocol::Webdav,
            host: "nas".into(),
            port: Some(5006),
            share: "/video/".into(),
            user: "u".into(),
            password: "secret".into(),
            tls: true,
            host_key: None,
            key_file: None,
        };
        assert_eq!(c.display_url(), "https://nas:5006/video");
        let json = serde_json::to_string(&c).unwrap();
        assert!(!json.contains("secret"), "passwords never serialise");
        assert!(matches!(
            classify("STATUS_LOGON_FAILURE")("x".into()),
            VfsError::Auth(_)
        ));
        assert!(matches!(
            classify("550 No such file")("x".into()),
            VfsError::NotFound(_)
        ));
        assert!(matches!(
            classify("Connection refused")("x".into()),
            VfsError::Unreachable(_)
        ));
    }
}
