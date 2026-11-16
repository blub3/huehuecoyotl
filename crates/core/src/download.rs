//! The download engine (issue #3): copies media files from a source to the local machine, for
//! "Save" (an offline copy in managed storage) and "Save as…" (an export to a place the user
//! picks). Every copied file is a `local_copy` row and a `download` job, so a copy survives a
//! restart and continues where it stopped.
//!
//! - Data is read with `Source::read_at`, so every protocol works, and written to
//!   `<name>.part`, which is renamed once the size and the fingerprint (SPEC 7) match.
//! - A dropped connection is retried with backoff and continues from the last written byte;
//!   after a restart the `.part` file's length is where the copy resumes.
//! - Reads go through a per-source [`Gate`]: playback reads outrank downloads and probes,
//!   which share a small per-source concurrency cap.
//! - Free space is checked before starting and while writing; running out is its own error
//!   category ([`ErrorKind::NoSpace`]) and leaves no partial file behind.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use huehuecoyotl_library::copies::{self, Copy, NewCopy};
use huehuecoyotl_library::{catalog, jobs};
use huehuecoyotl_vfs::{Caps, Entry, LocalSource, Source, VPath, VfsError, find_sidecar_subtitles};
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Notify, Semaphore};

use crate::{CResult, CoreError, ErrorKind, Event, Inner};

pub const DOWNLOAD_KINDS: &[&str] = &["download"];

/// Bytes per read.
const CHUNK: u32 = 1 << 20;
/// Kept free on the target drive beyond the file itself.
const RESERVE: u64 = 32 << 20;
/// Free space is checked again after this many bytes.
const SPACE_CHECK_EVERY: u64 = 64 << 20;
/// Downloads running at the same time (over all sources).
const PARALLEL: usize = 2;
/// Background reads (downloads, probes) in flight per source.
const BACKGROUND_PER_SOURCE: usize = 2;
/// Retries of one read before the job backs off: 0.5 s .. 30 s, about 90 s in total.
const RETRIES: u32 = 8;
/// Progress events per download at most this often.
const EVENT_EVERY: Duration = Duration::from_millis(500);
const DB_EVERY: Duration = Duration::from_secs(2);

pub const STORAGE_KEY: &str = "storage_dir";

// ---- read priorities ----------------------------------------------------------------------

/// Read scheduling for one source: playback first, then a few background reads at a time.
pub(crate) struct Gate {
    playback: AtomicUsize,
    last_playback: Mutex<Option<Instant>>,
    background: Semaphore,
}

impl Gate {
    fn new() -> Self {
        Self {
            playback: AtomicUsize::new(0),
            last_playback: Mutex::new(None),
            background: Semaphore::new(BACKGROUND_PER_SOURCE),
        }
    }

    fn playback_busy(&self) -> bool {
        self.playback.load(Ordering::Acquire) > 0
            || self
                .last_playback
                .lock()
                .unwrap()
                .is_some_and(|t| t.elapsed() < Duration::from_millis(150))
    }

    /// Wait while playback is reading (at most two seconds, so a download never starves).
    async fn yield_to_playback(&self) {
        let until = Instant::now() + Duration::from_secs(2);
        while self.playback_busy() && Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

struct PlaybackRead<'a>(&'a Gate);

impl Drop for PlaybackRead<'_> {
    fn drop(&mut self) {
        *self.0.last_playback.lock().unwrap() = Some(Instant::now());
        self.0.playback.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A source seen through its [`Gate`], as playback or as background work.
pub(crate) struct Prioritised {
    src: Arc<dyn Source>,
    gate: Arc<Gate>,
    playback: bool,
}

impl Prioritised {
    pub(crate) fn playback(src: Arc<dyn Source>, gate: Arc<Gate>) -> Self {
        Self {
            src,
            gate,
            playback: true,
        }
    }
    pub(crate) fn background(src: Arc<dyn Source>, gate: Arc<Gate>) -> Self {
        Self {
            src,
            gate,
            playback: false,
        }
    }
}

#[async_trait]
impl Source for Prioritised {
    async fn list(&self, dir: &VPath) -> huehuecoyotl_vfs::Result<Vec<Entry>> {
        self.src.list(dir).await
    }
    async fn stat(&self, path: &VPath) -> huehuecoyotl_vfs::Result<Entry> {
        self.src.stat(path).await
    }
    async fn read_at(
        &self,
        path: &VPath,
        offset: u64,
        len: u32,
    ) -> huehuecoyotl_vfs::Result<Bytes> {
        if self.playback {
            self.gate.playback.fetch_add(1, Ordering::AcqRel);
            let _read = PlaybackRead(&self.gate);
            return self.src.read_at(path, offset, len).await;
        }
        let _permit = self
            .gate
            .background
            .acquire()
            .await
            .map_err(|e| VfsError::Internal(e.to_string()))?;
        self.gate.yield_to_playback().await;
        self.src.read_at(path, offset, len).await
    }
    fn capabilities(&self) -> Caps {
        self.src.capabilities()
    }
}

impl Inner {
    pub(crate) fn gate(&self, source_id: i64) -> Arc<Gate> {
        self.gates
            .lock()
            .unwrap()
            .entry(source_id)
            .or_insert_with(|| Arc::new(Gate::new()))
            .clone()
    }

    /// A file's source for background work (downloads, probes).
    pub(crate) async fn background_source(
        self: &Arc<Self>,
        source_id: i64,
        kind: &str,
    ) -> CResult<Arc<dyn Source>> {
        if kind == "local" {
            return Ok(Arc::new(LocalSource));
        }
        let src: Arc<dyn Source> = self.remote_source(source_id).await?;
        Ok(Arc::new(Prioritised::background(src, self.gate(source_id))))
    }
}

// ---- free space ---------------------------------------------------------------------------

pub(crate) type FreeSpace = dyn Fn(&Path) -> io::Result<u64> + Send + Sync;

/// Bytes available to this user on the drive that holds `path` (or would hold it).
pub fn available_space(path: &Path) -> io::Result<u64> {
    let mut p = path;
    while !p.exists() {
        p = p
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no existing folder"))?;
    }
    imp::available(p)
}

#[cfg(unix)]
mod imp {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    pub fn available(p: &Path) -> io::Result<u64> {
        let c = CString::new(p.as_os_str().as_bytes())?;
        // SAFETY: statvfs is plain old data; zeroed is a valid value for it.
        let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: `c` is a valid NUL-terminated path and `s` a valid out pointer.
        if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((s.f_bavail as u64).saturating_mul(s.f_frsize as u64))
    }
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    pub fn available(p: &Path) -> io::Result<u64> {
        let w: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut avail = 0u64;
        // SAFETY: `w` is NUL-terminated; the other out pointers may be null.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                w.as_ptr(),
                &mut avail,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(avail)
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub fn available(_: &std::path::Path) -> std::io::Result<u64> {
        Ok(u64::MAX)
    }
}

fn human(bytes: u64) -> String {
    let gb = bytes as f64 / 1e9;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / 1e6)
    }
}

// ---- one file -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    Pause,
    Cancel,
}

/// Pause and cancel for a running copy.
pub(crate) struct Ctl {
    stop: AtomicU8,
    notify: Notify,
}

