//! The headless core (SPEC 2, principle 2). Every feature except video output is reachable
//! from here, by the Qt shell through the `cxx` bridge and by the CLI harness directly.
//!
//! Contract (SPEC 8): no call blocks; each request carries an id and its answer arrives as an
//! [`Event`] on the sink; change events carry ids, not payloads; every request can be
//! cancelled; errors carry an [`ErrorKind`]; panics never escape a task.

pub mod diagnostics;
mod download;
pub mod lang;
mod library;
mod network;
mod probe;
pub mod secrets;
mod worker;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use huehuecoyotl_library::{DbError, Library, catalog, queries};
use huehuecoyotl_metadata::MetadataProvider;
use huehuecoyotl_vfs::{
    LocalSource, MediaKind, RemoteSource, Source, VPath, VfsError, find_sidecar_subtitles,
    media_kind,
};
use tokio::runtime::Runtime;
use tokio::sync::Notify;
use tokio::task::AbortHandle;

pub use download::{DownloadInfo, available_space, is_portal_path};
pub use huehuecoyotl_library::catalog::{CandidateRow, Detail, EpisodeRow, FileInfo, Summary};
pub use huehuecoyotl_library::copies::SaveInfo;
pub use network::{Discovered, NetworkSpec};
pub use queries::TrackPref;
use secrets::KeySource;
pub use worker::find_match;

