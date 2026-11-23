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
    let (list, libs, library, gone) = inner
        .lib
        .read(|c| {
            let libs = catalog::libraries(c)?;
            let mut gone = std::collections::HashMap::new();
            for l in &libs {
                let g = huehuecoyotl_library::managed::gone_counts(c, l.id)?;
                if g.0 > 0 {
                    gone.insert(l.id, g);
                }
            }
            let own: std::collections::HashSet<i64> = libs
                .iter()
                .map(|l| l.id)
                .filter(|id| {
                    huehuecoyotl_library::managed::is_library_source(c, *id).unwrap_or(false)
                })
                .collect();
            let place = huehuecoyotl_library::managed::place(c)?;
            let copies = huehuecoyotl_library::managed::copies(c)?;
            Ok((shares::list(c)?, libs, (own, place, copies), gone))
        })
        .await?;
    let (own, place, copies) = library;
    let scanning = inner.scanning.lock().unwrap().clone();
    let offline = inner.offline.lock().unwrap().clone();
    let folder =
        |id: i64, name: &str, root: &str, url: String, content: &str, files: i64, last: i64| {
            json!({
                "sourceId": id, "name": name, "root": root, "url": url, "content": content,
                "files": files, "lastScan": last, "scanning": scanning.contains(&id),
                "online": !offline.contains(&id),
                "gone": gone.get(&id).map(|g| json!({ "total": g.0, "kept": g.1, "left": g.2 })),
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
                .filter(|f| !own.contains(&f.source_id))
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
                "port": cfg.port,
                "tls": cfg.tls,
                "keyFile": cfg.key_file,
                "url": cfg.display_url(),
                "online": !s.folders.iter().any(|f| offline.contains(&f.source_id)),
                "folders": folders,
            })
        })
        .collect();
    let local: Vec<Value> = libs
        .iter()
        .filter(|l| l.kind == "local" && !own.contains(&l.id))
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
    let share_name = place
        .as_ref()
        .and_then(|p| p.share)
        .and_then(|id| list.iter().find(|s| s.id == id))
        .map(|s| s.name.clone());
    let library_json = place.as_ref().map(|p| {
        json!({ "path": p.root, "shareName": share_name, "copies": copies.0, "size": copies.1 })
    });
    Ok(json!({ "shares": shares_json, "local": local, "library": library_json }))
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
    open: Option<Arc<dyn huehuecoyotl_vfs::RemoteSource>>,
) -> CResult<Vec<i64>> {
    let config_json = inner
        .lib
        .read(move |c| Ok(shares::get(c, share)?.map(|s| s.config_json)))
        .await?
        .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("share {share}")))?;
    let cfg: RemoteConfig = serde_json::from_str(&config_json)
        .map_err(|e| CoreError::new(ErrorKind::Internal, format!("share {share} settings: {e}")))?;
    let src = match open {
        Some(s) => s,
        None => share_connection(inner, share).await?,
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
    // Issue #150: a share holding the library stays until the library has moved.
    let check = ids.clone();
    let holds_library = inner
        .lib
        .read(move |c| {
            for id in &check {
                if huehuecoyotl_library::managed::is_library_source(c, *id)? {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .await?;
    if holds_library {
        return Err(CoreError::new(
            ErrorKind::Unsupported,
            "your library is on this share; move it under Settings → Library first",
        ));
    }
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

/// A connection to a share: one of its folders', else a new one with the share's password.
pub(crate) async fn share_connection(
    inner: &Arc<Inner>,
    share: i64,
) -> CResult<Arc<dyn huehuecoyotl_vfs::RemoteSource>> {
    let found = inner
        .lib
        .read(move |c| shares::get(c, share))
        .await?
        .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("share {share}")))?;
    if let Some(f) = found.folders.first() {
        return inner.remote_source(f.source_id).await;
    }
    let mut c: RemoteConfig = serde_json::from_str(&found.config_json)
        .map_err(|e| CoreError::new(ErrorKind::Internal, format!("share {share} settings: {e}")))?;
    let reference = shares::credential_ref(share);
    c.password = tokio::task::spawn_blocking(move || secrets::load_password(&reference))
        .await
        .unwrap_or_default();
    crate::network::connect_config(c).await
}

// ---- browsing a share (issue #120) -------------------------------------------------------------

/// What a quick look into a folder found: video files, audio files, audiobook files (M4B, AAX)
/// and sub-folders, within two levels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Look {
    pub videos: u32,
    pub audio: u32,
    pub books: u32,
    pub folders: u32,
}

/// At most this many sub-folders of a folder are looked into (the second level).
const LOOK_SUBFOLDERS: usize = 12;
/// A browse spends at most this long looking into folders; the rest show no guess.
const LOOK_BUDGET: std::time::Duration = std::time::Duration::from_secs(4);

fn count(look: &mut Look, entries: &[huehuecoyotl_vfs::Entry]) {
    for e in entries {
        if e.is_dir {
            continue;
        }
        let lower = e.name.to_lowercase();
        if lower.ends_with(".m4b") || lower.ends_with(".aax") || lower.ends_with(".aaxc") {
            look.books += 1;
        } else {
            match huehuecoyotl_vfs::media_kind(&e.name) {
                huehuecoyotl_vfs::MediaKind::Video => look.videos += 1,
                huehuecoyotl_vfs::MediaKind::Audio => look.audio += 1,
                _ => {}
            }
        }
    }
}

async fn look_into(src: &dyn huehuecoyotl_vfs::RemoteSource, dir: &VPath) -> Look {
    let mut look = Look::default();
    let Ok(first) = src.list(dir).await else {
        return look;
    };
    count(&mut look, &first);
    let subs: Vec<_> = first.iter().filter(|e| e.is_dir).collect();
    look.folders = subs.len() as u32;
    for d in subs.iter().take(LOOK_SUBFOLDERS) {
        if let Ok(second) = src.list(&d.path).await {
            count(&mut look, &second);
        }
    }
    look
}

/// What a folder probably holds: by its name first ("Filme", "Music", "Hörbücher"), else by
/// what the quick look found. `None` when nothing suggests media.
pub(crate) fn guess(name: &str, look: Option<Look>) -> Option<&'static str> {
    let n = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&[
        "hörbuch",
        "hörbücher",
        "hörspiel",
        "audiobook",
        "audio book",
        "audible",
    ]) {
        return Some("audiobooks");
    }
    // "Concerts" can be either: what the look saw decides.
    if n.contains("concert") && look.is_some_and(|l| l.videos > l.audio) {
        return Some("video");
    }
    if has(&[
        "music", "musik", "musique", "mp3", "flac", "hi-res", "hires", "concert",
    ]) && !has(&["video"])
    {
        return Some("music");
    }
    if has(&[
        "movie", "film", "kino", "tv", "show", "serie", "series", "episode", "video", "kids",
    ]) {
        return Some("video");
    }
    let l = look?;
    if l.books > 0 {
        Some("audiobooks")
    } else if l.videos > 0 && l.videos >= l.audio / 4 {
        Some("video")
    } else if l.audio > 0 {
        Some("music")
    } else {
        None
    }
}

