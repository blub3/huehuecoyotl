//! Network shares (2.8, issue #118, SPEC 6 "Shares"): one share, any number of library folders.
//! The share keeps the settings and the password (keystore entry `share:<id>`); its folders
//! share one live connection. At start-up, passwords of folders from before shares move to their
//! share's entry.

use std::sync::Arc;

use huehuecoyotl_library::{catalog, jobs, shares};
use huehuecoyotl_vfs::{RemoteConfig, VPath};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::library::enqueue_scan;
use crate::{CResult, Core, CoreError, ErrorKind, Event, Inner, secrets};

/// Move each share's password from its folders' old entries (`source:<id>`) to the share's own
/// (`share:<id>`), once. The newest folder's password wins; old entries with that same password
/// are deleted, a different one is kept until a connection with the share's works (then it is
/// deleted too, by [`forget_legacy`]). Blocking keystore calls run off the async threads.
pub(crate) async fn move_passwords(inner: &Arc<Inner>) {
    let Ok(list) = inner.lib.read(shares::list).await else {
        return;
    };
    for s in list {
        let id = s.id;
        let Ok(legacy) = inner
            .lib
            .read(move |c| shares::legacy_credentials(c, id))
            .await
        else {
            continue;
        };
        let _ = tokio::task::spawn_blocking(move || {
            let own = shares::credential_ref(id);
            if !secrets::load_password(&own).is_empty() {
                return;
            }
            let found = legacy
                .iter()
                .map(|r| (r.clone(), secrets::load_password(r)))
                .find(|(_, p)| !p.is_empty());
            let Some((_, password)) = found else { return };
            if secrets::store_password(&own, &password).is_err() {
                return; // keep the old entries: they still work
            }
            for r in &legacy {
                if secrets::load_password(r) == password {
                    secrets::delete_password(r);
                }
            }
            tracing::info!(
                share = id,
                "moved the share's password to its own keystore entry"
            );
        })
        .await;
    }
}

/// After a connection with the share's password worked: the folders' old entries go.
fn forget_legacy(legacy: Vec<String>) {
    for r in legacy {
        if !secrets::load_password(&r).is_empty() {
            secrets::delete_password(&r);
        }
    }
}

impl Inner {
    /// Issue #118: a live connection of another folder of the same share, if one is open.
    pub(crate) fn share_connection(
        &self,
        siblings: &[i64],
    ) -> Option<Arc<dyn huehuecoyotl_vfs::RemoteSource>> {
        let remotes = self.remotes.lock().unwrap();
        siblings.iter().find_map(|id| remotes.get(id).cloned())
    }

    /// The first connection of a share worked: old keystore entries of its folders can go.
    pub(crate) async fn share_connected(self: &Arc<Self>, share: i64) {
        if let Ok(legacy) = self
            .lib
            .read(move |c| shares::legacy_credentials(c, share))
            .await
        {
            let _ = tokio::task::spawn_blocking(move || forget_legacy(legacy)).await;
        }
    }
}

/// A folder to add: its path inside the share and what it holds.
#[derive(Clone, Debug, Deserialize)]
struct NewFolder {
    path: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    name: String,
}

/// Every share with its folders and states, and the folders on this computer, as JSON for the
/// Sources page: `shares: [{id, kind, name, protocol, host, share, user, url, online, folders:
/// [{sourceId, name, root, url, content, files, lastScan, scanning, online}]}]`, `local: [folder]`.
pub(crate) async fn overview(inner: &Arc<Inner>) -> CResult<Value> {
    let (list, libs) = inner
        .lib
        .read(|c| Ok((shares::list(c)?, catalog::libraries(c)?)))
        .await?;
    let scanning = inner.scanning.lock().unwrap().clone();
    let offline = inner.offline.lock().unwrap().clone();
    let folder =
        |id: i64, name: &str, root: &str, url: String, content: &str, files: i64, last: i64| {
            json!({
                "sourceId": id, "name": name, "root": root, "url": url, "content": content,
                "files": files, "lastScan": last, "scanning": scanning.contains(&id),
                "online": !offline.contains(&id),
            })
        };
    let shares_json: Vec<Value> = list
        .iter()
        .map(|s| {
            let cfg: RemoteConfig =
                serde_json::from_str(&s.config_json).unwrap_or_else(|_| RemoteConfig {
                    protocol: huehuecoyotl_vfs::Protocol::Smb,
                    host: String::new(),
                    port: None,
                    share: String::new(),
                    user: String::new(),
                    password: String::new(),
                    tls: false,
                    host_key: None,
                    key_file: None,
                });
            let folders: Vec<Value> = s
                .folders
                .iter()
                .map(|f| {
                    folder(
                        f.source_id,
                        &f.name,
                        &f.root,
                        crate::network::display_url(&s.kind, &s.config_json, &f.root),
                        &f.content,
                        f.files,
                        f.last_scan,
                    )
                })
                .collect();
            json!({
                "id": s.id,
                "name": s.name,
                "protocol": s.kind,
                "host": cfg.host,
                "share": cfg.share,
                "user": cfg.user,
                "url": cfg.display_url(),
                "online": !s.folders.iter().any(|f| offline.contains(&f.source_id)),
                "folders": folders,
            })
        })
        .collect();
    let local: Vec<Value> = libs
        .iter()
        .filter(|l| l.kind == "local")
        .map(|l| {
            folder(
                l.id,
                &l.name,
                &l.root,
                l.root.clone(),
                &l.content,
                l.files,
                l.last_scan.unwrap_or(0),
            )
        })
        .collect();
    Ok(json!({ "shares": shares_json, "local": local }))
}

