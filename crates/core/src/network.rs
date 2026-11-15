//! M3: network sources (SPEC 6). Libraries on SMB, NFS, WebDAV, FTP and SFTP shares, their passwords
//! in the OS keystore, one live connection per source, offline tracking, the folder picker of
//! the "add network share" dialog, playback through `coyotl://` URLs and LAN discovery.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use huehuecoyotl_library::catalog;
use huehuecoyotl_library::jobs;
use huehuecoyotl_vfs::{
    MediaKind, Protocol, RemoteConfig, RemoteSource, VPath, VfsError, find_sidecar_subtitles,
    media_kind, remote,
};
use serde::Deserialize;

use crate::library::{enqueue_scan, library_list};
use crate::{
    CResult, Core, CoreError, ErrorKind, Event, FsEntry, Inner, Ticket, lang, queries, secrets,
    sidecar,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// What the "add network share" dialog sends: where the share is, which folder inside it to use
/// as the library, and a display name. The password travels separately.
#[derive(Clone, Debug, Deserialize)]
pub struct NetworkSpec {
    #[serde(flatten)]
    pub cfg: RemoteConfig,
    /// Folder inside the share ("" or "/" = the whole share).
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
}

impl NetworkSpec {
    pub fn parse(json: &str) -> CResult<Self> {
        let mut s: Self = serde_json::from_str(json).map_err(|e| {
            CoreError::new(
                ErrorKind::Unsupported,
                format!("invalid share settings: {e}"),
            )
        })?;
        s.cfg.host = s.cfg.host.trim().to_string();
        s.cfg.share = s.cfg.share.trim().trim_matches(['/', '\\']).to_string();
        if s.cfg.protocol == Protocol::Nfs && !s.cfg.share.is_empty() {
            s.cfg.share = format!("/{}", s.cfg.share); // an export path
        }
        s.cfg.user = s.cfg.user.trim().to_string();
        if s.cfg.host.is_empty() {
            return Err(CoreError::new(
                ErrorKind::Unsupported,
                "Please enter a server name or address.",
            ));
        }
        if s.cfg.protocol == Protocol::Smb && s.cfg.share.is_empty() {
            return Err(CoreError::new(
                ErrorKind::Unsupported,
                "Please enter the name of the share.",
            ));
        }
        let p = s.path.trim().trim_matches('/');
        s.path = if p.is_empty() {
            String::new()
        } else {
            format!("/{p}")
        };
        Ok(s)
    }

    /// The configuration as stored in `source.config_json` (never with the password).
    pub fn config_json(&self) -> String {
        serde_json::to_string(&self.cfg).unwrap_or_default()
    }
}

/// Display URL of a stored network source, e.g. `smb://nas/media/Movies`.
pub(crate) fn display_url(kind: &str, config_json: &str, root: &str) -> String {
    if kind == "local" {
        return root.to_string();
    }
    match serde_json::from_str::<RemoteConfig>(config_json) {
        Ok(c) => format!("{}{}", c.display_url().trim_end_matches('/'), root),
        Err(_) => format!("{kind}://?"),
    }
}

fn timeout_err() -> CoreError {
    CoreError::new(ErrorKind::Unreachable, "The server did not answer in time.")
}

async fn connect(cfg: RemoteConfig) -> CResult<Arc<dyn RemoteSource>> {
    let attempt = async move {
        match cfg.protocol {
            // SFTP lives in its own crate (russh), which the vfs crate cannot depend on.
            Protocol::Sftp => huehuecoyotl_vfs_sftp::connect(cfg).await,
            _ => remote::connect(cfg).await,
        }
    };
    match tokio::time::timeout(CONNECT_TIMEOUT, attempt).await {
        Ok(r) => Ok(r?),
        Err(_) => Err(timeout_err()),
    }
}

impl Inner {
    /// The live connection of network source `id`, connecting on first use. Marks the source
    /// offline or online as a side effect.
    pub(crate) async fn remote_source(self: &Arc<Self>, id: i64) -> CResult<Arc<dyn RemoteSource>> {
        if let Some(s) = self.remotes.lock().unwrap().get(&id) {
            return Ok(s.clone());
        }
        let row = self
            .lib
            .read(move |c| catalog::source(c, id))
            .await?
            .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("source {id}")))?;
        let mut cfg: RemoteConfig = serde_json::from_str(&row.config_json).map_err(|e| {
            CoreError::new(ErrorKind::Internal, format!("source {id} settings: {e}"))
        })?;
        if let Some(r) = row.credential_ref.clone() {
            cfg.password = tokio::task::spawn_blocking(move || secrets::load_password(&r))
                .await
                .unwrap_or_default();
        }
        let had_key = cfg.host_key.is_some();
        let res = connect(cfg.clone()).await;
        let src = match res {
            Ok(s) => s,
            Err(e) => {
                if matches!(e.kind, ErrorKind::Unreachable) {
                    self.set_online(id, false);
                }
                return Err(e);
            }
        };
        // Trust on first use: remember the host key so a different server is refused later.
        if let (false, Some(k)) = (had_key, src.host_key()) {
            cfg.host_key = Some(k);
            let json = serde_json::to_string(&cfg).unwrap_or_default();
            let _ = self
                .lib
                .write(move |c| catalog::set_source_config(c, id, &json))
                .await;
        }
        self.set_online(id, true);
        self.remotes.lock().unwrap().insert(id, src.clone());
        Ok(src)
    }

    pub(crate) fn set_online(&self, id: i64, online: bool) {
        let changed = {
            let mut off = self.offline.lock().unwrap();
            if online {
                off.remove(&id)
            } else {
                off.insert(id)
            }
        };
        if changed {
            tracing::info!(source_id = id, online, "network source state changed");
            if !online {
                self.remotes.lock().unwrap().remove(&id);
            }
            // The Sources page re-reads its list on ScanState.
            self.sink.post(Event::ScanState {
                source_id: id,
                running: false,
                message: if online {
                    String::new()
                } else {
                    "The server cannot be reached.".into()
                },
            });
        }
    }

    pub(crate) fn forget_source(&self, id: i64) {
        self.remotes.lock().unwrap().remove(&id);
        self.offline.lock().unwrap().remove(&id);
    }

    /// The connection the dialog's folder picker uses, reused while the settings stay the same.
    async fn probe(&self, spec: &NetworkSpec, password: &str) -> CResult<Arc<dyn RemoteSource>> {
        let key = format!("{}\u{0}{password}", spec.config_json());
        if let Some((k, s)) = self.probe.lock().unwrap().as_ref() {
            if *k == key {
                return Ok(s.clone());
            }
        }
        let mut cfg = spec.cfg.clone();
        cfg.password = password.to_string();
        let s = connect(cfg).await?;
        *self.probe.lock().unwrap() = Some((key, s.clone()));
        Ok(s)
    }
}

