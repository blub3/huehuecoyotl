//! M0 spike, prototype 3: serve SMB files to libmpv through its stream callback API.
//!
//! The C++ side registers the protocol with
//!   `mpv_stream_cb_add_ro(mpv, "coyotl", nullptr, coyotl_stream_open)`
//! and mpv's demuxer threads then call the functions below directly. This crate
//! never links libmpv: the `mpv_stream_cb_info` layout is mirrored here.
//!
//! URI form for the spike: `coyotl://<host>[:port]/<share>/<path...>`.
//! Credentials come from COYOTL_SMB_USER / COYOTL_SMB_PASS (the product uses the keystore).
//!
//! What the spike implements from SPEC 5.4: sequential read-ahead in chunks of a few MiB
//! (fetched as parallel 1 MiB SMB reads), and transparent reconnect with retry. The
//! index-region block cache is left out.

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use smb::{Client, ClientConfig, FileAccessMask, FileCreateArgs, GetLen, UncPath};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

// ---- mpv/stream_cb.h mirror -------------------------------------------------------------

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

// ---- runtime and connection pool --------------------------------------------------------

const SMB_READ: usize = 1 << 20; // one SMB READ request
const CHUNK: usize = 4 << 20; // read-ahead unit, fetched as CHUNK/SMB_READ parallel requests
const RETRIES: u32 = 6; // ~0.2+0.4+0.8+1.6+3.2+6.4 s of backoff before giving up

fn rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("coyotl-io")
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

type Pool = Mutex<HashMap<String, Arc<Client>>>;
fn pool() -> &'static Pool {
    static P: OnceLock<Pool> = OnceLock::new();
    P.get_or_init(Default::default)
}

#[derive(Clone, Debug)]
struct Target {
    host: String,
    port: Option<u16>,
    share: String,
    path: String,
}

impl Target {
    fn parse(uri: &str) -> Option<Self> {
        let rest = uri.strip_prefix("coyotl://")?;
        let mut it = rest.splitn(3, '/');
        let hostport = it.next()?;
        let share = it.next()?.to_string();
        let path = it.next()?.replace('/', "\\");
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), Some(p.parse().ok()?)),
            None => (hostport.to_string(), None),
        };
        Some(Self { host, port, share, path })
    }
    fn key(&self) -> String {
        format!("{}:{:?}/{}", self.host, self.port, self.share)
    }
    fn unc(&self) -> UncPath {
        UncPath::from_str(&format!(r"\\{}\{}", self.host, self.share)).expect("unc")
    }
}

async fn client_for(t: &Target, fresh: bool) -> smb::Result<Arc<Client>> {
    let key = t.key();
    if !fresh {
        if let Some(c) = pool().lock().unwrap().get(&key) {
            return Ok(c.clone());
        }
    }
    let mut cfg = ClientConfig::default();
    cfg.dfs = false;
    cfg.connection.port = t.port;
    cfg.connection.smb2_only_negotiate = true;
    let client = Arc::new(Client::new(cfg));
    let user = std::env::var("COYOTL_SMB_USER").unwrap_or_else(|_| "guest".into());
    let pass = std::env::var("COYOTL_SMB_PASS").unwrap_or_default();
    client.share_connect(&t.unc(), &user, pass).await?;
    pool().lock().unwrap().insert(key, client.clone());
    Ok(client)
}

async fn open_file(t: &Target, fresh: bool) -> smb::Result<(Arc<smb::File>, u64)> {
    let client = client_for(t, fresh).await?;
    let args = FileCreateArgs::make_open_existing(FileAccessMask::new().with_generic_read(true));
    let file = client.create_file(&t.unc().with_path(&t.path), &args).await?.unwrap_file();
    let len = file.get_len().await?;
    Ok((Arc::new(file), len))
}