/// Where to browse: a share that exists, or a new one being added (its settings and password).
pub(crate) enum Target {
    Share(i64),
    New(Box<crate::network::NetworkSpec>, String),
}

/// The folders in `path` of a share, with a quick look into each, what is already in the
/// library, and folders inside one already added (`MusicDetail` of kind "shareFolders": `path`,
/// `parent`, `folders: [{name, path, guess, videos, audio, books, folders, looked, inLibrary:
/// [content], inside}]`).
pub(crate) async fn browse(inner: &Arc<Inner>, target: Target, path: &str) -> CResult<Value> {
    let (src, share) = match target {
        Target::Share(id) => (share_connection(inner, id).await?, Some(id)),
        Target::New(spec, password) => {
            let src = inner.probe(&spec, &password).await?;
            let (kind, json) = (spec.cfg.protocol.as_str().to_string(), spec.config_json());
            let existing = inner
                .lib
                .read(move |c| shares::find(c, &kind, &json))
                .await?;
            (src, existing)
        }
    };
    let added: Vec<(String, String, String)> = match share {
        Some(id) => inner
            .lib
            .read(move |c| shares::get(c, id))
            .await?
            .map(|s| {
                s.folders
                    .into_iter()
                    .map(|f| (f.root, f.content, f.name))
                    .collect()
            })
            .unwrap_or_default(),
        None => vec![],
    };
    let dir = VPath::new(if path.is_empty() { "/" } else { path });
    let listing = src.list(&dir).await?;
    let mut subs: Vec<_> = listing
        .into_iter()
        .filter(|e| e.is_dir && !e.name.starts_with('.'))
        .collect();
    subs.sort_by_key(|e| e.name.to_lowercase());
    let deadline = tokio::time::Instant::now() + LOOK_BUDGET;
    let mut folders = Vec::new();
    for e in subs {
        let p = e.path.to_string();
        let look = if tokio::time::Instant::now() < deadline {
            tokio::time::timeout_at(deadline, look_into(&*src, &e.path))
                .await
                .ok()
        } else {
            None
        };
        let in_library: Vec<&str> = added
            .iter()
            .filter(|(root, _, _)| *root == p || (root.is_empty() && p == "/"))
            .map(|(_, c, _)| c.as_str())
            .collect();
        let inside = added
            .iter()
            .find(|(root, _, _)| {
                p.starts_with(&format!("{}/", root.trim_end_matches('/'))) && root != &p
            })
            .map(|(_, _, name)| name.clone());
        let l = look.unwrap_or_default();
        folders.push(json!({
            "name": e.name,
            "path": p,
            "guess": guess(&e.name, look),
            "looked": look.is_some(),
            "videos": l.videos,
            "audio": l.audio,
            "books": l.books,
            "folders": l.folders,
            "inLibrary": in_library,
            "inside": inside,
        }));
    }
    let parent = match dir.parent() {
        Some(p) if p != dir => p.to_string(),
        _ => String::new(),
    };
    Ok(json!({ "share": share, "path": dir.to_string(), "parent": parent, "folders": folders }))
}