#[derive(Clone, Debug, PartialEq)]
pub struct LibraryInfo {
    pub id: i64,
    /// `local`, `smb`, `nfs`, `webdav`, `ftp`, `sftp`.
    pub kind: String,
    pub name: String,
    /// Local folder, or a display URL without credentials (`smb://nas/media/Movies`).
    pub root: String,
    /// Network sources: false while the server cannot be reached.
    pub online: bool,
    pub files: i64,
    pub last_scan_ms: i64,
    pub scanning: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewItem {
    pub item: Summary,
    pub candidates: Vec<CandidateRow>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Auth,
    Unreachable,
    NotFound,
    Unsupported,
    Internal,
    /// Not enough free space on the drive a copy is written to (issue #3).
    NoSpace,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct CoreError {
    pub kind: ErrorKind,
    pub message: String,
}

impl CoreError {
    fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<VfsError> for CoreError {
    fn from(e: VfsError) -> Self {
        let kind = match e {
            VfsError::Auth(_) => ErrorKind::Auth,
            VfsError::Unreachable(_) => ErrorKind::Unreachable,
            VfsError::NotFound(_) => ErrorKind::NotFound,
            VfsError::Unsupported(_) => ErrorKind::Unsupported,
            VfsError::Internal(_) => ErrorKind::Internal,
        };
        Self::new(kind, e.to_string())
    }
}

impl From<DbError> for CoreError {
    fn from(e: DbError) -> Self {
        Self::new(ErrorKind::Internal, e.to_string())
    }
}

type CResult<T> = Result<T, CoreError>;

/// Player states (SPEC 5.5). The numeric values cross the bridge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PlayerState {
    Idle = 0,
    Opening = 1,
    Buffering = 2,
    Playing = 3,
    Paused = 4,
    Ended = 5,
    Error = 6,
}

impl PlayerState {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Opening,
            2 => Self::Buffering,
            3 => Self::Playing,
            4 => Self::Paused,
            5 => Self::Ended,
            6 => Self::Error,
            _ => Self::Idle,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_video: bool,
    pub size: u64,
    pub progress: f32,
    pub watched: bool,
}

/// A sidecar subtitle found next to the video.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Subtitle {
    pub url: String,
    /// Normalised language code (639-2/B), empty if the file name has none.
    pub lang: String,
    pub forced: bool,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Ticket {
    pub ticket: u64,
    pub item_id: i64,
    pub title: String,
    /// What mpv opens: a local path for local files, a `coyotl://` URL for network sources.
    pub url: String,
    pub is_network: bool,
    pub resume_ms: u64,
    pub subtitles: Vec<Subtitle>,
    pub prefs: TrackPref,
    /// Every spelling of the preferred languages, ready for mpv's alang/slang.
    pub audio_langs: Vec<String>,
    pub sub_langs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecentItem {
    pub item_id: i64,
    pub title: String,
    pub path: String,
    pub position_ms: u64,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Folder {
        request_id: u64,
        path: String,
        parent: String,
        entries: Vec<FsEntry>,
    },
    Playback {
        request_id: u64,
        ticket: Ticket,
    },
    ContinueWatching {
        request_id: u64,
        items: Vec<RecentItem>,
    },
    /// Something about this item changed (play state, metadata). Re-query what you show.
    ItemChanged {
        item_id: i64,
    },
    /// One page of items (browse, search, recently added).
    Page {
        request_id: u64,
        total: u32,
        offset: u32,
        items: Vec<Summary>,
    },
    /// Issue #48: a page of a season's episodes (`Core::season_episodes`).
    Episodes {
        request_id: u64,
        total: u32,
        offset: u32,
        items: Vec<EpisodeRow>,
    },
    Detail {
        request_id: u64,
        detail: Box<Detail>,
    },
    Libraries {
        request_id: u64,
        libraries: Vec<LibraryInfo>,
    },
    Review {
        request_id: u64,
        items: Vec<ReviewItem>,
    },
    Candidates {
        request_id: u64,
        item_id: i64,
        candidates: Vec<CandidateRow>,
    },
    ScanState {
        source_id: i64,
        running: bool,
        message: String,
    },
    /// The library changed in bulk (scan or metadata). Grids re-query. Debounced.
    LibraryChanged {
        queued: i64,
        failed: i64,
    },
    /// File servers found on the local network.
    Discovered {
        request_id: u64,
        devices: Vec<Discovered>,
    },
    KeyStatus {
        request_id: u64,
        has_key: bool,
        from_env: bool,
        message: String,
    },
    Error {
        request_id: u64,
        kind: ErrorKind,
        message: String,
    },
    /// The state of an item's saved copies (issue #4).
    SaveState {
        request_id: u64,
        item_id: i64,
        info: huehuecoyotl_library::copies::SaveInfo,
    },
    /// "Save as…" (issue #5) would overwrite these files; nothing was started.
    ExportConflict {
        request_id: u64,
        paths: Vec<String>,
    },
    /// Saved copies and exports (issue #3), with the space saved copies use.
    Downloads {
        request_id: u64,
        items: Vec<DownloadInfo>,
        used_bytes: u64,
        storage_dir: String,
    },
    /// One copied file moved on (at most twice a second while it runs), changed state
    /// (queued, running, paused, done, failed, outdated) or was removed (`removed`).
    /// `main_id` is the row the Downloads screen shows; subtitles and parts count towards it.
    DownloadProgress {
        id: i64,
        main_id: i64,
        item_id: i64,
        /// saved | export (issue #48: tiles show only saved copies' progress).
        kind: String,
        state: String,
        bytes_done: u64,
        size: u64,
        error_kind: String,
        error: String,
    },
}

pub trait EventSink: Send + Sync + 'static {
    fn post(&self, event: Event);
}

impl<F: Fn(Event) + Send + Sync + 'static> EventSink for F {
    fn post(&self, event: Event) {
        self(event)
    }
}

struct Inner {
    lib: Library,
    local: LocalSource,
    sink: Box<dyn EventSink>,
    tasks: Mutex<HashMap<u64, AbortHandle>>,
    tickets: Mutex<HashMap<u64, i64>>,
    /// The library file a ticket plays (issue #33: what mpv reports during playback fills in
    /// the file's technical details when the probe could not).
    ticket_files: Mutex<HashMap<u64, i64>>,
    data_dir: PathBuf,
    /// BCP-47 UI language for metadata ("de-DE").
    language: String,
    provider: RwLock<Option<Arc<dyn MetadataProvider>>>,
    key_source: Mutex<Option<KeySource>>,
    /// Wakes the job workers: new work, a new key.
    wake: Notify,
    changed: AtomicBool,
    scanning: Mutex<HashSet<i64>>,
    /// Live connections of network sources, by source id.
    remotes: Mutex<HashMap<i64, Arc<dyn RemoteSource>>>,
    offline: Mutex<HashSet<i64>>,
    /// The "add network share" dialog's connection: (settings key, source).
    probe: Mutex<Option<(String, Arc<dyn RemoteSource>)>>,
    /// Read priorities per source (issue #3): playback before downloads and probes.
    gates: Mutex<HashMap<i64, Arc<download::Gate>>>,
    /// Pause and cancel of the copies that are running, by `local_copy` id.
    downloads: Mutex<HashMap<i64, Arc<download::Ctl>>>,
}

impl Inner {
    fn provider(&self) -> Option<Arc<dyn MetadataProvider>> {
        self.provider.read().unwrap().clone()
    }