impl Ctl {
    pub(crate) fn new() -> Self {
        Self {
            stop: AtomicU8::new(0),
            notify: Notify::new(),
        }
    }
    pub(crate) fn request(&self, s: Stop) {
        self.stop.store(
            match s {
                Stop::Pause => 1,
                Stop::Cancel => 2,
            },
            Ordering::Release,
        );
        self.notify.notify_waiters();
    }
    fn stopped(&self) -> Option<Stop> {
        match self.stop.load(Ordering::Acquire) {
            1 => Some(Stop::Pause),
            2 => Some(Stop::Cancel),
            _ => None,
        }
    }
    async fn wait(&self) -> Stop {
        loop {
            let n = self.notify.notified();
            tokio::pin!(n);
            n.as_mut().enable();
            if let Some(s) = self.stopped() {
                return s;
            }
            n.await;
        }
    }
}

#[derive(Debug)]
pub(crate) enum FetchError {
    Stopped(Stop),
    NoSpace(String),
    Source(VfsError),
    /// Writing the local file failed (permissions, a removed drive).
    Write(io::Error),
    Verify(String),
}

impl FetchError {
    fn kind(&self) -> ErrorKind {
        match self {
            Self::NoSpace(_) => ErrorKind::NoSpace,
            Self::Source(e) => vfs_kind(e),
            Self::Write(e) if e.kind() == io::ErrorKind::PermissionDenied => ErrorKind::Auth,
            Self::Write(e) if e.kind() == io::ErrorKind::NotFound => ErrorKind::NotFound,
            _ => ErrorKind::Internal,
        }
    }
    fn message(&self, dest: &Path) -> String {
        match self {
            Self::Stopped(_) => "stopped".into(),
            Self::NoSpace(m) => m.clone(),
            Self::Source(e) => e.to_string(),
            Self::Write(e) => format!(
                "Could not write to {}: {e}",
                dest.parent().unwrap_or(dest).display()
            ),
            Self::Verify(m) => m.clone(),
        }
    }
}

fn vfs_kind(e: &VfsError) -> ErrorKind {
    match e {
        VfsError::Auth(_) => ErrorKind::Auth,
        VfsError::Unreachable(_) => ErrorKind::Unreachable,
        VfsError::NotFound(_) => ErrorKind::NotFound,
        VfsError::Unsupported(_) => ErrorKind::Unsupported,
        VfsError::Internal(_) => ErrorKind::Internal,
    }
}

fn transient(e: &VfsError) -> bool {
    matches!(e, VfsError::Unreachable(_) | VfsError::Internal(_))
}

fn is_no_space(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::StorageFull || e.kind() == io::ErrorKind::QuotaExceeded
}

/// One file to copy.
pub(crate) struct Fetch<'a> {
    pub src: &'a dyn Source,
    pub path: &'a VPath,
    pub size: u64,
    pub part: &'a Path,
    /// The library's fingerprint of the source file, checked on the copy.
    pub fingerprint: Option<&'a [u8]>,
    pub ctl: &'a Ctl,
    pub free: &'a FreeSpace,
}

fn check_space(f: &Fetch<'_>, remaining: u64) -> Result<(), FetchError> {
    let dir = f.part.parent().unwrap_or(f.part);
    let avail = (f.free)(dir).map_err(FetchError::Write)?;
    let need = remaining.saturating_add(RESERVE);
    if avail < need {
        return Err(FetchError::NoSpace(format!(
            "Not enough free space on {}: {} needed, {} free",
            dir.display(),
            human(need),
            human(avail)
        )));
    }
    Ok(())
}

/// Copy `f.path` into `f.part`, continuing after what the `.part` file already holds, then
/// verify it. A failed check or a full disk removes the `.part` file; a pause keeps it.
pub(crate) async fn fetch(
    f: &Fetch<'_>,
    progress: &mut (dyn FnMut(u64) + Send),
) -> Result<(), FetchError> {
    let r = fetch_inner(f, progress).await;
    if matches!(
        r,
        Err(FetchError::NoSpace(_) | FetchError::Verify(_) | FetchError::Stopped(Stop::Cancel))
    ) {
        let _ = tokio::fs::remove_file(f.part).await;
    }
    r
}

async fn fetch_inner(
    f: &Fetch<'_>,
    progress: &mut (dyn FnMut(u64) + Send),
) -> Result<(), FetchError> {
    let w = |e: io::Error| {
        if is_no_space(&e) {
            FetchError::NoSpace(format!("The drive is full: {e}"))
        } else {
            FetchError::Write(e)
        }
    };
    if let Some(dir) = f.part.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(w)?;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(f.part)
        .await
        .map_err(w)?;
    let mut offset = file.metadata().await.map_err(w)?.len();
    if offset > f.size {
        file.set_len(0).await.map_err(w)?;
        offset = 0;
    }
    check_space(f, f.size - offset)?;
    progress(offset);
    let mut since_check = 0u64;
    let mut failures = 0u32;
    while offset < f.size {
        if let Some(s) = f.ctl.stopped() {
            file.flush().await.map_err(w)?;
            return Err(FetchError::Stopped(s));
        }
        let len = (f.size - offset).min(u64::from(CHUNK)) as u32;
        let read = tokio::select! {
            r = f.src.read_at(f.path, offset, len) => r,
            s = f.ctl.wait() => {
                file.flush().await.map_err(w)?;
                return Err(FetchError::Stopped(s));
            }
        };
        let data = match read {
            Ok(b) if b.is_empty() => {
                return Err(FetchError::Source(VfsError::Internal(format!(
                    "the file ended after {offset} of {} bytes",
                    f.size
                ))));
            }
            Ok(b) => {
                failures = 0;
                b
            }
            Err(e) if transient(&e) && failures < RETRIES => {
                let wait = Duration::from_millis(500 * (1u64 << failures.min(6)))
                    .min(Duration::from_secs(30));
                failures += 1;
                tracing::debug!(
                    offset,
                    attempt = failures,
                    "download read failed, retrying: {e}"
                );
                tokio::select! {
                    _ = tokio::time::sleep(wait) => {}
                    s = f.ctl.wait() => {
                        file.flush().await.map_err(w)?;
                        return Err(FetchError::Stopped(s));
                    }
                }
                continue;
            }
            Err(e) => {
                file.flush().await.map_err(w)?;
                return Err(FetchError::Source(e));
            }
        };
        let n = data.len().min((f.size - offset) as usize);
        file.write_all(&data[..n]).await.map_err(w)?;
        offset += n as u64;
        since_check += n as u64;
        progress(offset);
        if since_check >= SPACE_CHECK_EVERY {
            since_check = 0;
            file.flush().await.map_err(w)?;
            check_space(f, f.size - offset)?;
        }
    }
    file.flush().await.map_err(w)?;
    file.sync_all().await.map_err(w)?;
    drop(file);
    let len = tokio::fs::metadata(f.part).await.map_err(w)?.len();
    if len != f.size {
        return Err(FetchError::Verify(format!(
            "The copy has {len} bytes, the original {}",
            f.size
        )));
    }
    if let Some(want) = f.fingerprint {
        let local = VPath::new(f.part.to_string_lossy());
        let got = huehuecoyotl_scanner::scan::fingerprint(&LocalSource, &local, f.size)
            .await
            .map_err(FetchError::Source)?;
        if got != want {
            return Err(FetchError::Verify(
                "The copy differs from the original (fingerprint mismatch)".into(),
            ));
        }
    }
    Ok(())
}