// ---- adding a share in three steps (issue #121) ------------------------------------------------

/// How long signing in may take before the server counts as not answering.
const SIGN_IN_LIMIT: std::time::Duration = std::time::Duration::from_secs(20);

fn parse_folders(folders_json: &str) -> CResult<Vec<(String, String, String)>> {
    let parsed: Vec<NewFolder> = serde_json::from_str(folders_json)
        .map_err(|e| CoreError::new(ErrorKind::Unsupported, format!("invalid folders: {e}")))?;
    Ok(parsed
        .into_iter()
        .map(|f| (f.path, f.content, f.name))
        .collect())
}

/// Step 2, signing in. An SMB server without a share answers with the shares it offers to this
/// user, each with the share in the library it already is (`{shares: [{name, existing}]}`).
/// Otherwise the connection is tested: `{connected: true, folders: n, existing, hostKey}`, with
/// `folders` the number of folders at the top and `existing` the share in the library with
/// these settings (then its "Add folders" opens instead of a second share).
pub(crate) async fn sign_in(inner: &Arc<Inner>, spec_json: &str, password: &str) -> CResult<Value> {
    let raw: Value = serde_json::from_str(spec_json).map_err(|e| {
        CoreError::new(
            ErrorKind::Unsupported,
            format!("invalid share settings: {e}"),
        )
    })?;
    let smb_server = raw["protocol"].as_str() == Some("smb")
        && raw["share"].as_str().unwrap_or("").trim().is_empty();
    if smb_server {
        let mut cfg: RemoteConfig = serde_json::from_value(raw).map_err(|e| {
            CoreError::new(
                ErrorKind::Unsupported,
                format!("invalid share settings: {e}"),
            )
        })?;
        cfg.host = cfg.host.trim().to_string();
        cfg.user = cfg.user.trim().to_string();
        if cfg.host.is_empty() {
            return Err(CoreError::new(
                ErrorKind::Unsupported,
                "Please enter a server name or address.",
            ));
        }
        cfg.password = password.to_string();
        let names = tokio::time::timeout(SIGN_IN_LIMIT, huehuecoyotl_vfs::smb_shares(&cfg))
            .await
            .map_err(|_| {
                CoreError::new(ErrorKind::Unreachable, "The server did not answer in time.")
            })??;
        let configs: Vec<(String, String)> = names
            .into_iter()
            .map(|n| {
                let mut c = cfg.clone();
                c.share = n.clone();
                (n, serde_json::to_string(&c).unwrap_or_default())
            })
            .collect();
        let list = inner
            .lib
            .read(move |c| {
                configs
                    .into_iter()
                    .map(|(n, j)| Ok(json!({ "name": n, "existing": shares::find(c, "smb", &j)? })))
                    .collect::<huehuecoyotl_library::Result<Vec<Value>>>()
            })
            .await?;
        return Ok(json!({ "shares": list }));
    }
    let spec = crate::network::NetworkSpec::parse(spec_json)?;
    let src = tokio::time::timeout(SIGN_IN_LIMIT, inner.probe(&spec, password))
        .await
        .map_err(|_| {
            CoreError::new(ErrorKind::Unreachable, "The server did not answer in time.")
        })??;
    let top = match src.list(&VPath::new("/")).await {
        Ok(l) => l,
        Err(e) => {
            inner.probe.lock().unwrap().take();
            return Err(e.into());
        }
    };
    let folders = top
        .iter()
        .filter(|e| e.is_dir && !e.name.starts_with('.'))
        .count();
    let (kind, json) = (spec.cfg.protocol.as_str().to_string(), spec.config_json());
    let existing = inner
        .lib
        .read(move |c| shares::find(c, &kind, &json))
        .await?;
    Ok(json!({
        "connected": true,
        "folders": folders,
        "existing": existing,
        "hostKey": src.host_key().unwrap_or_default(),
    }))
}