    fn set_provider(
        self: &Arc<Self>,
        p: Option<Arc<dyn MetadataProvider>>,
        src: Option<KeySource>,
    ) {
        let has = p.is_some();
        *self.provider.write().unwrap() = p;
        *self.key_source.lock().unwrap() = src;
        if has {
            let me = self.clone();
            tokio::spawn(async move {
                let _ = me
                    .lib
                    .write(|c| huehuecoyotl_library::jobs::wake(c, worker::META_KINDS))
                    .await;
                me.wake.notify_waiters();
            });
        }
    }

    /// TMDB said 401: drop the key so the UI asks for a new one.
    fn key_rejected(self: &Arc<Self>) {
        tracing::warn!("TMDB rejected the API key");
        *self.provider.write().unwrap() = None;
        self.sink
            .post(self.key_status_event(0, "TMDB rejected the stored API key.".into()));
    }

    fn key_status_event(&self, request_id: u64, message: String) -> Event {
        let src = *self.key_source.lock().unwrap();
        Event::KeyStatus {
            request_id,
            has_key: self.provider().is_some(),
            from_env: src == Some(KeySource::Environment),
            message,
        }
    }

    /// Post a debounced LibraryChanged so grids re-query at most a few times per second.
    fn mark_changed(self: &Arc<Self>) {
        if self.changed.swap(true, Ordering::AcqRel) {
            return;
        }
        let me = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            me.changed.store(false, Ordering::Release);
            // Without a metadata key only scans can run; do not report waiting metadata work.
            let st = if me.provider().is_some() {
                me.lib.read(huehuecoyotl_library::jobs::stats).await
            } else {
                me.lib
                    .read(|c| huehuecoyotl_library::jobs::stats_for(c, worker::SCAN_KINDS))
                    .await
            }
            .unwrap_or_default();
            me.sink.post(Event::LibraryChanged {
                queued: st.queued + st.running,
                failed: st.failed,
            });
        });
    }
}

pub struct Core {
    rt: Runtime,
    inner: Arc<Inner>,
    next_ticket: AtomicU64,
}

/// Log to `<data_dir>/logs/coyotl.log`, rotated at 5 MiB with three old files kept (and to
/// stderr instead when `COYOTL_LOG_STDERR` is set). Safe to call more than once; only the first
/// call installs the subscriber.
pub fn init_logging(data_dir: &Path) {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let file = diagnostics::RotatingFile::open(&data_dir.join("logs"));
        let filter = tracing_subscriber::EnvFilter::try_from_env("COYOTL_LOG")
            .unwrap_or_else(|_| "info".into());
        let builder = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false);
        let _ = match (file, std::env::var_os("COYOTL_LOG_STDERR")) {
            (Ok(f), None) => builder.with_writer(Mutex::new(f)).try_init(),
            _ => builder.with_writer(std::io::stderr).try_init(),
        };
    });
}