/// Read `len` bytes at `off` as parallel SMB_READ-sized requests.
async fn fetch(file: Arc<smb::File>, off: u64, len: usize) -> std::io::Result<Vec<u8>> {
    let mut parts = Vec::new();
    let mut o = 0;
    while o < len {
        let n = SMB_READ.min(len - o);
        let f = file.clone();
        let at = off + o as u64;
        parts.push(tokio::spawn(async move {
            let mut buf = vec![0u8; n];
            let mut got = 0;
            while got < n {
                let r = f.read_block(&mut buf[got..], at + got as u64, None, false).await?;
                if r == 0 {
                    break;
                }
                got += r;
            }
            buf.truncate(got);
            Ok::<_, std::io::Error>(buf)
        }));
        o += n;
    }
    let mut out = Vec::with_capacity(len);
    for p in parts {
        let b = p.await.map_err(std::io::Error::other)??;
        let short = b.len() < SMB_READ;
        out.extend_from_slice(&b);
        if short {
            break;
        }
    }
    Ok(out)
}

// ---- per-open stream state --------------------------------------------------------------

struct Stream {
    target: Target,
    file: Arc<smb::File>,
    size: u64,
    pos: u64,
    buf_off: u64,
    buf: Vec<u8>,
    prefetch: Option<(u64, JoinHandle<std::io::Result<Vec<u8>>>)>,
    cancelled: Arc<AtomicBool>,
    pub stats: Stats,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Stats {
    pub reads: u64,
    pub bytes: u64,
    pub prefetch_hits: u64,
    pub sync_fetches: u64,
    pub seeks: u64,
    pub reconnects: u64,
}

impl Stream {
    fn start_prefetch(&mut self, off: u64) {
        if off >= self.size {
            self.prefetch = None;
            return;
        }
        let len = CHUNK.min((self.size - off) as usize);
        let f = self.file.clone();
        self.prefetch = Some((off, rt().spawn(fetch(f, off, len))));
    }