/// Step 3, adding the first folders of a new share: the share is created (or found), its
/// password kept, and the folders added. When the folders fail, a share created here goes again
/// with its password, so leaving or failing keeps nothing.
pub(crate) async fn create(
    inner: &Arc<Inner>,
    spec_json: &str,
    password: &str,
    folders: Vec<(String, String, String)>,
) -> CResult<i64> {
    if folders.is_empty() {
        return Err(CoreError::new(
            ErrorKind::Unsupported,
            "Choose at least one folder.",
        ));
    }
    let spec = crate::network::NetworkSpec::parse(spec_json)?;
    let src = inner.probe(&spec, password).await?;
    let mut cfg = spec.cfg.clone();
    if cfg.host_key.is_none() {
        cfg.host_key = src.host_key();
    }
    let (kind, json) = (
        cfg.protocol.as_str().to_string(),
        serde_json::to_string(&cfg).unwrap_or_default(),
    );
    let (share, created) = inner
        .lib
        .write(move |c| shares::find_or_create(c, &kind, &json))
        .await?;
    if created {
        let (reference, password) = (shares::credential_ref(share), password.to_string());
        let stored =
            tokio::task::spawn_blocking(move || secrets::store_password(&reference, &password))
                .await
                .unwrap_or(Ok(()));
        if let Err(w) = stored {
            tracing::warn!("{w}");
        }
    }
    match add_folders(inner, share, folders, Some(src)).await {
        Ok(_) => {
            inner.probe.lock().unwrap().take();
            Ok(share)
        }
        Err(e) => {
            if created {
                remove_share(inner, share).await.ok();
            }
            Err(e)
        }
    }
}

// ---- editing a share (issue #122) --------------------------------------------------------------