impl Core {
    /// Open the core. `language` is the UI language as a BCP-47 tag ("de-DE"); metadata is
    /// requested in it with an English fallback.
    pub fn open(data_dir: &Path, language: &str, sink: impl EventSink) -> CResult<Self> {
        init_logging(data_dir);
        let lib = Library::open(&data_dir.join("library.db"))?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("coyotl-core")
            .enable_all()
            .build()
            .map_err(|e| CoreError::new(ErrorKind::Internal, e.to_string()))?;
        tracing::info!(version = env!("CARGO_PKG_VERSION"), data_dir = %data_dir.display(), "core opened");
        let inner = Arc::new(Inner {
            lib,
            local: LocalSource,
            sink: Box::new(sink),
            tasks: Mutex::default(),
            tickets: Mutex::default(),
            ticket_files: Mutex::default(),
            data_dir: data_dir.to_path_buf(),
            language: if language.is_empty() {
                "en-US".into()
            } else {
                language.to_string()
            },
            provider: RwLock::new(None),
            key_source: Mutex::new(None),
            wake: Notify::new(),
            changed: AtomicBool::new(false),
            scanning: Mutex::default(),
            remotes: Mutex::default(),
            offline: Mutex::default(),
            probe: Mutex::default(),
            gates: Mutex::default(),
            downloads: Mutex::default(),
        });
        // mpv's demuxer threads open coyotl:// URLs through this (SPEC 5.3).
        let weak = Arc::downgrade(&inner);
        huehuecoyotl_stream::set_resolver(Arc::new(move |id| {
            let inner = weak
                .upgrade()
                .ok_or_else(|| VfsError::Internal("core closed".into()))?;
            huehuecoyotl_stream::block_on(inner.remote_source(id))
                .map(|s| {
                    Arc::new(download::Prioritised::playback(s, inner.gate(id))) as Arc<dyn Source>
                })
                .map_err(|e| match e.kind {
                    ErrorKind::Auth => VfsError::Auth(e.message),
                    ErrorKind::Unreachable => VfsError::Unreachable(e.message),
                    ErrorKind::NotFound => VfsError::NotFound(e.message),
                    ErrorKind::Unsupported => VfsError::Unsupported(e.message),
                    ErrorKind::Internal | ErrorKind::NoSpace => VfsError::Internal(e.message),
                })
        }));
        // Offline copies: clean up after the last session (exports are forgotten) and queue
        // unfinished ones, before the first request can list them.
        rt.block_on(download::startup(&inner));
        let start = inner.clone();
        rt.spawn(async move {
            // Crash-safe queue: running jobs from a previous session run again.
            let _ = start
                .lib
                .write(|c| huehuecoyotl_library::jobs::recover(c))
                .await;
            // Technical details of files not analysed yet (low priority, its own worker).
            probe::enqueue_unprobed(&start).await;
            // Larger grid thumbnails for libraries from before they existed (offline).
            worker::enqueue_tiers(
                &start,
                huehuecoyotl_library::jobs::priority::BACKGROUND,
                "tiers",
            )
            .await;
            // Catch up with changes made while the app was closed.
            if let Ok(libs) = start
                .lib
                .read(huehuecoyotl_library::catalog::libraries)
                .await
            {
                for l in libs {
                    library::enqueue_scan(
                        &start,
                        l.id,
                        huehuecoyotl_library::jobs::priority::BACKGROUND,
                    )
                    .await;
                }
            }
            if let Ok(Some((key, src))) = tokio::task::spawn_blocking(secrets::load_tmdb_key).await
            {
                match huehuecoyotl_metadata::tmdb::Tmdb::new(key, start.language.clone()) {
                    // A provider set meanwhile (tests, the CLI) wins over the keystore.
                    Ok(t) if start.provider().is_none() => {
                        start.set_provider(Some(Arc::new(t)), Some(src))
                    }
                    Ok(_) => {}
                    Err(e) => tracing::error!("TMDB client: {e}"),
                }
            }
            worker::spawn_workers(&start);
            download::spawn_worker(&start);
            network::spawn_reconnect(&start);
        });
        Ok(Self {
            rt,
            inner,
            next_ticket: AtomicU64::new(1),
        })
    }

    /// Spawn a request. Its result (or error, including a panic) is posted to the sink,
    /// unless it was cancelled.
    fn spawn<F, Fut>(&self, request_id: u64, f: F)
    where
        F: FnOnce(Arc<Inner>) -> Fut,
        Fut: std::future::Future<Output = CResult<Event>> + Send + 'static,
    {
        let inner = self.inner.clone();
        let work = self.rt.spawn(f(inner.clone()));
        inner
            .tasks
            .lock()
            .unwrap()
            .insert(request_id, work.abort_handle());
        self.rt.spawn(async move {
            let res = work.await;
            inner.tasks.lock().unwrap().remove(&request_id);
            let ev = match res {
                Ok(Ok(ev)) => ev,
                Ok(Err(e)) => Event::Error {
                    request_id,
                    kind: e.kind,
                    message: e.message,
                },
                Err(j) if j.is_cancelled() => return,
                Err(j) => {
                    tracing::error!(request_id, "request panicked: {j}");
                    Event::Error {
                        request_id,
                        kind: ErrorKind::Internal,
                        message: "internal error".into(),
                    }
                }
            };
            inner.sink.post(ev);
        });
    }

    pub fn cancel(&self, request_id: u64) {
        if let Some(h) = self.inner.tasks.lock().unwrap().remove(&request_id) {
            h.abort();
        }
    }