/// Every minute, try to reach network sources that are offline; once one answers again it is
/// marked online and rescanned, so changes made meanwhile show up.
pub(crate) fn spawn_reconnect(inner: &Arc<Inner>) {
    let weak = Arc::downgrade(inner);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let Some(inner) = weak.upgrade() else { return };
            let ids: Vec<i64> = inner.offline.lock().unwrap().iter().copied().collect();
            for id in ids {
                if inner.remote_source(id).await.is_ok() {
                    enqueue_scan(&inner, id, jobs::priority::BACKGROUND).await;
                }
            }
        }
    });
}

/// Prepare a file on a network source for playback.
pub(crate) async fn prepare_remote(
    inner: &Arc<Inner>,
    source_id: i64,
    path: &str,
    ticket_id: u64,
) -> CResult<Ticket> {
    let src = inner.remote_source(source_id).await?;
    let vp = VPath::new(path);
    let entry = match src.stat(&vp).await {
        Err(VfsError::Unreachable(m)) => {
            inner.set_online(source_id, false);
            return Err(CoreError::new(ErrorKind::Unreachable, m));
        }
        r => r?,
    };
    let siblings = match vp.parent() {
        Some(dir) => src.list(&dir).await.unwrap_or_default(),
        None => vec![],
    };
    let subtitles = find_sidecar_subtitles(&entry.name, &siblings)
        .into_iter()
        .map(|e| {
            sidecar(
                &entry.name,
                &e.name,
                huehuecoyotl_stream::url(source_id, &e.path),
            )
        })
        .collect();
    let p = vp.to_string();
    let item_id = inner
        .lib
        .read(move |c| catalog::item_for_source_path(c, source_id, &p))
        .await?
        .ok_or_else(|| {
            CoreError::new(ErrorKind::NotFound, format!("{path} is not in the library"))
        })?;
    let (title, state, prefs) = inner
        .lib
        .read(move |c| {
            Ok((
                queries::item_title(c, item_id)?,
                queries::playstate(c, item_id)?,
                queries::track_pref(c, item_id)?,
            ))
        })
        .await?;
    inner.tickets.lock().unwrap().insert(ticket_id, item_id);
    let p3 = vp.to_string();
    if let Ok(Some(file)) = inner
        .lib
        .read(move |c| catalog::file_for_path(c, Some(source_id), &p3))
        .await
    {
        inner.ticket_files.lock().unwrap().insert(ticket_id, file);
    }
    Ok(Ticket {
        ticket: ticket_id,
        item_id,
        title: title.unwrap_or_else(|| entry.name.clone()),
        url: huehuecoyotl_stream::url(source_id, &vp),
        is_network: true,
        resume_ms: if state.watched { 0 } else { state.position_ms },
        subtitles,
        audio_langs: lang::variants(&prefs.audio_lang),
        sub_langs: lang::variants(&prefs.sub_lang),
        prefs,
    })
}

