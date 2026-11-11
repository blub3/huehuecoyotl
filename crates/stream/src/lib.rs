//! mpv stream callbacks over any [`Source`] (SPEC 5.3, 5.4).
//!
//! The Qt shell registers the protocol once:
//!   `mpv_stream_cb_add_ro(mpv, "coyotl", nullptr, coyotl_stream_open)`
//! and mpv's demuxer threads then call into this crate directly. URLs are
//! `coyotl://<source id>/<percent-encoded path inside the source>`.
//!
//! The core installs a resolver that maps a source id to a connected [`Source`].
//!
//! What this layer adds on top of mpv's own demuxer cache (SPEC 5.4):
//! - adaptive read-ahead: a seek starts with a small read, which grows while reads stay
//!   sequential, plus one chunk prefetched in the background;
//! - retry with exponential backoff, so a short network drop becomes a pause, not an error.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use bytes::Bytes;
use huehuecoyotl_vfs::{Source, VPath, VfsError};
use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

// ---- mpv/stream_cb.h mirror (this crate never links libmpv) ----------------------------------

pub const MPV_ERROR_LOADING_FAILED: c_int = -13;
pub const MPV_ERROR_GENERIC: i64 = -20;

pub type ReadFn = unsafe extern "C" fn(*mut c_void, *mut c_char, u64) -> i64;
pub type SeekFn = unsafe extern "C" fn(*mut c_void, i64) -> i64;
pub type SizeFn = unsafe extern "C" fn(*mut c_void) -> i64;
pub type CloseFn = unsafe extern "C" fn(*mut c_void);
pub type CancelFn = unsafe extern "C" fn(*mut c_void);

#[repr(C)]
pub struct MpvStreamCbInfo {
    pub cookie: *mut c_void,
    pub read_fn: Option<ReadFn>,
    pub seek_fn: Option<SeekFn>,
    pub size_fn: Option<SizeFn>,
    pub close_fn: Option<CloseFn>,
    pub cancel_fn: Option<CancelFn>,
}

// ---- URLs ---------------------------------------------------------------------------------

/// Characters escaped in the path part of a `coyotl://` URL. '/' stays readable.
const PATH: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'%')
    .add(b'#')
    .add(b'?')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'\\');

pub fn url(source_id: i64, path: &VPath) -> String {
    format!(
        "coyotl://{source_id}{}",
        utf8_percent_encode(path.as_str(), PATH)
    )
}

pub fn parse_url(url: &str) -> Option<(i64, VPath)> {
    let rest = url.strip_prefix("coyotl://")?;
    let (id, path) = rest.split_once('/')?;
    let path = percent_decode_str(path).decode_utf8().ok()?;
    Some((id.parse().ok()?, VPath::new(format!("/{path}"))))
}

// ---- resolver and runtime -----------------------------------------------------------------

pub type Resolver = dyn Fn(i64) -> Result<Arc<dyn Source>, VfsError> + Send + Sync;

fn resolver() -> &'static RwLock<Option<Arc<Resolver>>> {
    static R: OnceLock<RwLock<Option<Arc<Resolver>>>> = OnceLock::new();
    R.get_or_init(|| RwLock::new(None))
}

/// Install the function that turns a source id into a connected source. Called by the core;
/// it may block (it runs on mpv's threads, never on the GUI thread).
pub fn set_resolver(r: Arc<Resolver>) {
    *resolver().write().unwrap() = Some(r);
}

fn rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("coyotl-stream")
            .enable_all()
            .build()
            .expect("stream runtime")
    })
}

/// Run a future to completion from a non-async thread (mpv's demuxer, tests).
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    rt().block_on(f)
}

// ---- the stream ---------------------------------------------------------------------------

const FIRST_CHUNK: u32 = 256 * 1024;
const MAX_CHUNK: u32 = 8 << 20;
const RETRIES: u32 = 7; // 0.25 s .. 16 s of backoff, about 32 s in total

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub reads: u64,
    pub bytes: u64,
    pub fetches: u64,
    pub prefetch_hits: u64,
    pub seeks: u64,
    pub retries: u64,
}

pub struct Stream {
    src: Arc<dyn Source>,
    path: VPath,
    size: u64,
    pos: u64,
    buf_off: u64,
    buf: Bytes,
    chunk: u32,
    prefetch: Option<(u64, JoinHandle<Result<Bytes, VfsError>>)>,
    cancelled: Arc<AtomicBool>,
    pub stats: Stats,
}