// ---- managed storage ----------------------------------------------------------------------

/// Where saved copies go: the configured folder, or `Saved` in the data directory.
pub(crate) async fn storage_dir(inner: &Arc<Inner>) -> PathBuf {
    let set = inner
        .lib
        .read(|c| copies::setting(c, STORAGE_KEY))
        .await
        .ok()
        .flatten();
    match set {
        Some(s) if !s.is_empty() => PathBuf::from(s),
        _ => inner.data_dir.join("Saved"),
    }
}

/// A file or folder name without characters Windows and FAT drives refuse.
fn clean(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s.trim_end_matches(['.', ' ']).to_string();
    if s.is_empty() { "Unknown".into() } else { s }
}

fn part_of(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// The progress of a row as an event.
fn progress_event(row: &Copy) -> Event {
    Event::DownloadProgress {
        id: row.id,
        main_id: row.main_id.unwrap_or(row.id),
        item_id: row.item_id.unwrap_or(0),
        state: row.state.clone(),
        bytes_done: row.bytes_done,
        size: row.size,
        error_kind: row.error_kind.clone(),
        error: row.error.clone(),
    }
}

async fn post_row(inner: &Arc<Inner>, id: i64) {
    if let Ok(Some(row)) = inner.lib.read(move |c| copies::get(c, id)).await {
        inner.sink.post(progress_event(&row));
        if let (Some(item_id), "done" | "failed" | "paused" | "outdated") =
            (row.item_id, row.state.as_str())
        {
            inner.sink.post(Event::ItemChanged { item_id });
        }
    }
}

fn enqueue(c: &rusqlite::Connection, id: i64, prio: i64) -> huehuecoyotl_library::Result<bool> {
    jobs::enqueue(
        c,
        "download",
        &json!({ "copy_id": id }).to_string(),
        prio,
        Some(&format!("download:{id}")),
    )
}

use huehuecoyotl_library::rusqlite;

/// Queue a copy of one library file. `dest` is the local file to write; `None` puts a saved
/// copy into managed storage (`Title (Year)/original name`).
async fn add_copy(
    inner: &Arc<Inner>,
    kind: &'static str,
    file_id: i64,
    dest: Option<PathBuf>,
) -> CResult<i64> {
    let t = inner
        .lib
        .read(move |c| copies::target(c, file_id))
        .await?
        .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("file {file_id}")))?;
    let name = t.path.rsplit('/').next().unwrap_or(&t.path).to_string();
    let dest = match dest {
        Some(d) => d,
        None => {
            let item = t.item_id;
            let dirs = inner.lib.read(move |c| copies::layout_dir(c, item)).await?;
            let root = storage_dir(inner).await;
            let mut base = root.clone();
            for d in &dirs[..dirs.len() - 1] {
                base.push(clean(d));
            }
            let last = clean(&dirs[dirs.len() - 1]);
            // Two versions with the same file name: "Alien (1979) [2]/Alien.mkv".
            let mut n = 1;
            loop {
                let dir = if n == 1 {
                    base.join(&last)
                } else {
                    base.join(format!("{last} [{n}]"))
                };
                let p = dir.join(clean(&name));
                let ps = p.to_string_lossy().to_string();
                let taken = inner.lib.read(move |c| copies::path_taken(c, &ps)).await?;
                if !taken && !p.exists() && !part_of(&p).exists() {
                    break p;
                }
                n += 1;
            }
        }
    };
    let row = NewCopy {
        kind: kind.into(),
        main_id: None,
        file_id: Some(t.file_id),
        item_id: Some(t.item_id),
        source_id: t.source_id,
        source_path: t.path.clone(),
        src_mtime: t.mtime,
        path: dest.to_string_lossy().to_string(),
        size: t.size,
    };
    let id = inner
        .lib
        .write(move |c| {
            let id = copies::insert(c, &row)?;
            enqueue(c, id, jobs::priority::USER)?;
            Ok(id)
        })
        .await?;
    inner.wake.notify_waiters();
    post_row(inner, id).await;
    Ok(id)
}

/// Remove a copy's files (and `.part` files) and its rows, stopping it first if it runs.
pub(crate) async fn delete_copy(inner: &Arc<Inner>, id: i64) -> CResult<()> {
    let rows = inner
        .lib
        .read(move |c| {
            let mut v: Vec<Copy> = copies::get(c, id)?.into_iter().collect();
            v.extend(copies::companions(c, id)?);
            Ok(v)
        })
        .await?;
    let Some(main) = rows.first().cloned() else {
        return Ok(());
    };
    for r in &rows {
        if let Some(ctl) = inner.downloads.lock().unwrap().get(&r.id) {
            ctl.request(Stop::Cancel);
        }
    }
    inner.lib.write(move |c| copies::delete(c, id)).await?;
    for r in &rows {
        let p = PathBuf::from(&r.path);
        // An export that finished belongs to the user: only its partial file is removed.
        if !(r.kind == copies::EXPORT && r.state == "done") {
            let _ = tokio::fs::remove_file(&p).await;
        }
        let _ = tokio::fs::remove_file(part_of(&p)).await;
    }
    // Remove folders a saved copy leaves empty (inside managed storage only).
    if main.kind == copies::SAVED {
        let root = storage_dir(inner).await;
        let mut dir = PathBuf::from(&main.path).parent().map(Path::to_path_buf);
        while let Some(d) = dir {
            if !d.starts_with(&root) || d == root || std::fs::remove_dir(&d).is_err() {
                break;
            }
            dir = d.parent().map(Path::to_path_buf);
        }
    }
    inner.sink.post(Event::DownloadProgress {
        id,
        main_id: id,
        item_id: main.item_id.unwrap_or(0),
        state: "removed".into(),
        bytes_done: 0,
        size: 0,
        error_kind: String::new(),
        error: String::new(),
    });
    if let Some(item_id) = main.item_id {
        inner.sink.post(Event::ItemChanged { item_id });
    }
    Ok(())
}

pub(crate) async fn pause(inner: &Arc<Inner>, id: i64) -> CResult<()> {
    let ids = family(inner, id).await?;
    for r in &ids {
        let rid = r.id;
        if matches!(r.state.as_str(), "queued" | "running") {
            inner
                .lib
                .write(move |c| {
                    copies::set_state(c, rid, "paused")?;
                    c.execute(
                        "DELETE FROM job WHERE dedupe = ?1 AND state = 'queued'",
                        [format!("download:{rid}")],
                    )?;
                    Ok(())
                })
                .await?;
            if let Some(ctl) = inner.downloads.lock().unwrap().get(&rid) {
                ctl.request(Stop::Pause);
            }
            post_row(inner, rid).await;
        }
    }
    Ok(())
}