/// Test, and with `save` store, a share's new connection settings (`spec_json` as for adding;
/// `password` "" keeps the stored one). Every folder must be found at the new place, else
/// nothing is saved and `missing` names them. A new password is stored only when it works. When
/// the new settings are those of another share (one server reached under two names), saving
/// joins that share's folders into this one; a folder both have stays once. Answers `{ok,
/// missing, joins: {id, name} | null, saved, hostKey}`.
pub(crate) async fn edit(
    inner: &Arc<Inner>,
    share: i64,
    spec_json: &str,
    password: &str,
    save: bool,
) -> CResult<Value> {
    let spec = crate::network::NetworkSpec::parse(spec_json)?;
    let found = inner
        .lib
        .read(move |c| shares::get(c, share))
        .await?
        .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("share {share}")))?;
    let old: Option<RemoteConfig> = serde_json::from_str(&found.config_json).ok();
    let mut cfg = spec.cfg.clone();
    // An SFTP server keeps its trusted key while its address stays; a new address is a new key.
    cfg.host_key = old.as_ref().and_then(|o| {
        (o.protocol == cfg.protocol && o.host.eq_ignore_ascii_case(&cfg.host) && o.port == cfg.port)
            .then(|| o.host_key.clone())
            .flatten()
    });
    let new_password = !password.is_empty();
    cfg.password = if new_password {
        password.to_string()
    } else {
        let reference = shares::credential_ref(share);
        tokio::task::spawn_blocking(move || secrets::load_password(&reference))
            .await
            .unwrap_or_default()
    };
    let src = tokio::time::timeout(SIGN_IN_LIMIT, crate::network::connect_config(cfg.clone()))
        .await
        .map_err(|_| {
            CoreError::new(ErrorKind::Unreachable, "The server did not answer in time.")
        })??;
    if cfg.host_key.is_none() {
        cfg.host_key = src.host_key();
    }
    let mut missing = Vec::new();
    for f in &found.folders {
        let at = VPath::new(if f.root.is_empty() { "/" } else { &f.root });
        if !src.stat(&at).await.map(|e| e.is_dir).unwrap_or(false) {
            missing.push(f.name.clone());
        }
    }
    let (kind, json) = (
        cfg.protocol.as_str().to_string(),
        serde_json::to_string(&cfg).unwrap_or_default(),
    );
    let (k, j) = (kind.clone(), json.clone());
    let other = inner
        .lib
        .read(move |c| {
            let key = shares::group_key(&k, &j);
            Ok(shares::list(c)?
                .into_iter()
                .find(|s| s.id != share && shares::group_key(&s.kind, &s.config_json) == key))
        })
        .await?;
    let joins = other
        .as_ref()
        .map(|o| json!({ "id": o.id, "name": o.name }));
    let host_key = src.host_key().unwrap_or_default();
    if !missing.is_empty() || !save {
        return Ok(json!({
            "ok": missing.is_empty(), "missing": missing, "joins": joins, "saved": false,
            "hostKey": host_key,
        }));
    }

    // Saving: the settings, then the other share's folders, then the password.
    inner
        .lib
        .write(move |c| shares::update(c, share, &kind, &json))
        .await?;
    let mut folder_ids: Vec<i64> = found.folders.iter().map(|f| f.source_id).collect();
    if let Some(o) = other {
        let from = o.id;
        let dups = inner
            .lib
            .read(move |c| shares::duplicates(c, share, from))
            .await?;
        for id in dups {
            crate::download::delete_saved(inner, Some(id)).await?;
            inner
                .lib
                .write(move |c| catalog::remove_library(c, id))
                .await?;
            inner.forget_source(id);
        }
        let (moved, reference) = inner
            .lib
            .write(move |c| shares::join(c, share, from))
            .await?;
        folder_ids.extend(moved);
        if let Some(r) = reference {
            tokio::task::spawn_blocking(move || secrets::delete_password(&r))
                .await
                .ok();
        }
    }
    if new_password {
        let (reference, password) = (shares::credential_ref(share), password.to_string());
        let stored =
            tokio::task::spawn_blocking(move || secrets::store_password(&reference, &password))
                .await
                .unwrap_or(Ok(()));
        if let Err(w) = stored {
            tracing::warn!("{w}");
        }
    }
    for id in folder_ids {
        inner.forget_source(id);
        inner.remotes.lock().unwrap().insert(id, src.clone());
        inner.set_online(id, true);
    }
    inner.mark_changed();
    Ok(json!({ "ok": true, "missing": [], "joins": joins, "saved": true, "hostKey": host_key }))
}