fn overview_event(request_id: u64, v: Value) -> Event {
    Event::MusicDetail {
        request_id,
        kind: "shares".into(),
        json: v.to_string(),
    }
}

/// Add folders to a share: each must be a folder there; one scan each. Returns the new sources.
pub(crate) async fn add_folders(
    inner: &Arc<Inner>,
    share: i64,
    folders: Vec<(String, String, String)>,
) -> CResult<Vec<i64>> {
    let (any, config_json) = inner
        .lib
        .read(move |c| {
            let s = shares::get(c, share)?;
            Ok((
                s.as_ref()
                    .and_then(|s| s.folders.first().map(|f| f.source_id)),
                s.map(|s| s.config_json),
            ))
        })
        .await?;
    let config_json =
        config_json.ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("share {share}")))?;
    let cfg: RemoteConfig = serde_json::from_str(&config_json)
        .map_err(|e| CoreError::new(ErrorKind::Internal, format!("share {share} settings: {e}")))?;
    // A connection to check the folders: a folder's, else a new one with the share's password.
    let src = match any {
        Some(id) => inner.remote_source(id).await?,
        None => {
            let mut c = cfg.clone();
            let reference = shares::credential_ref(share);
            c.password = tokio::task::spawn_blocking(move || secrets::load_password(&reference))
                .await
                .unwrap_or_default();
            crate::network::connect_config(c).await?
        }
    };
    let mut added = Vec::new();
    for (path, content, name) in folders {
        let root = {
            let p = path.trim().trim_matches('/');
            if p.is_empty() {
                String::new()
            } else {
                format!("/{p}")
            }
        };
        let st = src
            .stat(&VPath::new(if root.is_empty() { "/" } else { &root }))
            .await?;
        if !st.is_dir {
            return Err(CoreError::new(
                ErrorKind::Unsupported,
                format!("{root} is not a folder"),
            ));
        }
        let name = if name.trim().is_empty() {
            match root.rsplit('/').find(|s| !s.is_empty()) {
                Some(l) => format!("{l} on {}", cfg.host),
                None => shares::default_name(&config_json),
            }
        } else {
            name.trim().to_string()
        };
        let content = crate::library::content_type(&content).to_string();
        let (id, created) = inner
            .lib
            .write(move |c| shares::add_folder(c, share, &root, &content, &name))
            .await?;
        inner
            .remotes
            .lock()
            .unwrap()
            .entry(id)
            .or_insert_with(|| src.clone());
        if created {
            enqueue_scan(inner, id, jobs::priority::USER).await;
            added.push(id);
        }
    }
    inner.mark_changed();
    Ok(added)
}

/// Remove a share: its folders (with what only they provided, and their saved copies), then
/// the share and its password.
pub(crate) async fn remove_share(inner: &Arc<Inner>, share: i64) -> CResult<()> {
    let ids = inner
        .lib
        .read(move |c| shares::folder_ids(c, share))
        .await?;
    for id in ids {
        crate::download::delete_saved(inner, Some(id)).await?;
        inner
            .lib
            .write(move |c| catalog::remove_library(c, id))
            .await?;
        inner.forget_source(id);
    }
    let reference = inner.lib.write(move |c| shares::remove(c, share)).await?;
    if let Some(r) = reference {
        let _ = tokio::task::spawn_blocking(move || secrets::delete_password(&r)).await;
    }
    inner.mark_changed();
    Ok(())
}

impl Core {
    /// Issue #118: the shares with their folders, and the folders on this computer
    /// (`MusicDetail` of kind "shares").
    pub fn shares(&self, request_id: u64) {
        self.spawn(request_id, move |inner| async move {
            Ok(overview_event(request_id, overview(&inner).await?))
        });
    }

    /// Issue #118: add folders to a share (`folders_json`: `[{path, content, name}]`); answers
    /// with the shares.
    pub fn share_add_folders(&self, request_id: u64, share: i64, folders_json: &str) {
        let parsed: Result<Vec<NewFolder>, _> = serde_json::from_str(folders_json);
        self.spawn(request_id, move |inner| async move {
            let folders = parsed
                .map_err(|e| {
                    CoreError::new(ErrorKind::Unsupported, format!("invalid folders: {e}"))
                })?
                .into_iter()
                .map(|f| (f.path, f.content, f.name))
                .collect();
            add_folders(&inner, share, folders).await?;
            Ok(overview_event(request_id, overview(&inner).await?))
        });
    }

    /// Issue #118: remove a share with all its folders and its password; answers with the shares.
    pub fn share_remove(&self, request_id: u64, share: i64) {
        self.spawn(request_id, move |inner| async move {
            remove_share(&inner, share).await?;
            Ok(overview_event(request_id, overview(&inner).await?))
        });
    }
}