pub(crate) async fn resume(inner: &Arc<Inner>, id: i64) -> CResult<()> {
    for r in family(inner, id).await? {
        let rid = r.id;
        if matches!(r.state.as_str(), "paused" | "failed") {
            inner
                .lib
                .write(move |c| {
                    copies::set_state(c, rid, "queued")?;
                    // A failed job row would block the dedupe key; start it afresh.
                    c.execute(
                        "DELETE FROM job WHERE dedupe = ?1 AND state = 'failed'",
                        [format!("download:{rid}")],
                    )?;
                    enqueue(c, rid, jobs::priority::USER)
                })
                .await?;
            post_row(inner, rid).await;
        }
    }
    inner.wake.notify_waiters();
    Ok(())
}

/// A main row and its companions.
async fn family(inner: &Arc<Inner>, id: i64) -> CResult<Vec<Copy>> {
    Ok(inner
        .lib
        .read(move |c| {
            let mut v: Vec<Copy> = copies::get(c, id)?.into_iter().collect();
            v.extend(copies::companions(c, id)?);
            Ok(v)
        })
        .await?)
}

/// "Save": offline copies of a movie or episode (one version, or the largest network one),
/// or of every episode of a season or show. Already saved or running copies are kept; failed
/// and outdated ones are made again. Returns the new main rows.
pub(crate) async fn save(inner: &Arc<Inner>, item_id: i64, file_id: i64) -> CResult<Vec<i64>> {
    let items = inner
        .lib
        .read(move |c| copies::playable_items(c, item_id))
        .await?;
    let single = items.len() == 1 && items[0] == item_id;
    let mut made = Vec::new();
    for it in items {
        let file = if single && file_id > 0 {
            let t = inner
                .lib
                .read(move |c| copies::target(c, file_id))
                .await?
                .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("file {file_id}")))?;
            if t.source_kind == "local" {
                return Err(CoreError::new(
                    ErrorKind::Unsupported,
                    "This file is already on this computer.",
                ));
            }
            Some(file_id)
        } else {
            inner
                .lib
                .read(move |c| copies::network_files(c, it))
                .await?
                .first()
                .copied()
        };
        let Some(file) = file else { continue };
        // One saved copy per item: a copy of another version counts for a season or show.
        let existing = inner
            .lib
            .read(move |c| {
                Ok(match copies::saved_for_file(c, file)? {
                    Some(r) => Some(r),
                    None if !single => copies::saved_for_item(c, it)?.into_iter().next(),
                    None => None,
                })
            })
            .await?;
        if let Some(r) = existing {
            if matches!(r.state.as_str(), "failed" | "outdated") {
                delete_copy(inner, r.id).await?;
            } else {
                if r.state == "paused" {
                    resume(inner, r.id).await?;
                }
                continue;
            }
        }
        made.push(add_copy(inner, copies::SAVED, file, None).await?);
    }
    if made.is_empty() && single && file_id == 0 {
        let has_network = !inner
            .lib
            .read(move |c| copies::network_files(c, item_id))
            .await?
            .is_empty();
        if !has_network {
            return Err(CoreError::new(
                ErrorKind::Unsupported,
                "Only files on network sources can be saved.",
            ));
        }
    }
    inner.mark_changed();
    Ok(made)
}

/// "Save as…": copy one file to `dest`, a file path chosen by the user.
pub(crate) async fn export(inner: &Arc<Inner>, file_id: i64, dest: PathBuf) -> CResult<i64> {
    add_copy(inner, copies::EXPORT, file_id, Some(dest)).await
}

/// Sidecar subtitles next to the main file, and the other parts of a multi-part file: rows of
/// their own, added once when the main file starts.
async fn add_companions(inner: &Arc<Inner>, main: &Copy, src: &dyn Source) {
    let main_id = main.id;
    let have: HashSet<String> = inner
        .lib
        .read(move |c| copies::companions(c, main_id))
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.source_path)
        .collect();
    let vp = VPath::new(&main.source_path);
    let dest_dir = PathBuf::from(&main.path)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let mut new = Vec::new();
    if let Some(dir) = vp.parent() {
        if let Ok(siblings) = src.list(&dir).await {
            for e in find_sidecar_subtitles(vp.file_name(), &siblings) {
                new.push((None, e.path.to_string(), e.mtime, e.size, e.name.clone()));
            }
        }
    }
    // Other parts (cd1, part 2, ...) of the same movie in the same folder.
    let name = vp.file_name().to_string();
    if huehuecoyotl_scanner::parse::parse(&name).part.is_some() {
        if let Some(item) = main.item_id {
            let dir = vp.parent().map(|p| p.to_string()).unwrap_or_default();
            let mine = main.file_id;
            let parts = inner
                .lib
                .read(move |c| {
                    let mut out = Vec::new();
                    for id in copies::network_files(c, item)? {
                        if Some(id) == mine {
                            continue;
                        }
                        if let Some(t) = copies::target(c, id)? {
                            let p = VPath::new(&t.path);
                            let same_dir = p.parent().map(|d| d.to_string()) == Some(dir.clone());
                            if same_dir
                                && huehuecoyotl_scanner::parse::parse(p.file_name())
                                    .part
                                    .is_some()
                            {
                                out.push(t);
                            }
                        }
                    }
                    Ok(out)
                })
                .await
                .unwrap_or_default();
            for t in parts {
                let n = t.path.rsplit('/').next().unwrap_or(&t.path).to_string();
                new.push((Some(t.file_id), t.path, t.mtime.unwrap_or(0), t.size, n));
            }
        }
    }
    let (kind, item_id, source_id) = (main.kind.clone(), main.item_id, main.source_id);
    let rows: Vec<NewCopy> = new
        .into_iter()
        .filter(|(_, p, ..)| !have.contains(p))
        .map(|(file_id, source_path, mtime, size, name)| NewCopy {
            kind: kind.clone(),
            main_id: Some(main_id),
            file_id,
            item_id,
            source_id,
            source_path,
            src_mtime: Some(mtime),
            path: dest_dir.join(clean(&name)).to_string_lossy().to_string(),
            size,
        })
        .collect();
    if rows.is_empty() {
        return;
    }
    let r = inner
        .lib
        .write(move |c| {
            for r in &rows {
                let id = copies::insert(c, r)?;
                enqueue(c, id, jobs::priority::USER)?;
            }
            Ok(())
        })
        .await;
    if let Err(e) = r {
        tracing::error!(main_id, "adding subtitles and parts failed: {e}");
    }
    inner.wake.notify_waiters();
}

/// What happened to one job.
enum Done {
    Finished,
    /// Try again later (counts as an attempt).
    Retry(String),
}