/// Issue #122: what a folder of a share holds, changed: the folder is added again with the new
/// type (its items, play state and saved copies of the old type go) and scanned.
pub(crate) async fn set_folder_content(
    inner: &Arc<Inner>,
    source: i64,
    content: &str,
) -> CResult<()> {
    let content = crate::library::content_type(content).to_string();
    let row = inner
        .lib
        .read(move |c| {
            Ok(c.query_row(
                "SELECT share_id, root, content, name FROM source WHERE id = ?1",
                [source],
                |r| {
                    Ok((
                        r.get::<_, Option<i64>>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )
            .ok())
        })
        .await?;
    let Some((Some(share), root, old, name)) = row else {
        return Err(CoreError::new(
            ErrorKind::NotFound,
            format!("folder {source} of a share"),
        ));
    };
    if old == content {
        return Ok(());
    }
    crate::download::delete_saved(inner, Some(source)).await?;
    inner
        .lib
        .write(move |c| catalog::remove_library(c, source))
        .await?;
    inner.forget_source(source);
    let (id, _) = inner
        .lib
        .write(move |c| shares::add_folder(c, share, &root, &content, &name))
        .await?;
    enqueue_scan(inner, id, jobs::priority::USER).await;
    inner.mark_changed();
    Ok(())
}

impl Core {
    /// Issue #122: test (`save` false) or save a share's new connection settings; answers with a
    /// `MusicDetail` of kind "shareEdit" (see `shares::edit`).
    pub fn share_edit(
        &self,
        request_id: u64,
        share: i64,
        spec_json: &str,
        password: &str,
        save: bool,
    ) {
        let (spec_json, password) = (spec_json.to_string(), password.to_string());
        self.spawn(request_id, move |inner| async move {
            let out = edit(&inner, share, &spec_json, &password, save).await?;
            Ok(Event::MusicDetail {
                request_id,
                kind: "shareEdit".into(),
                json: out.to_string(),
            })
        });
    }

    /// Issue #122: change what a folder of a share holds; answers with the shares.
    pub fn share_folder_content(&self, request_id: u64, source: i64, content: &str) {
        let content = content.to_string();
        self.spawn(request_id, move |inner| async move {
            set_folder_content(&inner, source, &content).await?;
            Ok(overview_event(request_id, overview(&inner).await?))
        });
    }
}

impl Core {
    /// Issue #121: sign in to a server or share being added (`MusicDetail` "shareSignIn"; see
    /// `shares::sign_in`).
    pub fn share_sign_in(&self, request_id: u64, spec_json: &str, password: &str) {
        let (spec_json, password) = (spec_json.to_string(), password.to_string());
        self.spawn(request_id, move |inner| async move {
            let out = sign_in(&inner, &spec_json, &password).await?;
            Ok(Event::MusicDetail {
                request_id,
                kind: "shareSignIn".into(),
                json: out.to_string(),
            })
        });
    }

    /// Issue #121: add a new share with its first folders (`[{path, content, name}]`); answers
    /// with the shares, like `share_add_folders`.
    pub fn share_create(
        &self,
        request_id: u64,
        spec_json: &str,
        password: &str,
        folders_json: &str,
    ) {
        let (spec_json, password) = (spec_json.to_string(), password.to_string());
        let folders = parse_folders(folders_json);
        self.spawn(request_id, move |inner| async move {
            create(&inner, &spec_json, &password, folders?).await?;
            Ok(overview_event(request_id, overview(&inner).await?))
        });
    }
}

impl Core {
    /// Issue #120: browse a share's folders with a quick look into each. `target_json`:
    /// `{"share": id}` for a share in the library, else the settings of a new one (as for
    /// `browse_network`, with `password`). Answers with a `MusicDetail` of kind "shareFolders".
    pub fn share_browse(&self, request_id: u64, target_json: &str, password: &str, path: &str) {
        let (target_json, password, path) = (
            target_json.to_string(),
            password.to_string(),
            path.to_string(),
        );
        self.spawn(request_id, move |inner| async move {
            let v: Value = serde_json::from_str(&target_json).unwrap_or_default();
            let target = match v["share"].as_i64() {
                Some(id) => Target::Share(id),
                None => Target::New(
                    Box::new(crate::network::NetworkSpec::parse(&target_json)?),
                    password,
                ),
            };
            let out = browse(&inner, target, &path).await?;
            Ok(Event::MusicDetail {
                request_id,
                kind: "shareFolders".into(),
                json: out.to_string(),
            })
        });
    }
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
            add_folders(&inner, share, folders, None).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn look(videos: u32, audio: u32, books: u32) -> Option<Look> {
        Some(Look {
            videos,
            audio,
            books,
            folders: 0,
        })
    }

    #[test]
    fn guesses_by_name_then_by_files() {
        assert_eq!(guess("Filme", None), Some("video"));
        assert_eq!(guess("TV Shows", None), Some("video"));
        assert_eq!(
            guess("Serien", look(0, 30, 0)),
            Some("video"),
            "the name wins"
        );
        assert_eq!(guess("Musik", None), Some("music"));
        assert_eq!(guess("Hörbücher", None), Some("audiobooks"));
        assert_eq!(guess("Music Videos", None), Some("video"));
        assert_eq!(guess("Concerts", look(12, 0, 0)), Some("video"));
        assert_eq!(guess("Concerts", look(0, 40, 0)), Some("music"));
        assert_eq!(guess("Stuff", look(0, 0, 3)), Some("audiobooks"));
        assert_eq!(guess("Stuff", look(1, 200, 0)), Some("music"));
        assert_eq!(guess("Stuff", look(20, 3, 0)), Some("video"));
        assert_eq!(guess("Backups", look(0, 0, 0)), None);
        assert_eq!(guess("Backups", None), None);
    }

    #[test]
    fn counts_files_by_kind() {
        let e = |name: &str, is_dir: bool| huehuecoyotl_vfs::Entry {
            path: VPath::new(format!("/x/{name}")),
            name: name.into(),
            is_dir,
            size: 0,
            mtime: 0,
        };
        let mut l = Look::default();
        count(
            &mut l,
            &[
                e("a.mkv", false),
                e("b.flac", false),
                e("c.M4B", false),
                e("d", true),
                e("e.txt", false),
            ],
        );
        assert_eq!(
            l,
            Look {
                videos: 1,
                audio: 1,
                books: 1,
                folders: 0
            }
        );
    }
}