    /// Load the chunk containing `self.pos` into `buf`, with retry and reconnect.
    fn fill(&mut self) -> std::io::Result<()> {
        let pos = self.pos;
        if let Some((off, h)) = self.prefetch.take() {
            if off == pos {
                if let Ok(Ok(b)) = rt().block_on(h) {
                    self.stats.prefetch_hits += 1;
                    self.buf_off = off;
                    self.buf = b;
                    return Ok(());
                }
            } else {
                h.abort();
            }
        }
        self.stats.sync_fetches += 1;
        let len = CHUNK.min((self.size - pos) as usize);
        let mut delay = Duration::from_millis(200);
        for attempt in 0..=RETRIES {
            if self.cancelled.load(Ordering::Relaxed) {
                return Err(std::io::Error::other("cancelled"));
            }
            match rt().block_on(fetch(self.file.clone(), pos, len)) {
                Ok(b) => {
                    self.buf_off = pos;
                    self.buf = b;
                    return Ok(());
                }
                Err(e) if attempt < RETRIES => {
                    eprintln!("[coyotl-stream] read at {pos} failed ({e}); reconnect in {delay:?}");
                    std::thread::sleep(delay);
                    delay *= 2;
                    if let Ok((f, _)) = rt().block_on(open_file(&self.target, true)) {
                        self.file = f;
                        self.stats.reconnects += 1;
                    }
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!()
    }

    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.stats.reads += 1;
        if self.pos >= self.size || out.is_empty() {
            return Ok(0);
        }
        let in_buf = self.pos >= self.buf_off && self.pos < self.buf_off + self.buf.len() as u64;
        if !in_buf {
            self.fill()?;
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
}

// ---- extern "C" surface (what mpv calls) ------------------------------------------------

fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    // mpv's threads are not tokio threads; smb handles need the runtime context even to drop.
    let _rt = rt().enter();
    // SPEC 8, rule 7: no panic may unwind into C.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(fallback)
}

/// mpv_stream_cb_open_ro_fn
///
/// # Safety
/// Called by libmpv with a valid NUL-terminated `uri` and writable `info`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn coyotl_stream_open(
    _user: *mut c_void,
    uri: *mut c_char,
    info: *mut MpvStreamCbInfo,
) -> c_int {
    guard(MPV_ERROR_LOADING_FAILED, || {
        let uri = unsafe { CStr::from_ptr(uri) }.to_string_lossy().into_owned();
        let Some(target) = Target::parse(&uri) else {
            eprintln!("[coyotl-stream] bad uri {uri}");
            return MPV_ERROR_LOADING_FAILED;
        };
        let (file, size) = match rt().block_on(open_file(&target, false)) {
            Ok(v) => v,
            Err(_) => match rt().block_on(open_file(&target, true)) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[coyotl-stream] open {uri}: {e}");
                    return MPV_ERROR_LOADING_FAILED;
                }
            },
        };
        let mut s = Box::new(Stream {
            target,
            file,
            size,
            pos: 0,
            buf_off: 0,
            buf: Vec::new(),
            prefetch: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            stats: Stats::default(),
        });
        s.start_prefetch(0);
        unsafe {
            *info = MpvStreamCbInfo {
                cookie: Box::into_raw(s).cast(),
                read_fn: Some(coyotl_stream_read),
                seek_fn: Some(coyotl_stream_seek),
                size_fn: Some(coyotl_stream_size),
                close_fn: Some(coyotl_stream_close),
                cancel_fn: Some(coyotl_stream_cancel),
            };
        }
        0
    })
}

unsafe fn stream<'a>(cookie: *mut c_void) -> &'a mut Stream {
    unsafe { &mut *cookie.cast::<Stream>() }
}

/// # Safety
/// `cookie` comes from `coyotl_stream_open`; `buf` has room for `n` bytes.
pub unsafe extern "C" fn coyotl_stream_read(cookie: *mut c_void, buf: *mut c_char, n: u64) -> i64 {
    guard(-1, || {
        let s = unsafe { stream(cookie) };
        let out = unsafe { std::slice::from_raw_parts_mut(buf.cast::<u8>(), n as usize) };
        match s.read(out) {
            Ok(n) => n as i64,
            Err(e) => {
                eprintln!("[coyotl-stream] read failed: {e}");
                -1
            }
        }
    })
}

/// # Safety
/// `cookie` comes from `coyotl_stream_open`.
pub unsafe extern "C" fn coyotl_stream_seek(cookie: *mut c_void, off: i64) -> i64 {
    guard(MPV_ERROR_GENERIC, || {
        let s = unsafe { stream(cookie) };
        if off < 0 || off as u64 > s.size {
            return MPV_ERROR_GENERIC;
        }
        s.stats.seeks += 1;
        s.pos = off as u64;
        off
    })
}

/// # Safety
/// `cookie` comes from `coyotl_stream_open`.
pub unsafe extern "C" fn coyotl_stream_size(cookie: *mut c_void) -> i64 {
    guard(MPV_ERROR_GENERIC, || unsafe { stream(cookie) }.size as i64)
}

/// # Safety
/// `cookie` comes from `coyotl_stream_open` and is not used afterwards.
pub unsafe extern "C" fn coyotl_stream_close(cookie: *mut c_void) {
    guard((), || {
        let s = unsafe { Box::from_raw(cookie.cast::<Stream>()) };
        if let Some((_, h)) = &s.prefetch {
            h.abort();
        }
        eprintln!("[coyotl-stream] close {:?} {:?}", s.target.path, s.stats);
    })
}

/// # Safety
/// `cookie` comes from `coyotl_stream_open`. May be called from any thread.
pub unsafe extern "C" fn coyotl_stream_cancel(cookie: *mut c_void) {
    guard((), || {
        let s = unsafe { &*cookie.cast::<Stream>() };
        s.cancelled.store(true, Ordering::Relaxed);
    })
}

/// Test hook for the harness: stats of an open stream.
///
/// # Safety
/// `cookie` comes from `coyotl_stream_open` and is still open.
pub unsafe fn stats(cookie: *mut c_void) -> Stats {
    unsafe { stream(cookie) }.stats
}