async fn run(inner: &Arc<Inner>, id: i64, attempts: i64) -> Done {
    let Ok(Some(row)) = inner.lib.read(move |c| copies::get(c, id)).await else {
        return Done::Finished; // deleted meanwhile
    };
    if !matches!(row.state.as_str(), "queued" | "running") {
        return Done::Finished; // paused or finished meanwhile
    }
    let _ = inner
        .lib
        .write(move |c| copies::set_state(c, id, "running"))
        .await;
    post_row(inner, id).await;
    let ctl = Arc::new(Ctl::new());
    inner.downloads.lock().unwrap().insert(id, ctl.clone());
    let res = transfer(inner, &row, &ctl).await;
    inner.downloads.lock().unwrap().remove(&id);
    let dest = PathBuf::from(&row.path);
    let out = match res {
        Ok(()) => {
            let _ = inner.lib.write(move |c| copies::set_done(c, id)).await;
            tracing::info!(id, path = %row.path, "download complete");
            Done::Finished
        }
        Err(FetchError::Stopped(Stop::Pause)) => {
            let done = std::fs::metadata(part_of(&dest)).map_or(0, |m| m.len());
            let _ = inner
                .lib
                .write(move |c| copies::set_progress(c, id, done))
                .await;
            Done::Finished
        }
        Err(FetchError::Stopped(Stop::Cancel)) => return Done::Finished,
        Err(FetchError::Source(e)) if transient(&e) && attempts < jobs::MAX_ATTEMPTS => {
            let done = std::fs::metadata(part_of(&dest)).map_or(0, |m| m.len());
            let _ = inner
                .lib
                .write(move |c| {
                    copies::set_progress(c, id, done)?;
                    copies::set_state(c, id, "queued")
                })
                .await;
            if matches!(e, VfsError::Unreachable(_)) {
                inner.set_online(row.source_id, false);
            }
            post_row(inner, id).await;
            return Done::Retry(e.to_string());
        }
        Err(e) => {
            let (kind, msg) = (kind_name(e.kind()), e.message(&dest));
            tracing::warn!(id, kind, "download failed: {msg}");
            let _ = inner
                .lib
                .write(move |c| copies::set_failed(c, id, kind, &msg))
                .await;
            Done::Finished
        }
    };
    post_row(inner, id).await;
    inner.mark_changed();
    out
}

pub(crate) fn kind_name(k: ErrorKind) -> &'static str {
    match k {
        ErrorKind::Auth => "auth",
        ErrorKind::Unreachable => "unreachable",
        ErrorKind::NotFound => "not_found",
        ErrorKind::Unsupported => "unsupported",
        ErrorKind::Internal => "internal",
        ErrorKind::NoSpace => "no_space",
    }
}

fn core_to_vfs(e: CoreError) -> VfsError {
    match e.kind {
        ErrorKind::Auth => VfsError::Auth(e.message),
        ErrorKind::Unreachable => VfsError::Unreachable(e.message),
        ErrorKind::NotFound => VfsError::NotFound(e.message),
        ErrorKind::Unsupported => VfsError::Unsupported(e.message),
        ErrorKind::Internal | ErrorKind::NoSpace => VfsError::Internal(e.message),
    }
}

async fn transfer(inner: &Arc<Inner>, row: &Copy, ctl: &Ctl) -> Result<(), FetchError> {
    let sid = row.source_id;
    let kind = inner
        .lib
        .read(move |c| catalog::source(c, sid))
        .await
        .map_err(|e| FetchError::Source(VfsError::Internal(e.to_string())))?
        .map(|s| s.kind)
        .ok_or_else(|| FetchError::Source(VfsError::NotFound("the library was removed".into())))?;
    let src = inner
        .background_source(sid, &kind)
        .await
        .map_err(|e| FetchError::Source(core_to_vfs(e)))?;
    let vp = VPath::new(&row.source_path);
    let entry = src.stat(&vp).await.map_err(FetchError::Source)?;
    let dest = PathBuf::from(&row.path);
    let part = part_of(&dest);
    let changed = entry.size != row.size
        || (row.bytes_done > 0
            && entry.mtime != 0
            && row.src_mtime.is_some_and(|m| m != 0 && m != entry.mtime));
    if changed {
        // Not the file the copy started from: begin again.
        let _ = tokio::fs::remove_file(&part).await;
        let (id, size, mtime) = (row.id, entry.size, entry.mtime);
        inner
            .lib
            .write(move |c| copies::restart(c, id, size, Some(mtime)))
            .await
            .map_err(|e| FetchError::Source(VfsError::Internal(e.to_string())))?;
    }
    if row.main_id.is_none() {
        add_companions(inner, row, &*src).await;
    }
    let fingerprint = match row.file_id {
        Some(f) if !changed => inner
            .lib
            .read(move |c| copies::target(c, f))
            .await
            .ok()
            .flatten()
            .filter(|t| t.size == entry.size)
            .and_then(|t| t.fingerprint),
        _ => None,
    };
    let free = &available_space as &FreeSpace;
    let f = Fetch {
        src: &*src,
        path: &vp,
        size: entry.size,
        part: &part,
        fingerprint: fingerprint.as_deref(),
        ctl,
        free,
    };
    let (mut last_ev, mut last_db) = (Instant::now() - EVENT_EVERY, Instant::now());
    let (id, main_id, item_id, total) = (
        row.id,
        row.main_id.unwrap_or(row.id),
        row.item_id.unwrap_or(0),
        entry.size,
    );
    let sink = inner.clone();
    let mut on_progress = move |done: u64| {
        if last_ev.elapsed() >= EVENT_EVERY || done == total {
            last_ev = Instant::now();
            sink.sink.post(Event::DownloadProgress {
                id,
                main_id,
                item_id,
                state: "running".into(),
                bytes_done: done,
                size: total,
                error_kind: String::new(),
                error: String::new(),
            });
        }
        if last_db.elapsed() >= DB_EVERY {
            last_db = Instant::now();
            drop(sink.lib.submit(move |c| copies::set_progress(c, id, done)));
        }
    };
    fetch(&f, &mut on_progress).await?;
    tokio::fs::rename(&part, &dest)
        .await
        .map_err(FetchError::Write)?;
    Ok(())
}

// ---- worker and start-up ------------------------------------------------------------------

pub(crate) fn spawn_worker(inner: &Arc<Inner>) {
    let inner = inner.clone();
    tokio::spawn(async move {
        let slots = Arc::new(Semaphore::new(PARALLEL));
        loop {
            let Ok(permit) = slots.clone().acquire_owned().await else {
                return;
            };
            let job = inner
                .lib
                .write(|c| jobs::claim(c, DOWNLOAD_KINDS))
                .await
                .unwrap_or_else(|e| {
                    tracing::error!("claiming a download failed: {e}");
                    None
                });
            let Some(job) = job else {
                drop(permit);
                let _ = tokio::time::timeout(Duration::from_secs(20), inner.wake.notified()).await;
                continue;
            };
            let me = inner.clone();
            tokio::spawn(async move {
                let v: serde_json::Value = serde_json::from_str(&job.payload).unwrap_or_default();
                let copy = v["copy_id"].as_i64().unwrap_or(0);
                let jid = job.id;
                let res = match run(&me, copy, job.attempts).await {
                    Done::Finished => me.lib.write(move |c| jobs::complete(c, jid)).await,
                    Done::Retry(err) => {
                        tracing::warn!(copy, attempt = job.attempts, "download interrupted: {err}");
                        me.lib.write(move |c| jobs::fail(c, jid, &err, None)).await
                    }
                };
                if let Err(e) = res {
                    tracing::error!("updating download job {jid} failed: {e}");
                }
                drop(permit);
                me.wake.notify_waiters();
            });
        }
    });
}

