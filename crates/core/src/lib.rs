//! The headless core (SPEC 2, principle 2). Every feature except video output is reachable
//! from here, by the Qt shell through the `cxx` bridge and by the CLI harness directly.
//!
//! Contract (SPEC 8): no call blocks; each request carries an id and its answer arrives as an
//! [`Event`] on the sink; change events carry ids, not payloads; every request can be
//! cancelled; errors carry an [`ErrorKind`]; panics never escape a task.

pub mod lang;

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use huehuecoyotl_library::{DbError, Library, queries};
use huehuecoyotl_vfs::{
    LocalSource, MediaKind, Source, VPath, VfsError, find_sidecar_subtitles, media_kind,
};
use tokio::runtime::Runtime;
use tokio::task::AbortHandle;

pub use queries::TrackPref;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Auth,
    Unreachable,
    NotFound,
    Unsupported,
    Internal,
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
    Error {
        request_id: u64,
        kind: ErrorKind,
        message: String,
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
}

pub struct Core {
    rt: Runtime,
    inner: Arc<Inner>,
    next_ticket: AtomicU64,
}

/// Log to `<data_dir>/logs/coyotl.log` (and stderr when `COYOTL_LOG_STDERR` is set).
/// Safe to call more than once; only the first call installs the subscriber.
pub fn init_logging(data_dir: &Path) {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let dir = data_dir.join("logs");
        let _ = std::fs::create_dir_all(&dir);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("coyotl.log"));
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
    pub fn open(data_dir: &Path, sink: impl EventSink) -> CResult<Self> {
        init_logging(data_dir);
        let lib = Library::open(&data_dir.join("library.db"))?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("coyotl-core")
            .enable_all()
            .build()
            .map_err(|e| CoreError::new(ErrorKind::Internal, e.to_string()))?;
        tracing::info!(version = env!("CARGO_PKG_VERSION"), data_dir = %data_dir.display(), "core opened");
        Ok(Self {
            rt,
            inner: Arc::new(Inner {
                lib,
                local: LocalSource,
                sink: Box::new(sink),
                tasks: Mutex::default(),
                tickets: Mutex::default(),
            }),
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

    /// Prepare a known item (e.g. from "continue watching") for playback.
    pub fn open_item(&self, request_id: u64, item_id: i64) {
        let ticket_id = self.next_ticket.fetch_add(1, Ordering::Relaxed);
        self.spawn(request_id, move |inner| async move {
            let path: Option<String> = inner
                .lib
                .read(move |c| {
                    use huehuecoyotl_library::rusqlite::OptionalExtension;
                    Ok(c.query_row("SELECT path FROM file WHERE item_id = ?1 AND source_id = 1 ORDER BY id LIMIT 1", [item_id], |r| r.get(0)).optional()?)
                })
                .await?;
            let path = path.ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("item {item_id} has no file")))?;
            let ticket = prepare_local(&inner, &path, ticket_id).await?;
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
        // Queued from here, in call order, onto the single writer.
        let fut = self
            .inner
            .lib
            .submit(move |c| queries::save_progress(c, item_id, position_ms, duration_ms, ended));
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

    pub fn set_watched(&self, item_id: i64, watched: bool) {
        let inner = self.inner.clone();
        let fut = inner
            .lib
            .submit(move |c| queries::set_watched(c, item_id, watched));
        self.rt.spawn(async move {
            match fut.await {
                Ok(()) => inner.sink.post(Event::ItemChanged { item_id }),
                Err(e) => tracing::error!(item_id, "set_watched failed: {e}"),
            }
        });
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        tracing::info!("core shutting down");
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
    let item_id = inner
        .lib
        .write(move |c| queries::ensure_local_video(c, &p, size, mtime))
        .await?;
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
    Ok(Ticket {
        ticket: ticket_id,
        item_id,
        title: title.unwrap_or_else(|| entry.name.clone()),
        url: vp.to_string(),
        is_network: false,
        resume_ms: if state.watched { 0 } else { state.position_ms },
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
        let d = tempfile::tempdir().unwrap();
        let (tx, rx) = unbounded();
        let c = Core::open(&d.path().join("data"), move |e| {
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