/// A file server found on the local network.
#[derive(Clone, Debug, PartialEq)]
pub struct Discovered {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    /// WebDAV over HTTPS.
    pub tls: bool,
}

const SERVICES: &[(&str, Protocol)] = &[
    ("_smb._tcp.local.", Protocol::Smb),
    ("_nfs._tcp.local.", Protocol::Nfs),
    ("_webdav._tcp.local.", Protocol::Webdav),
    ("_webdavs._tcp.local.", Protocol::Webdav),
    ("_ftp._tcp.local.", Protocol::Ftp),
    ("_sftp-ssh._tcp.local.", Protocol::Sftp),
];

/// Browse mDNS/DNS-SD for `secs` seconds. Blocking: run with `spawn_blocking`.
fn discover_blocking(secs: u64) -> Result<Vec<Discovered>, String> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let mut rxs = Vec::new();
    for (ty, p) in SERVICES {
        match daemon.browse(ty) {
            Ok(rx) => rxs.push((rx, *p)),
            Err(e) => tracing::warn!("mDNS browse {ty}: {e}"),
        }
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    let mut found: HashMap<(String, String), Discovered> = HashMap::new();
    while std::time::Instant::now() < deadline {
        let mut idle = true;
        for (rx, p) in &rxs {
            while let Ok(ev) = rx.try_recv() {
                idle = false;
                if let ServiceEvent::ServiceResolved(info) = ev {
                    let host = info
                        .get_addresses_v4()
                        .into_iter()
                        .next()
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| info.get_hostname().trim_end_matches('.').to_string());
                    let name = info
                        .get_fullname()
                        .split("._")
                        .next()
                        .unwrap_or_default()
                        .to_string();
                    let tls = info.ty_domain.starts_with("_webdavs");
                    // One entry per announced service, even with several addresses.
                    found
                        .entry((
                            p.as_str().to_string() + if tls { "s" } else { "" },
                            name.clone(),
                        ))
                        .or_insert(Discovered {
                            name,
                            protocol: p.as_str().into(),
                            host,
                            port: info.get_port(),
                            tls,
                        });
                }
            }
        }
        if idle {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let _ = daemon.shutdown();
    let mut v: Vec<_> = found.into_values().collect();
    v.sort_by(|a, b| (&a.name, &a.protocol).cmp(&(&b.name, &b.protocol)));
    Ok(v)
}

impl Core {
    /// SFTP: the host key fingerprint of the server the dialog last connected to ("" for other
    /// protocols or before connecting), for the user to check before adding the library.
    pub fn network_host_key(&self) -> String {
        self.inner
            .probe
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|(_, s)| s.host_key())
            .unwrap_or_default()
    }

    /// List a folder of a share that is not a library yet (the dialog's folder picker; listing
    /// "/" doubles as the connection test). Answers with a Folder event of sub-folders and
    /// videos, or a typed Error (wrong password, unreachable, no such share).
    pub fn browse_network(&self, request_id: u64, spec_json: &str, password: &str, path: &str) {
        let (spec_json, password, path) = (
            spec_json.to_string(),
            password.to_string(),
            path.to_string(),
        );
        self.spawn(request_id, move |inner| async move {
            let spec = NetworkSpec::parse(&spec_json)?;
            let src = inner.probe(&spec, &password).await?;
            let dir = VPath::new(if path.is_empty() { "/" } else { &path });
            let listing = match src.list(&dir).await {
                Ok(l) => l,
                Err(e) => {
                    inner.probe.lock().unwrap().take();
                    return Err(e.into());
                }
            };
            let mut entries: Vec<FsEntry> = listing
                .into_iter()
                .filter(|e| e.is_dir || media_kind(&e.name) == MediaKind::Video)
                .map(|e| FsEntry {
                    is_video: !e.is_dir,
                    name: e.name,
                    path: e.path.to_string(),
                    is_dir: e.is_dir,
                    size: e.size,
                    progress: 0.0,
                    watched: false,
                })
                .collect();
            entries.sort_by(|a, b| {
                (!a.is_dir, a.name.to_lowercase()).cmp(&(!b.is_dir, b.name.to_lowercase()))
            });
            let parent = match dir.parent() {
                Some(p) if p != dir => p.to_string(),
                _ => String::new(),
            };
            Ok(Event::Folder {
                request_id,
                path: dir.to_string(),
                parent,
                entries,
            })
        });
    }

    /// Add a folder on a network share as a library: test the connection, store the password
    /// in the keystore, scan.
    pub fn add_network_library(&self, request_id: u64, spec_json: &str, password: &str) {
        let (spec_json, password) = (spec_json.to_string(), password.to_string());
        self.spawn(request_id, move |inner| async move {
            let spec = NetworkSpec::parse(&spec_json)?;
            let src = inner.probe(&spec, &password).await?;
            let root = VPath::new(if spec.path.is_empty() {
                "/"
            } else {
                &spec.path
            });
            let st = src.stat(&root).await?;
            if !st.is_dir {
                return Err(CoreError::new(
                    ErrorKind::Unsupported,
                    format!("{root} is not a folder"),
                ));
            }
            let mut cfg = spec.cfg.clone();
            if cfg.host_key.is_none() {
                cfg.host_key = src.host_key();
            }
            let json = serde_json::to_string(&cfg).unwrap_or_default();
            let name = if spec.name.trim().is_empty() {
                let last = spec.path.rsplit('/').find(|s| !s.is_empty());
                match last {
                    Some(l) => format!("{l} on {}", cfg.host),
                    None if !cfg.share.is_empty() => {
                        format!("{} on {}", cfg.share.trim_matches('/'), cfg.host)
                    }
                    None => cfg.host.clone(),
                }
            } else {
                spec.name.trim().to_string()
            };
            let (kind, root_s) = (cfg.protocol.as_str().to_string(), spec.path.clone());
            let id = inner
                .lib
                .write(move |c| catalog::add_network_library(c, &kind, &name, &json, &root_s))
                .await?;
            let reference = format!("source:{id}");
            let warning =
                tokio::task::spawn_blocking(move || secrets::store_password(&reference, &password))
                    .await
                    .unwrap_or(Ok(()));
            if let Err(w) = warning {
                tracing::warn!("{w}");
            }
            inner.remotes.lock().unwrap().insert(id, src);
            inner.probe.lock().unwrap().take();
            enqueue_scan(&inner, id, jobs::priority::USER).await;
            Ok(Event::Libraries {
                request_id,
                libraries: library_list(&inner).await?,
            })
        });
    }

    /// Look for file servers on the local network for a few seconds.
    pub fn discover(&self, request_id: u64) {
        self.spawn(request_id, move |_inner| async move {
            let devices = tokio::task::spawn_blocking(|| discover_blocking(3))
                .await
                .map_err(|e| CoreError::new(ErrorKind::Internal, e.to_string()))?
                .map_err(|e| CoreError::new(ErrorKind::Unsupported, e))?;
            Ok(Event::Discovered {
                request_id,
                devices,
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_parsing() {
        let s = NetworkSpec::parse(
            r#"{"protocol":"smb","host":" nas ","share":"/media/","user":"me","path":"Movies/"}"#,
        )
        .unwrap();
        assert_eq!(
            (s.cfg.host.as_str(), s.cfg.share.as_str(), s.path.as_str()),
            ("nas", "media", "/Movies")
        );
        assert!(!s.config_json().contains("password"));
        let n = NetworkSpec::parse(r#"{"protocol":"nfs","host":"h","share":"srv/media"}"#).unwrap();
        assert_eq!(n.cfg.share, "/srv/media");
        assert!(NetworkSpec::parse(r#"{"protocol":"smb","host":"h"}"#).is_err());
        assert!(NetworkSpec::parse(r#"{"protocol":"gopher","host":"h"}"#).is_err());
        assert_eq!(
            display_url("smb", &s.config_json(), "/Movies"),
            "smb://nas/media/Movies"
        );
    }
}