/// At start-up: drop exports of the last session (removing their partial files), queue
/// unfinished copies again, mark saved copies whose file disappeared, and remove `.part` files
/// in managed storage that no row knows about.
pub(crate) async fn startup(inner: &Arc<Inner>) {
    let rows = inner.lib.read(copies::all).await.unwrap_or_default();
    let mut known: HashSet<PathBuf> = HashSet::new();
    let mut gone = Vec::new();
    let mut requeue = Vec::new();
    let mut missing = Vec::new();
    for r in &rows {
        let p = PathBuf::from(&r.path);
        if r.kind == copies::EXPORT {
            if r.state != "done" {
                let _ = std::fs::remove_file(part_of(&p));
            }
            if r.main_id.is_none() {
                gone.push(r.id);
            }
            continue;
        }
        known.insert(part_of(&p));
        match r.state.as_str() {
            "queued" | "running" => requeue.push(r.id),
            "done" if !p.exists() => missing.push(r.id),
            _ => {}
        }
    }
    let r = inner
        .lib
        .write(move |c| {
            for id in &gone {
                copies::delete(c, *id)?;
            }
            for id in &requeue {
                copies::set_state(c, *id, "queued")?;
                enqueue(c, *id, jobs::priority::USER)?;
            }
            for id in &missing {
                copies::set_failed(c, *id, "not_found", "The saved copy is no longer on disk.")?;
            }
            Ok(())
        })
        .await;
    if let Err(e) = r {
        tracing::error!("restoring downloads failed: {e}");
    }
    let root = storage_dir(inner).await;
    let orphans = tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "part") && !known.contains(&p) {
                    out.push(p);
                }
            }
        }
        for p in &out {
            let _ = std::fs::remove_file(p);
        }
        out.len()
    })
    .await
    .unwrap_or(0);
    if orphans > 0 {
        tracing::info!(files = orphans, "removed orphaned partial downloads");
    }
}

// ---- listing ------------------------------------------------------------------------------

/// One main row with its companions summed up, for the Downloads screen and the CLI.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct DownloadInfo {
    pub id: i64,
    /// `saved` or `export`.
    pub kind: String,
    pub item_id: i64,
    pub file_id: i64,
    pub title: String,
    pub path: String,
    pub state: String,
    pub bytes_done: u64,
    pub size: u64,
    pub error_kind: String,
    pub error: String,
    pub created_at: i64,
    pub completed_at: i64,
}

/// The state shown for a family: the main file's, unless a companion is still busy or failed.
fn combined_state(main: &str, others: &[&Copy]) -> String {
    for s in ["running", "queued", "failed"] {
        if main == s || others.iter().any(|c| c.state == s) {
            if main == "paused" && s != "failed" {
                return main.into();
            }
            return s.into();
        }
    }
    main.into()
}