impl Stream {
    pub fn open(source_id: i64, path: &VPath) -> Result<Self, VfsError> {
        let r = resolver()
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| VfsError::Internal("no source resolver".into()))?;
        let src = r(source_id)?;
        let size = block_on(src.stat(path))?.size;
        Ok(Self::new(src, path.clone(), size))
    }

    pub fn new(src: Arc<dyn Source>, path: VPath, size: u64) -> Self {
        Self {
            src,
            path,
            size,
            pos: 0,
            buf_off: 0,
            buf: Bytes::new(),
            chunk: FIRST_CHUNK,
            prefetch: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            stats: Stats::default(),
        }
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    fn start_prefetch(&mut self, off: u64) {
        if off >= self.size {
            self.prefetch = None;
            return;
        }
        let (src, path, len) = (self.src.clone(), self.path.clone(), self.chunk);
        self.prefetch = Some((
            off,
            rt().spawn(async move { src.read_at(&path, off, len).await }),
        ));
    }

    /// Fetch the chunk at `pos`, retrying transport errors with backoff.
    fn fill(&mut self) -> Result<(), VfsError> {
        let pos = self.pos;
        if let Some((off, h)) = self.prefetch.take() {
            if off == pos {
                if let Ok(Ok(b)) = block_on(h) {
                    self.stats.prefetch_hits += 1;
                    self.buf_off = off;
                    self.buf = b;
                    return Ok(());
                }
            } else {
                h.abort();
            }
        }
        let mut delay = Duration::from_millis(250);
        let mut attempt = 0;
        loop {
            if self.cancelled.load(Ordering::Relaxed) {
                return Err(VfsError::Internal("cancelled".into()));
            }
            self.stats.fetches += 1;
            match block_on(self.src.read_at(&self.path, pos, self.chunk)) {
                Ok(b) => {
                    self.buf_off = pos;
                    self.buf = b;
                    return Ok(());
                }
                Err(e @ (VfsError::Unreachable(_) | VfsError::Internal(_)))
                    if attempt < RETRIES =>
                {
                    tracing::warn!(path = %self.path, pos, attempt, "read failed, retrying in {delay:?}: {e}");
                    self.stats.retries += 1;
                    std::thread::sleep(delay);
                    delay = (delay * 2).min(Duration::from_secs(16));
                    attempt += 1;
                }
                Err(e) => return Err(e),
            }
        }
    }

    pub fn read(&mut self, out: &mut [u8]) -> Result<usize, VfsError> {
        self.stats.reads += 1;
        if self.pos >= self.size || out.is_empty() {
            return Ok(0);
        }
        let in_buf = self.pos >= self.buf_off && self.pos < self.buf_off + self.buf.len() as u64;
        if !in_buf {
            // Sequential continuation: read further ahead next time.
            if self.pos == self.buf_off + self.buf.len() as u64 && !self.buf.is_empty() {
                self.chunk = (self.chunk * 2).min(MAX_CHUNK);
            }
            self.fill()?;
            if self.buf.is_empty() {
                return Ok(0); // file shrank
            }
            let next = self.buf_off + self.buf.len() as u64;
            self.start_prefetch(next);
        }
        let i = (self.pos - self.buf_off) as usize;
        let n = out.len().min(self.buf.len() - i);
        out[..n].copy_from_slice(&self.buf[i..i + n]);
        self.pos += n as u64;
        self.stats.bytes += n as u64;
        Ok(n)
    }

    pub fn seek(&mut self, to: u64) -> Result<u64, VfsError> {
        if to > self.size {
            return Err(VfsError::Internal("seek past end".into()));
        }
        if to != self.pos {
            self.stats.seeks += 1;
            let in_buf = to >= self.buf_off && to < self.buf_off + self.buf.len() as u64;
            if !in_buf {
                // A real seek: start small again (index probes, scrubbing).
                self.chunk = FIRST_CHUNK;
            }
        }
        self.pos = to;
        Ok(to)
    }

    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Some((_, h)) = self.prefetch.take() {
            h.abort();
        }
    }
}

// ---- extern "C" surface (what mpv calls) ----------------------------------------------------

struct Cookie {
    stream: Stream,
    cancelled: Arc<AtomicBool>,
}

fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    // Backends may need the runtime context even to drop handles; mpv's threads are not ours.
    let _rt = rt().enter();
    // SPEC 8, rule 7: no panic may unwind into C.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(fallback)
}