    /// List a folder; an empty path lists the starting places (home, drives, mounts).
    pub fn browse(&self, request_id: u64, path: &str) {
        let path = path.to_string();
        self.spawn(request_id, move |inner| async move {
            if path.is_empty() {
                let entries = LocalSource::places()
                    .into_iter()
                    .map(|e| FsEntry {
                        name: e.name,
                        path: e.path.to_string(),
                        is_dir: true,
                        is_video: false,
                        size: 0,
                        progress: 0.0,
                        watched: false,
                    })
                    .collect();
                return Ok(Event::Folder {
                    request_id,
                    path,
                    parent: String::new(),
                    entries,
                });
            }
            let dir = VPath::new(&path);
            let listing = inner.local.list(&dir).await?;
            let dir_str = dir.to_string();
            let states = inner
                .lib
                .read(move |c| queries::local_states_in_dir(c, &dir_str))
                .await?;
            let entries = listing
                .into_iter()
                .filter(|e| e.is_dir || media_kind(&e.name) == MediaKind::Video)
                .map(|e| {
                    let st = states.get(e.path.as_str());
                    FsEntry {
                        progress: st.map_or(0.0, |s| s.progress()),
                        watched: st.is_some_and(|s| s.watched),
                        is_video: !e.is_dir,
                        name: e.name,
                        path: e.path.to_string(),
                        is_dir: e.is_dir,
                        size: e.size,
                    }
                })
                .collect();
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

    /// Prepare a local file for playback: register it, find sidecar subtitles, and return
    /// the resume point and track preferences in a ticket.
    pub fn open_path(&self, request_id: u64, path: &str) {
        let path = path.to_string();
        let ticket_id = self.next_ticket.fetch_add(1, Ordering::Relaxed);
        self.spawn(request_id, move |inner| async move {
            let ticket = prepare_local(&inner, &path, ticket_id).await?;
            Ok(Event::Playback { request_id, ticket })
        });
    }

    /// Prepare a known item for playback. Shows and seasons play their next-up episode.
    /// Issue #34: play one version (file) of a movie or episode.
    pub fn open_item_file(&self, request_id: u64, item_id: i64, file_id: i64) {
        self.open_files(request_id, item_id, Some(file_id));
    }

    pub fn open_item(&self, request_id: u64, item_id: i64) {
        self.open_files(request_id, item_id, None);
    }

    fn open_files(&self, request_id: u64, item_id: i64, only: Option<i64>) {
        let ticket_id = self.next_ticket.fetch_add(1, Ordering::Relaxed);
        self.spawn(request_id, move |inner| async move {
            let files = inner
                .lib
                .read(move |c| {
                    let mut id = item_id;
                    if let Some(d) = huehuecoyotl_library::catalog::detail(c, item_id)? {
                        if let Some(n) = d.next_up {
                            id = n.id;
                        }
                    }
                    let mut files = huehuecoyotl_library::catalog::playable_files(c, id)?;
                    if let Some(f) = only {
                        files.retain(|p| p.file_id == f);
                    }
                    // Issue #4: a complete, current saved copy plays instead of the network file.
                    let mut out = Vec::new();
                    for f in files {
                        let saved = if f.kind == "local" {
                            None
                        } else {
                            huehuecoyotl_library::copies::usable_path(c, f.file_id)?
                        };
                        out.push((f, saved, id));
                    }
                    // Files on this computer first, then saved copies, then network files.
                    out.sort_by_key(|(f, saved, _)| (f.kind != "local", saved.is_none()));
                    Ok(out)
                })
                .await?;
            let mut last =
                CoreError::new(ErrorKind::NotFound, format!("item {item_id} has no file"));
            let mut ticket = None;
            for (f, saved, played) in files {
                // A source marked offline is tried anyway: the user asked, and it may be back.
                let r = if let Some(path) = saved {
                    match prepare_copy(&inner, &path, played, f.file_id, ticket_id).await {
                        Ok(t) => Ok(t),
                        // The copy went missing: fall back to the network file.
                        Err(_) => {
                            network::prepare_remote(&inner, f.source_id, &f.path, ticket_id).await
                        }
                    }
                } else if f.kind == "local" {
                    prepare_local(&inner, &f.path, ticket_id).await
                } else {
                    network::prepare_remote(&inner, f.source_id, &f.path, ticket_id).await
                };
                match r {
                    Ok(t) => {
                        ticket = Some(t);
                        break;
                    }
                    Err(e) => last = e,
                }
            }
            let ticket = ticket.ok_or(last)?;
            Ok(Event::Playback { request_id, ticket })
        });
    }

    /// Record playback progress for a ticket. Fire-and-forget; writes stay in call order.
    pub fn report_progress(
        &self,
        ticket: u64,
        position_ms: u64,
        duration_ms: u64,
        state: PlayerState,
    ) {
        let Some(item_id) = self.inner.tickets.lock().unwrap().get(&ticket).copied() else {
            tracing::warn!(ticket, "progress for unknown ticket");
            return;
        };
        if duration_ms == 0 && state != PlayerState::Ended {
            return; // nothing meaningful to store yet
        }
        let ended = state == PlayerState::Ended;
        // Issue #34: a known version keeps its own position when it is another cut.
        let file = self
            .inner
            .ticket_files
            .lock()
            .unwrap()
            .get(&ticket)
            .copied();
        // Queued from here, in call order, onto the single writer.
        let fut = self.inner.lib.submit(move |c| match file {
            Some(f) => {
                queries::save_version_progress(c, item_id, f, position_ms, duration_ms, ended)
            }
            None => queries::save_progress(c, item_id, position_ms, duration_ms, ended),
        });
        let sink_inner = self.inner.clone();
        self.rt.spawn(async move {
            match fut.await {
                Ok(_) => sink_inner.sink.post(Event::ItemChanged { item_id }),
                Err(e) => tracing::error!(item_id, "saving progress failed: {e}"),
            }
        });
        if ended {
            self.inner.tickets.lock().unwrap().remove(&ticket);
        }
    }

    /// What mpv reports about the playing file (issue #33 fallback): stored as the file's
    /// technical details when it has none, or only an error or older mpv data.
    pub fn report_media_info(&self, ticket: u64, mpv_json: &str) {
        let Some(file_id) = self
            .inner
            .ticket_files
            .lock()
            .unwrap()
            .get(&ticket)
            .copied()
        else {
            return;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(mpv_json) else {
            return;
        };
        let inner = self.inner.clone();
        self.rt.spawn(async move {
            let r = inner
                .lib
                .write(move |c| {
                    let current = catalog::probe_json(c, file_id)?.and_then(|j| {
                        serde_json::from_str::<huehuecoyotl_probe::ProbeInfo>(&j).ok()
                    });
                    let replace = current.is_none_or(|p| p.error.is_some() || p.source == "mpv");
                    if !replace {
                        return Ok(None);
                    }
                    let info = huehuecoyotl_probe::from_mpv(&v);
                    let json = serde_json::to_string(&info).unwrap_or_default();
                    catalog::set_probe(c, file_id, &json)
                })
                .await;
            match r {
                Ok(Some(item_id)) => inner.sink.post(Event::ItemChanged { item_id }),
                Ok(None) => {}
                Err(e) => tracing::error!(file_id, "storing media info failed: {e}"),
            }
        });
    }

    pub fn set_track_pref(&self, ticket: u64, mut pref: TrackPref) {
        pref.audio_lang = lang::normalize(&pref.audio_lang);
        pref.sub_lang = lang::normalize(&pref.sub_lang);
        let Some(item_id) = self.inner.tickets.lock().unwrap().get(&ticket).copied() else {
            return;
        };
        let fut = self
            .inner
            .lib
            .submit(move |c| queries::set_track_pref(c, item_id, &pref));
        self.rt.spawn(async move {
            if let Err(e) = fut.await {
                tracing::error!(item_id, "saving track preference failed: {e}");
            }
        });
    }

    pub fn continue_watching(&self, request_id: u64, limit: u32) {
        self.spawn(request_id, move |inner| async move {
            let rows = inner
                .lib
                .read(move |c| queries::continue_watching(c, limit))
                .await?;
            let items = rows
                .into_iter()
                .map(|r| RecentItem {
                    item_id: r.item_id,
                    title: r.title,
                    path: r.path,
                    position_ms: r.state.position_ms,
                    duration_ms: r.state.duration_ms,
                })
                .collect();
            Ok(Event::ContinueWatching { request_id, items })
        });
    }

    /// Mark an item watched or unwatched; shows and seasons apply to all their episodes.
    pub fn set_watched(&self, item_id: i64, watched: bool) {
        let inner = self.inner.clone();
        self.rt.spawn(async move {
            match worker::set_watched_tree(&inner, item_id, watched).await {
                Ok(()) => {
                    inner.sink.post(Event::ItemChanged { item_id });
                    inner.mark_changed();
                }
                Err(e) => tracing::error!(item_id, "set_watched failed: {e}"),
            }
        });
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        tracing::info!("core shutting down");
        // Network connections close from tasks on this runtime; drop them while it runs.
        let _rt = self.rt.enter();
        self.inner.remotes.lock().unwrap().clear();
        self.inner.probe.lock().unwrap().take();
    }
}

async fn prepare_local(inner: &Arc<Inner>, path: &str, ticket_id: u64) -> CResult<Ticket> {
    let vp = VPath::new(path);
    let entry = inner.local.stat(&vp).await?;
    if entry.is_dir {
        return Err(CoreError::new(
            ErrorKind::Unsupported,
            format!("{path} is a folder"),
        ));
    }
    let siblings = match vp.parent() {
        Some(dir) => inner.local.list(&dir).await.unwrap_or_default(),
        None => vec![],
    };
    let subtitles = find_sidecar_subtitles(&entry.name, &siblings)
        .into_iter()
        .map(|e| sidecar(&entry.name, &e.name, e.path.to_string()))
        .collect();
    let (p, size, mtime) = (vp.to_string(), entry.size, entry.mtime);
    let p2 = p.clone();
    let in_library = inner
        .lib
        .read(move |c| huehuecoyotl_library::catalog::library_item_for_path(c, &p2))
        .await?;
    let item_id = match in_library {
        Some(id) => id,
        None => {
            inner
                .lib
                .write(move |c| queries::ensure_local_video(c, &p, size, mtime))
                .await?
        }
    };
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
        .read(move |c| huehuecoyotl_library::catalog::file_for_path(c, None, &p3))
        .await
    {
        inner.ticket_files.lock().unwrap().insert(ticket_id, file);
    }
    // Issue #34: a version of another cut resumes from its own position.
    let file = inner.ticket_files.lock().unwrap().get(&ticket_id).copied();
    let resume_ms = match file {
        Some(f) => inner
            .lib
            .read(move |c| {
                let dur = catalog::file_duration_ms(c, f)?;
                queries::resume_for(c, item_id, f, dur)
            })
            .await
            .unwrap_or(if state.watched { 0 } else { state.position_ms }),
        None => {
            if state.watched {
                0
            } else {
                state.position_ms
            }
        }
    };
    Ok(Ticket {
        ticket: ticket_id,
        item_id,
        title: title.unwrap_or_else(|| entry.name.clone()),
        url: vp.to_string(),
        is_network: false,
        resume_ms,
        subtitles,
        audio_langs: lang::variants(&prefs.audio_lang),
        sub_langs: lang::variants(&prefs.sub_lang),
        prefs,
    })
}

/// Prepare a saved copy (issue #4) of a library file: a local file, but the item, its watch
/// state and its version's resume position are the library file's.
async fn prepare_copy(
    inner: &Arc<Inner>,
    path: &str,
    item_id: i64,
    file_id: i64,
    ticket_id: u64,
) -> CResult<Ticket> {
    let vp = VPath::new(path);
    let entry = inner.local.stat(&vp).await?;
    let siblings = match vp.parent() {
        Some(dir) => inner.local.list(&dir).await.unwrap_or_default(),
        None => vec![],
    };
    let subtitles = find_sidecar_subtitles(&entry.name, &siblings)
        .into_iter()
        .map(|e| sidecar(&entry.name, &e.name, e.path.to_string()))
        .collect();
    let (title, state, prefs, resume) = inner
        .lib
        .read(move |c| {
            let dur = catalog::file_duration_ms(c, file_id)?;
            Ok((
                queries::item_title(c, item_id)?,
                queries::playstate(c, item_id)?,
                queries::track_pref(c, item_id)?,
                queries::resume_for(c, item_id, file_id, dur),
            ))
        })
        .await?;
    inner.tickets.lock().unwrap().insert(ticket_id, item_id);
    inner
        .ticket_files
        .lock()
        .unwrap()
        .insert(ticket_id, file_id);
    let fallback = if state.watched { 0 } else { state.position_ms };
    Ok(Ticket {
        ticket: ticket_id,
        item_id,
        title: title.unwrap_or_else(|| entry.name.clone()),
        url: vp.to_string(),
        is_network: false,
        resume_ms: resume.unwrap_or(fallback),
        subtitles,
        audio_langs: lang::variants(&prefs.audio_lang),
        sub_langs: lang::variants(&prefs.sub_lang),
        prefs,
    })
}

/// `Movie.de.forced.srt` next to `Movie.mkv` -> German, forced.
fn sidecar(video: &str, sub: &str, url: String) -> Subtitle {
    let stem = video.rsplit_once('.').map_or(video, |(s, _)| s);
    let middle = sub.get(stem.len()..).unwrap_or("");
    let middle = middle.rsplit_once('.').map_or("", |(m, _)| m);
    let mut out = Subtitle {
        url,
        ..Default::default()
    };
    for tok in middle.split('.').filter(|t| !t.is_empty()) {
        let t = tok.to_ascii_lowercase();
        if t == "forced" {
            out.forced = true;
        } else if out.lang.is_empty() && lang::is_language(&t) {
            out.lang = lang::normalize(&t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::{Receiver, unbounded};
    use std::time::Duration;

    fn core() -> (tempfile::TempDir, Core, Receiver<Event>) {
        // SAFETY: tests set this before opening a core; nothing reads it concurrently.
        unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
        let d = tempfile::tempdir().unwrap();
        let (tx, rx) = unbounded();
        let c = Core::open(&d.path().join("data"), "en-US", move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        (d, c, rx)
    }

    fn next(rx: &Receiver<Event>) -> Event {
        rx.recv_timeout(Duration::from_secs(5)).expect("event")
    }

    #[test]
    fn browse_play_resume_cycle() {
        let (d, core, rx) = core();
        let m = d.path().join("media");
        std::fs::create_dir_all(m.join("Extras")).unwrap();
        std::fs::write(m.join("Movie.mkv"), b"x").unwrap();
        std::fs::write(m.join("Movie.en.srt"), b"x").unwrap();
        std::fs::write(m.join("notes.txt"), b"x").unwrap();
        let mdir = VPath::new(m.to_string_lossy()).to_string();

        core.browse(1, &mdir);
        let Event::Folder {
            request_id: 1,
            entries,
            ..
        } = next(&rx)
        else {
            panic!()
        };
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Extras", "Movie.mkv"]);

        let movie = format!("{mdir}/Movie.mkv");
        core.open_path(2, &movie);
        let Event::Playback {
            request_id: 2,
            ticket,
        } = next(&rx)
        else {
            panic!()
        };
        assert_eq!(ticket.resume_ms, 0);
        assert_eq!(ticket.title, "Movie");
        assert_eq!(
            ticket.subtitles,
            [Subtitle {
                url: format!("{mdir}/Movie.en.srt"),
                lang: "eng".into(),
                forced: false
            }]
        );

        core.report_progress(ticket.ticket, 300_000, 6_000_000, PlayerState::Paused);
        assert!(matches!(next(&rx), Event::ItemChanged { item_id } if item_id == ticket.item_id));
        core.set_track_pref(
            ticket.ticket,
            TrackPref {
                audio_lang: "en".into(),
                sub_lang: "deu".into(),
                sub_off: false,
            },
        );

        core.browse(3, &mdir);
        let Event::Folder { entries, .. } = next(&rx) else {
            panic!()
        };
        assert!((entries[1].progress - 0.05).abs() < 1e-6);

        core.continue_watching(4, 10);
        let Event::ContinueWatching { items, .. } = next(&rx) else {
            panic!()
        };
        assert_eq!(items[0].position_ms, 300_000);

        core.open_item(5, ticket.item_id);
        let Event::Playback { ticket: t2, .. } = next(&rx) else {
            panic!()
        };
        assert_eq!(t2.resume_ms, 300_000);
        assert_eq!(t2.prefs.sub_lang, "ger");
        assert_eq!(t2.audio_langs, ["eng", "en"]);
        assert_eq!(t2.sub_langs, ["ger", "deu", "de"]);
        assert_ne!(t2.ticket, ticket.ticket);

        core.report_progress(t2.ticket, 6_000_000, 6_000_000, PlayerState::Ended);
        next(&rx);
        core.open_path(6, &movie);
        let Event::Playback { ticket: t3, .. } = next(&rx) else {
            panic!()
        };
        assert_eq!(t3.resume_ms, 0, "watched items start from the beginning");
    }

    #[test]
    fn sidecar_names() {
        let s = sidecar("Movie.mkv", "Movie.de.forced.srt", "u".into());
        assert_eq!((s.lang.as_str(), s.forced), ("ger", true));
        let s = sidecar("Movie.mkv", "Movie.srt", "u".into());
        assert_eq!((s.lang.as_str(), s.forced), ("", false));
        let s = sidecar("Movie.mkv", "Movie.Director Commentary.eng.ass", "u".into());
        assert_eq!(s.lang, "eng");
    }

    #[test]
    fn errors_are_typed() {
        let (_d, core, rx) = core();
        core.open_path(7, "/definitely/not/here.mkv");
        assert!(matches!(
            next(&rx),
            Event::Error {
                request_id: 7,
                kind: ErrorKind::NotFound,
                ..
            }
        ));
        core.browse(8, "");
        assert!(matches!(next(&rx), Event::Folder { request_id: 8, .. }));
    }
}