pub(crate) async fn list(inner: &Arc<Inner>) -> CResult<(Vec<DownloadInfo>, u64)> {
    let rows = inner.lib.read(copies::all).await?;
    let titles: HashMap<i64, String> = {
        let ids: Vec<i64> = rows.iter().filter_map(|r| r.item_id).collect();
        inner
            .lib
            .read(move |c| {
                let mut m = HashMap::new();
                for id in ids {
                    m.insert(id, display_title(c, id)?);
                }
                Ok(m)
            })
            .await?
    };
    let mut by_main: HashMap<i64, Vec<&Copy>> = HashMap::new();
    for r in rows.iter().filter(|r| r.main_id.is_some()) {
        by_main.entry(r.main_id.unwrap()).or_default().push(r);
    }
    let mut used = 0u64;
    let mut out = Vec::new();
    for m in rows.iter().filter(|r| r.main_id.is_none()) {
        let others = by_main.remove(&m.id).unwrap_or_default();
        let mut d = DownloadInfo {
            id: m.id,
            kind: m.kind.clone(),
            item_id: m.item_id.unwrap_or(0),
            file_id: m.file_id.unwrap_or(0),
            title: m
                .item_id
                .and_then(|i| titles.get(&i).cloned())
                .unwrap_or_else(|| {
                    PathBuf::from(&m.path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                }),
            path: m.path.clone(),
            state: combined_state(&m.state, &others),
            bytes_done: m.bytes_done,
            size: m.size,
            error_kind: m.error_kind.clone(),
            error: m.error.clone(),
            created_at: m.created_at,
            completed_at: m.completed_at.unwrap_or(0),
        };
        for o in &others {
            d.bytes_done += o.bytes_done;
            d.size += o.size;
            if d.error.is_empty() && !o.error.is_empty() {
                d.error = o.error.clone();
                d.error_kind = o.error_kind.clone();
            }
        }
        if m.kind == copies::SAVED {
            used += d.bytes_done;
        }
        out.push(d);
    }
    Ok((out, used))
}

/// kind, title, year, episode, show title, season.
type TitleRow = (
    String,
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<i64>,
);

/// "Alien (1979)", "Firefly · S01E02 · The Train Job".
fn display_title(c: &rusqlite::Connection, id: i64) -> huehuecoyotl_library::Result<String> {
    use huehuecoyotl_library::rusqlite::OptionalExtension;
    let r: Option<TitleRow> = c
        .query_row(
            "SELECT e.kind, e.title, e.year, e.episode_no, sh.title, se.season_no
             FROM item e LEFT JOIN item se ON se.id = e.parent_id LEFT JOIN item sh ON sh.id = se.parent_id
             WHERE e.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?;
    let Some((kind, title, year, ep, show, season)) = r else {
        return Ok(String::new());
    };
    let title = title.unwrap_or_default();
    Ok(if kind == "episode" {
        let code = format!("S{:02}E{:02}", season.unwrap_or(0), ep.unwrap_or(0));
        [show.unwrap_or_default(), code, title]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    } else {
        match year {
            Some(y) if y > 0 => format!("{title} ({y})"),
            _ => title,
        }
    })
}

/// Delete every saved copy (Settings → Storage), or those of one source (a removed library).
pub(crate) async fn delete_saved(inner: &Arc<Inner>, source: Option<i64>) -> CResult<usize> {
    let ids: Vec<i64> = inner
        .lib
        .read(copies::all)
        .await?
        .into_iter()
        .filter(|r| r.kind == copies::SAVED && r.main_id.is_none())
        .filter(|r| source.is_none_or(|s| r.source_id == s))
        .map(|r| r.id)
        .collect();
    for id in &ids {
        delete_copy(inner, *id).await?;
    }
    Ok(ids.len())
}

// ---- the core's API -----------------------------------------------------------------------

impl crate::Core {
    /// "Save" (issue #4): offline copies of a movie or episode, or of every episode of a
    /// season or show. `file_id` picks a version (0 = the largest network file). Answers with
    /// the Downloads list.
    pub fn save_item(&self, request_id: u64, item_id: i64, file_id: i64) {
        self.spawn(request_id, move |inner| async move {
            save(&inner, item_id, file_id).await?;
            downloads_event(&inner, request_id).await
        });
    }

    /// "Save as…" (issue #5): copy one file to `dest`. The caller has asked about overwriting.
    pub fn export_file(&self, request_id: u64, file_id: i64, dest: &str) {
        let dest = PathBuf::from(dest);
        self.spawn(request_id, move |inner| async move {
            export(&inner, file_id, dest).await?;
            downloads_event(&inner, request_id).await
        });
    }

    pub fn pause_download(&self, id: i64) {
        let inner = self.inner.clone();
        self.rt.spawn(async move {
            if let Err(e) = pause(&inner, id).await {
                tracing::error!(id, "pausing a download failed: {}", e.message);
            }
        });
    }

    /// Continue a paused copy, or try a failed one again.
    pub fn resume_download(&self, id: i64) {
        let inner = self.inner.clone();
        self.rt.spawn(async move {
            if let Err(e) = resume(&inner, id).await {
                tracing::error!(id, "resuming a download failed: {}", e.message);
            }
        });
    }

    /// Cancel a running copy or delete a saved one: its local files and its row go, the file on
    /// the source stays. A finished export is only forgotten (the file belongs to the user).
    pub fn delete_download(&self, request_id: u64, id: i64) {
        self.spawn(request_id, move |inner| async move {
            delete_copy(&inner, id).await?;
            inner.mark_changed();
            downloads_event(&inner, request_id).await
        });
    }

    /// Delete the saved copies of an item (a movie, an episode, or all of a season or show).
    pub fn delete_saved_item(&self, request_id: u64, item_id: i64) {
        self.spawn(request_id, move |inner| async move {
            let items = inner
                .lib
                .read(move |c| copies::playable_items(c, item_id))
                .await?;
            for it in items {
                let rows = inner
                    .lib
                    .read(move |c| copies::saved_for_item(c, it))
                    .await?;
                for r in rows {
                    delete_copy(&inner, r.id).await?;
                }
            }
            inner.mark_changed();
            downloads_event(&inner, request_id).await
        });
    }

    /// Pause or continue every running saved copy of an item (a season's, a show's).
    pub fn pause_saved_item(&self, item_id: i64) {
        self.each_saved(item_id, true);
    }
    pub fn resume_saved_item(&self, item_id: i64) {
        self.each_saved(item_id, false);
    }

    fn each_saved(&self, item_id: i64, stop: bool) {
        let inner = self.inner.clone();
        self.rt.spawn(async move {
            let r: CResult<()> = async {
                let items = inner
                    .lib
                    .read(move |c| copies::playable_items(c, item_id))
                    .await?;
                for it in items {
                    for row in inner
                        .lib
                        .read(move |c| copies::saved_for_item(c, it))
                        .await?
                    {
                        if stop {
                            pause(&inner, row.id).await?;
                        } else {
                            resume(&inner, row.id).await?;
                        }
                    }
                }
                Ok(())
            }
            .await;
            if let Err(e) = r {
                tracing::error!(
                    item_id,
                    "pausing or resuming saved copies failed: {}",
                    e.message
                );
            }
        });
    }

    /// The state of an item's saved copies (the detail page polls it while copies run).
    pub fn save_state(&self, request_id: u64, item_id: i64) {
        self.spawn(request_id, move |inner| async move {
            let info = inner
                .lib
                .read(move |c| copies::save_info(c, item_id))
                .await?;
            Ok(Event::SaveState {
                request_id,
                item_id,
                info,
            })
        });
    }

    /// Settings → Storage: "Delete all saved copies".
    pub fn delete_all_saved(&self, request_id: u64) {
        self.spawn(request_id, move |inner| async move {
            delete_saved(&inner, None).await?;
            inner.mark_changed();
            downloads_event(&inner, request_id).await
        });
    }

    pub fn downloads(&self, request_id: u64) {
        self.spawn(request_id, move |inner| async move {
            downloads_event(&inner, request_id).await
        });
    }

    /// Where new saved copies go; an empty path restores the default. Existing copies stay
    /// where they are.
    pub fn set_storage_dir(&self, request_id: u64, path: &str) {
        let path = path.to_string();
        self.spawn(request_id, move |inner| async move {
            if !path.is_empty() {
                std::fs::create_dir_all(&path)
                    .map_err(|e| CoreError::new(ErrorKind::Unsupported, format!("{path}: {e}")))?;
            }
            inner
                .lib
                .write(move |c| {
                    copies::set_setting(
                        c,
                        STORAGE_KEY,
                        Some(&path).filter(|p| !p.is_empty()).map(|p| p.as_str()),
                    )
                })
                .await?;
            downloads_event(&inner, request_id).await
        });
    }
}

async fn downloads_event(inner: &Arc<Inner>, request_id: u64) -> CResult<Event> {
    let (items, used_bytes) = list(inner).await?;
    Ok(Event::Downloads {
        request_id,
        items,
        used_bytes,
        storage_dir: storage_dir(inner).await.to_string_lossy().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    /// An in-memory source that counts bytes, can fail reads and can be slow.
    struct Mem {
        data: Vec<u8>,
        read: AtomicU64,
        fail_from: AtomicU64,
        fail_times: AtomicU64,
        /// Milliseconds per MiB read; reads are serialised like one connection.
        delay_ms: u64,
        conn: tokio::sync::Mutex<()>,
    }

    impl Mem {
        fn new(n: usize) -> Arc<Self> {
            Self::slow(n, 0)
        }
        fn slow(n: usize, delay_ms: u64) -> Arc<Self> {
            Arc::new(Self {
                data: (0..n).map(|i| (i * 7 % 251) as u8).collect(),
                read: AtomicU64::new(0),
                fail_from: AtomicU64::new(u64::MAX),
                fail_times: AtomicU64::new(0),
                delay_ms,
                conn: tokio::sync::Mutex::new(()),
            })
        }
    }

    #[async_trait]
    impl Source for Mem {
        async fn list(&self, _: &VPath) -> huehuecoyotl_vfs::Result<Vec<Entry>> {
            Ok(vec![])
        }
        async fn stat(&self, p: &VPath) -> huehuecoyotl_vfs::Result<Entry> {
            Ok(Entry {
                path: p.clone(),
                name: p.file_name().into(),
                is_dir: false,
                size: self.data.len() as u64,
                mtime: 1,
            })
        }
        async fn read_at(
            &self,
            _: &VPath,
            offset: u64,
            len: u32,
        ) -> huehuecoyotl_vfs::Result<Bytes> {
            let _conn = self.conn.lock().await;
            if self.delay_ms > 0 {
                let us = self.delay_ms * 1000 * u64::from(len) / (1 << 20);
                tokio::time::sleep(Duration::from_micros(us)).await;
            }
            if offset >= self.fail_from.load(Ordering::SeqCst)
                && self.fail_times.load(Ordering::SeqCst) > 0
            {
                self.fail_times.fetch_sub(1, Ordering::SeqCst);
                return Err(VfsError::Unreachable("connection reset".into()));
            }
            let s = (offset as usize).min(self.data.len());
            let e = (s + len as usize).min(self.data.len());
            self.read.fetch_add((e - s) as u64, Ordering::SeqCst);
            Ok(Bytes::copy_from_slice(&self.data[s..e]))
        }
        fn capabilities(&self) -> Caps {
            Caps::default()
        }
    }

    fn plenty(_: &Path) -> io::Result<u64> {
        Ok(u64::MAX / 2)
    }

    async fn fp(src: &Mem) -> Vec<u8> {
        huehuecoyotl_scanner::scan::fingerprint(src, &VPath::new("/m.mkv"), src.data.len() as u64)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn resumes_after_failures_without_starting_over() {
        let d = tempfile::tempdir().unwrap();
        let src = Mem::new(5 * CHUNK as usize + 123);
        let want = fp(&src).await;
        src.fail_from.store(2 * u64::from(CHUNK), Ordering::SeqCst);
        src.fail_times.store(2, Ordering::SeqCst);

        src.read.store(0, Ordering::SeqCst);
        let part = d.path().join("a/m.mkv.part");
        let ctl = Ctl::new();
        let path = VPath::new("/m.mkv");
        let f = Fetch {
            src: &*src,
            path: &path,
            size: src.data.len() as u64,
            part: &part,
            fingerprint: Some(&want),
            ctl: &ctl,
            free: &plenty,
        };
        let mut seen = Vec::new();
        fetch(&f, &mut |n| seen.push(n)).await.unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), src.data);
        assert_eq!(
            src.read.load(Ordering::SeqCst),
            src.data.len() as u64,
            "no byte read twice"
        );
        assert_eq!(*seen.last().unwrap(), src.data.len() as u64);
    }

    #[tokio::test]
    async fn a_stopped_copy_continues_from_its_part_file() {
        let d = tempfile::tempdir().unwrap();
        let src = Mem::slow(6 * CHUNK as usize, 5);
        let part = d.path().join("m.mkv.part");
        let path = VPath::new("/m.mkv");
        let ctl = Ctl::new();
        let f = Fetch {
            src: &*src,
            path: &path,
            size: src.data.len() as u64,
            part: &part,
            fingerprint: None,
            ctl: &ctl,
            free: &plenty,
        };
        // Pause after three chunks (a kill would leave the same .part file behind).
        let mut n = 0;
        let r = fetch(&f, &mut |_| {
            n += 1;
            if n == 4 {
                ctl.request(Stop::Pause);
            }
        })
        .await;
        assert!(matches!(r, Err(FetchError::Stopped(Stop::Pause))), "{r:?}");
        let kept = std::fs::metadata(&part).unwrap().len();
        assert_eq!(kept, 3 * u64::from(CHUNK));
        let before = src.read.load(Ordering::SeqCst);
        let ctl2 = Ctl::new();
        let f2 = Fetch { ctl: &ctl2, ..f };
        fetch(&f2, &mut |_| {}).await.unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), src.data);
        assert_eq!(
            src.read.load(Ordering::SeqCst) - before,
            src.data.len() as u64 - kept
        );
        // Cancelling removes the partial file.
        std::fs::write(&part, b"x").unwrap();
        let ctl3 = Ctl::new();
        ctl3.request(Stop::Cancel);
        let f3 = Fetch { ctl: &ctl3, ..f2 };
        assert!(matches!(
            fetch(&f3, &mut |_| {}).await,
            Err(FetchError::Stopped(Stop::Cancel))
        ));
        assert!(!part.exists());
    }

    #[tokio::test]
    async fn a_full_disk_is_its_own_error_and_leaves_nothing_behind() {
        let d = tempfile::tempdir().unwrap();
        let size = 70 * CHUNK as usize;
        let src = Mem::new(size);
        let part = d.path().join("m.mkv.part");
        let path = VPath::new("/m.mkv");
        let ctl = Ctl::new();
        // Before starting: too little room.
        let small = |_: &Path| Ok(10u64 << 20);
        let f = Fetch {
            src: &*src,
            path: &path,
            size: size as u64,
            part: &part,
            fingerprint: None,
            ctl: &ctl,
            free: &small,
        };
        let e = fetch(&f, &mut |_| {}).await.unwrap_err();
        assert!(matches!(e, FetchError::NoSpace(_)), "{e:?}");
        assert_eq!(e.kind(), ErrorKind::NoSpace);
        assert!(e.message(&part).contains("free"), "{}", e.message(&part));
        assert!(!part.exists());
        // While writing: the drive fills up after the first check.
        let calls = AtomicU64::new(0);
        let shrinking = move |_: &Path| {
            Ok(if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                u64::MAX / 2
            } else {
                1 << 20
            })
        };
        let f = Fetch {
            free: &shrinking,
            ..f
        };
        let e = fetch(&f, &mut |_| {}).await.unwrap_err();
        assert!(matches!(e, FetchError::NoSpace(_)), "{e:?}");
        assert!(!part.exists(), "no partial file after a full disk");
    }

    #[tokio::test]
    async fn a_wrong_fingerprint_fails_the_copy() {
        let d = tempfile::tempdir().unwrap();
        let src = Mem::new(200_000);
        let part = d.path().join("m.mkv.part");
        let path = VPath::new("/m.mkv");
        let ctl = Ctl::new();
        let f = Fetch {
            src: &*src,
            path: &path,
            size: 200_000,
            part: &part,
            fingerprint: Some(b"not it"),
            ctl: &ctl,
            free: &plenty,
        };
        assert!(matches!(
            fetch(&f, &mut |_| {}).await,
            Err(FetchError::Verify(_))
        ));
        assert!(!part.exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn playback_reads_outrank_downloads() {
        let src: Arc<dyn Source> = Mem::slow(64 * CHUNK as usize, 10);
        let gate = Arc::new(Gate::new());
        let bg = Arc::new(Prioritised::background(src.clone(), gate.clone()));
        let play = Prioritised::playback(src.clone(), gate.clone());
        let path = VPath::new("/m.mkv");
        // Two downloads keep the source busy.
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let (bg, path) = (bg.clone(), path.clone());
            tasks.push(tokio::spawn(async move {
                let mut n = 0u64;
                let start = Instant::now();
                while start.elapsed() < Duration::from_millis(1500) {
                    bg.read_at(&path, n, CHUNK).await.unwrap();
                    n += 1;
                }
                n
            }));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        // Playback reads steadily for a second (a 256 KiB read takes 2.5 ms on this source, a
        // download chunk 10 ms). Only the first read may wait behind a chunk already on the wire.
        let mut slow = 0;
        let start = Instant::now();
        let mut off = 0;
        while start.elapsed() < Duration::from_secs(1) {
            let t = Instant::now();
            play.read_at(&path, off, 256 * 1024).await.unwrap();
            if t.elapsed() > Duration::from_millis(9) {
                slow += 1;
            }
            off += 256 * 1024;
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        assert!(slow <= 1, "{slow} playback reads waited behind downloads");
        // Downloads still made progress, and held back while playback read.
        let done: u64 = futures_sum(tasks).await;
        assert!(done > 0);
        assert!(
            done < 100,
            "downloads ran at full speed beside playback: {done}"
        );
    }

    async fn futures_sum(tasks: Vec<tokio::task::JoinHandle<u64>>) -> u64 {
        let mut s = 0;
        for t in tasks {
            s += t.await.unwrap();
        }
        s
    }

    #[test]
    fn names_and_states() {
        assert_eq!(clean("Alien: Director's Cut?"), "Alien Director's Cut");
        assert_eq!(clean("  .. "), "Unknown");
        assert_eq!(clean("a/b\\c"), "a b c");
        let c = Copy {
            state: "running".into(),
            ..Default::default()
        };
        assert_eq!(combined_state("done", &[&c]), "running");
        assert_eq!(combined_state("paused", &[&c]), "paused");
        assert_eq!(combined_state("done", &[]), "done");
        assert!(available_space(Path::new("/definitely/not/here")).unwrap() > 0);
    }
}