/// mpv_stream_cb_open_ro_fn.
///
/// # Safety
/// Called by libmpv with a valid NUL-terminated `uri` and a writable `info`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn coyotl_stream_open(
    _user: *mut c_void,
    uri: *mut c_char,
    info: *mut MpvStreamCbInfo,
) -> c_int {
    guard(MPV_ERROR_LOADING_FAILED, || {
        let uri = unsafe { CStr::from_ptr(uri) }
            .to_string_lossy()
            .into_owned();
        let Some((sid, path)) = parse_url(&uri) else {
            tracing::error!(%uri, "bad stream URL");
            return MPV_ERROR_LOADING_FAILED;
        };
        let stream = match Stream::open(sid, &path) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(%uri, "cannot open stream: {e}");
                return MPV_ERROR_LOADING_FAILED;
            }
        };
        let cancelled = stream.cancel_flag();
        let cookie = Box::new(Cookie { stream, cancelled });
        unsafe {
            *info = MpvStreamCbInfo {
                cookie: Box::into_raw(cookie).cast(),
                read_fn: Some(read_cb),
                seek_fn: Some(seek_cb),
                size_fn: Some(size_cb),
                close_fn: Some(close_cb),
                cancel_fn: Some(cancel_cb),
            };
        }
        0
    })
}

unsafe fn cookie<'a>(c: *mut c_void) -> &'a mut Cookie {
    unsafe { &mut *c.cast::<Cookie>() }
}

unsafe extern "C" fn read_cb(c: *mut c_void, buf: *mut c_char, n: u64) -> i64 {
    guard(-1, || {
        let ck = unsafe { cookie(c) };
        let out = unsafe { std::slice::from_raw_parts_mut(buf.cast::<u8>(), n as usize) };
        match ck.stream.read(out) {
            Ok(n) => n as i64,
            Err(e) => {
                tracing::error!(path = %ck.stream.path, "stream read failed: {e}");
                -1
            }
        }
    })
}

unsafe extern "C" fn seek_cb(c: *mut c_void, off: i64) -> i64 {
    guard(MPV_ERROR_GENERIC, || {
        if off < 0 {
            return MPV_ERROR_GENERIC;
        }
        match unsafe { cookie(c) }.stream.seek(off as u64) {
            Ok(p) => p as i64,
            Err(_) => MPV_ERROR_GENERIC,
        }
    })
}

unsafe extern "C" fn size_cb(c: *mut c_void) -> i64 {
    guard(MPV_ERROR_GENERIC, || {
        unsafe { cookie(c) }.stream.size() as i64
    })
}

unsafe extern "C" fn close_cb(c: *mut c_void) {
    guard((), || {
        let ck = unsafe { Box::from_raw(c.cast::<Cookie>()) };
        tracing::debug!(path = %ck.stream.path, stats = ?ck.stream.stats, "stream closed");
    })
}

unsafe extern "C" fn cancel_cb(c: *mut c_void) {
    guard((), || {
        unsafe { cookie(c) }
            .cancelled
            .store(true, Ordering::Relaxed)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use huehuecoyotl_vfs::{Caps, Entry};
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU32;

    /// In-memory source that can fail a number of times and records read sizes.
    struct Mem {
        data: Vec<u8>,
        fail_next: AtomicU32,
        lens: Mutex<Vec<u32>>,
    }

    #[async_trait]
    impl Source for Mem {
        async fn list(&self, _d: &VPath) -> huehuecoyotl_vfs::Result<Vec<Entry>> {
            Ok(vec![])
        }
        async fn stat(&self, p: &VPath) -> huehuecoyotl_vfs::Result<Entry> {
            Ok(Entry {
                path: p.clone(),
                name: p.file_name().into(),
                is_dir: false,
                size: self.data.len() as u64,
                mtime: 0,
            })
        }
        async fn read_at(&self, _p: &VPath, off: u64, len: u32) -> huehuecoyotl_vfs::Result<Bytes> {
            if self.fail_next.load(Ordering::SeqCst) > 0 {
                self.fail_next.fetch_sub(1, Ordering::SeqCst);
                return Err(VfsError::Unreachable("cable pulled".into()));
            }
            self.lens.lock().unwrap().push(len);
            let s = (off as usize).min(self.data.len());
            let e = (s + len as usize).min(self.data.len());
            Ok(Bytes::copy_from_slice(&self.data[s..e]))
        }
        fn capabilities(&self) -> Caps {
            Caps::default()
        }
    }

    fn mem(size: usize) -> Arc<Mem> {
        Arc::new(Mem {
            data: (0..size).map(|i| (i * 31 % 251) as u8).collect(),
            fail_next: AtomicU32::new(0),
            lens: Mutex::new(vec![]),
        })
    }

    #[test]
    fn urls_round_trip() {
        let p = VPath::new("/Movies/Alien (1979)/50% #1 ?.mkv");
        let u = url(7, &p);
        assert_eq!(u, "coyotl://7/Movies/Alien%20(1979)/50%25%20%231%20%3F.mkv");
        assert_eq!(parse_url(&u), Some((7, p)));
        assert_eq!(parse_url("http://x"), None);
    }

    #[test]
    fn sequential_reads_grow_and_seeks_shrink() {
        let m = mem(40 << 20);
        let mut s = Stream::new(m.clone(), VPath::new("/f"), m.data.len() as u64);
        let mut buf = vec![0u8; 1 << 20];
        let mut got = Vec::new();
        for _ in 0..12 {
            let n = s.read(&mut buf).unwrap();
            got.extend_from_slice(&buf[..n]);
        }
        assert!(got[..] == m.data[..got.len()], "bytes match");
        let lens = m.lens.lock().unwrap().clone();
        assert_eq!(lens[0], FIRST_CHUNK, "starts small");
        assert!(
            lens.contains(&MAX_CHUNK),
            "grows while sequential: {lens:?}"
        );
        assert!(s.stats.prefetch_hits > 0);
        // A real seek starts small again.
        s.seek(30 << 20).unwrap();
        s.read(&mut buf[..1000]).unwrap();
        assert_eq!(*m.lens.lock().unwrap().last().unwrap(), FIRST_CHUNK);
        assert!(buf[..1000] == m.data[30 << 20..(30 << 20) + 1000]);
    }

    #[test]
    fn short_outage_becomes_a_pause() {
        let m = mem(1 << 20);
        m.fail_next.store(3, Ordering::SeqCst);
        let mut s = Stream::new(m.clone(), VPath::new("/f"), m.data.len() as u64);
        let mut buf = vec![0u8; 4096];
        let t = std::time::Instant::now();
        assert_eq!(s.read(&mut buf).unwrap(), 4096);
        assert!(buf[..] == m.data[..4096]);
        assert_eq!(s.stats.retries, 3);
        assert!(t.elapsed() >= Duration::from_millis(250 + 500 + 1000));
    }

    #[test]
    fn resolver_and_callbacks() {
        let m = mem(300_000);
        let src: Arc<dyn Source> = m.clone();
        set_resolver(Arc::new(move |id| {
            if id == 3 {
                Ok(src.clone())
            } else {
                Err(VfsError::NotFound(format!("source {id}")))
            }
        }));
        let uri = std::ffi::CString::new(url(3, &VPath::new("/a b.mkv"))).unwrap();
        let mut info = MpvStreamCbInfo {
            cookie: std::ptr::null_mut(),
            read_fn: None,
            seek_fn: None,
            size_fn: None,
            close_fn: None,
            cancel_fn: None,
        };
        assert_eq!(
            unsafe { coyotl_stream_open(std::ptr::null_mut(), uri.as_ptr() as *mut _, &mut info) },
            0
        );
        let (read, seek, size, close) = (
            info.read_fn.unwrap(),
            info.seek_fn.unwrap(),
            info.size_fn.unwrap(),
            info.close_fn.unwrap(),
        );
        assert_eq!(unsafe { size(info.cookie) }, 300_000);
        assert_eq!(unsafe { seek(info.cookie, 299_990) }, 299_990);
        let mut b = [0u8; 64];
        assert_eq!(unsafe { read(info.cookie, b.as_mut_ptr().cast(), 64) }, 10);
        assert_eq!(
            unsafe { read(info.cookie, b.as_mut_ptr().cast(), 64) },
            0,
            "EOF"
        );
        unsafe { close(info.cookie) };
        let bad = std::ffi::CString::new(url(9, &VPath::new("/x"))).unwrap();
        assert_eq!(
            unsafe { coyotl_stream_open(std::ptr::null_mut(), bad.as_ptr() as *mut _, &mut info) },
            MPV_ERROR_LOADING_FAILED
        );
    }
}
