//! FFmpeg's libavformat, libavcodec and libavutil, loaded at runtime. The app already ships them
//! with libmpv (and has them loaded), so nothing links against FFmpeg at build time and no new
//! library is shipped. The types come from `ffi_types.rs` (generated for FFmpeg 8.1); a library
//! with a different major version is refused, since the struct layouts would not match.

use core::ffi::{c_char, c_int, c_uint, c_void};
use std::sync::OnceLock;

use libloading::Library;

pub use crate::ffi_types::*;

/// `AVERROR_EOF`: FFERRTAG('E','O','F',' ').
pub const AVERROR_EOF: c_int =
    -((b'E' as c_int) | ((b'O' as c_int) << 8) | ((b'F' as c_int) << 16) | ((b' ' as c_int) << 24));
/// `AVERROR(EIO)`.
pub const AVERROR_EIO: c_int = -5;

pub type ReadFn = unsafe extern "C" fn(*mut c_void, *mut u8, c_int) -> c_int;
pub type SeekFn = unsafe extern "C" fn(*mut c_void, i64, c_int) -> i64;
type WriteFn = unsafe extern "C" fn(*mut c_void, *const u8, c_int) -> c_int;

/// The FFmpeg functions the probe calls.
pub struct Ffmpeg {
    _libs: Vec<Library>,
    pub avformat_alloc_context: unsafe extern "C" fn() -> *mut AVFormatContext,
    pub avformat_open_input: unsafe extern "C" fn(
        *mut *mut AVFormatContext,
        *const c_char,
        *const AVInputFormat,
        *mut *mut c_void,
    ) -> c_int,
    pub avformat_find_stream_info:
        unsafe extern "C" fn(*mut AVFormatContext, *mut *mut c_void) -> c_int,
    pub avformat_close_input: unsafe extern "C" fn(*mut *mut AVFormatContext),
    pub avio_alloc_context: unsafe extern "C" fn(
        *mut u8,
        c_int,
        c_int,
        *mut c_void,
        Option<ReadFn>,
        Option<WriteFn>,
        Option<SeekFn>,
    ) -> *mut AVIOContext,
    pub avio_context_free: unsafe extern "C" fn(*mut *mut AVIOContext),
    pub av_malloc: unsafe extern "C" fn(usize) -> *mut c_void,
    pub av_freep: unsafe extern "C" fn(*mut c_void),
    pub av_dict_get: unsafe extern "C" fn(
        *const c_void,
        *const c_char,
        *const AVDictionaryEntry,
        c_int,
    ) -> *mut AVDictionaryEntry,
    pub av_dict_set:
        unsafe extern "C" fn(*mut *mut c_void, *const c_char, *const c_char, c_int) -> c_int,
    pub av_dict_free: unsafe extern "C" fn(*mut *mut c_void),
    pub av_pix_fmt_desc_get: unsafe extern "C" fn(c_int) -> *const AVPixFmtDescriptor,
    pub av_channel_layout_describe:
        unsafe extern "C" fn(*const AVChannelLayout, *mut c_char, usize) -> c_int,
    pub av_color_transfer_name: unsafe extern "C" fn(c_int) -> *const c_char,
    pub av_color_primaries_name: unsafe extern "C" fn(c_int) -> *const c_char,
    pub avcodec_get_name: unsafe extern "C" fn(c_int) -> *const c_char,
    pub avcodec_profile_name: unsafe extern "C" fn(c_int, c_int) -> *const c_char,
    pub av_packet_side_data_get:
        unsafe extern "C" fn(*const AVPacketSideData, c_int, c_int) -> *const AVPacketSideData,
}

/// File names of the three libraries for this platform, with the major versions the bindings
/// were generated for.
fn names() -> [String; 3] {
    let (f, c, u) = (
        LIBAVFORMAT_VERSION_MAJOR,
        LIBAVCODEC_VERSION_MAJOR,
        LIBAVUTIL_VERSION_MAJOR,
    );
    if cfg!(windows) {
        [
            format!("avutil-{u}.dll"),
            format!("avcodec-{c}.dll"),
            format!("avformat-{f}.dll"),
        ]
    } else if cfg!(target_os = "macos") {
        [
            format!("libavutil.{u}.dylib"),
            format!("libavcodec.{c}.dylib"),
            format!("libavformat.{f}.dylib"),
        ]
    } else {
        [
            format!("libavutil.so.{u}"),
            format!("libavcodec.so.{c}"),
            format!("libavformat.so.{f}"),
        ]
    }
}

/// Open the libraries: from `COYOTL_FFMPEG_DIR` when set (tests, the CLI), else by name, which
/// finds the copies the app already loaded with libmpv.
fn open(name: &str) -> Result<Library, String> {
    let mut tried = Vec::new();
    if let Some(dir) = std::env::var_os("COYOTL_FFMPEG_DIR") {
        let p = std::path::Path::new(&dir).join(name);
        // SAFETY: loading FFmpeg runs only its (side-effect free) library constructors.
        match unsafe { Library::new(&p) } {
            Ok(l) => return Ok(l),
            Err(e) => tried.push(format!("{}: {e}", p.display())),
        }
    }
    // SAFETY: as above.
    match unsafe { Library::new(name) } {
        Ok(l) => Ok(l),
        Err(e) => {
            tried.push(format!("{name}: {e}"));
            Err(tried.join("; "))
        }
    }
}

macro_rules! sym {
    ($lib:expr, $name:ident) => {{
        // SAFETY: the symbol is declared with the signature of the FFmpeg version the bindings
        // were generated for; `load` checks the major versions first.
        let s: libloading::Symbol<'_, _> =
            unsafe { $lib.get(concat!(stringify!($name), "\0").as_bytes()) }
                .map_err(|e| format!("{}: {e}", stringify!($name)))?;
        *s
    }};
}

/// libswresample, which libavcodec links: loaded first from `COYOTL_FFMPEG_DIR`, where the
/// dynamic loader would not find it on its own. In the app it is already loaded with libmpv.
fn swresample() -> Option<Library> {
    let dir = std::env::var_os("COYOTL_FFMPEG_DIR")?;
    let name = if cfg!(windows) {
        "swresample-6.dll"
    } else if cfg!(target_os = "macos") {
        "libswresample.6.dylib"
    } else {
        "libswresample.so.6"
    };
    // SAFETY: as in `open`.
    unsafe { Library::new(std::path::Path::new(&dir).join(name)) }.ok()
}

fn load() -> Result<Ffmpeg, String> {
    let [u, c, f] = names();
    let avutil = open(&u)?;
    let swr = swresample();
    let (avcodec, avformat) = (open(&c)?, open(&f)?);
    // The bindings match one major version; check each library's own idea of it.
    for (lib, func, want) in [
        (&avformat, "avformat_version", LIBAVFORMAT_VERSION_MAJOR),
        (&avcodec, "avcodec_version", LIBAVCODEC_VERSION_MAJOR),
        (&avutil, "avutil_version", LIBAVUTIL_VERSION_MAJOR),
    ] {
        // SAFETY: `*_version` takes no arguments and returns an unsigned int in every version.
        let v: libloading::Symbol<'_, unsafe extern "C" fn() -> c_uint> =
            unsafe { lib.get(format!("{func}\0").as_bytes()) }.map_err(|e| e.to_string())?;
        // SAFETY: as above.
        let major = unsafe { v() } >> 16;
        if major != want {
            return Err(format!(
                "{func} reports major {major}, the probe needs {want}"
            ));
        }
    }
    let ff = Ffmpeg {
        avformat_alloc_context: sym!(avformat, avformat_alloc_context),
        avformat_open_input: sym!(avformat, avformat_open_input),
        avformat_find_stream_info: sym!(avformat, avformat_find_stream_info),
        avformat_close_input: sym!(avformat, avformat_close_input),
        avio_alloc_context: sym!(avformat, avio_alloc_context),
        avio_context_free: sym!(avformat, avio_context_free),
        av_malloc: sym!(avutil, av_malloc),
        av_freep: sym!(avutil, av_freep),
        av_dict_get: sym!(avutil, av_dict_get),
        av_dict_set: sym!(avutil, av_dict_set),
        av_dict_free: sym!(avutil, av_dict_free),
        av_pix_fmt_desc_get: sym!(avutil, av_pix_fmt_desc_get),
        av_channel_layout_describe: sym!(avutil, av_channel_layout_describe),
        av_color_transfer_name: sym!(avutil, av_color_transfer_name),
        av_color_primaries_name: sym!(avutil, av_color_primaries_name),
        avcodec_get_name: sym!(avcodec, avcodec_get_name),
        avcodec_profile_name: sym!(avcodec, avcodec_profile_name),
        av_packet_side_data_get: sym!(avcodec, av_packet_side_data_get),
        _libs: [avformat, avcodec, avutil].into_iter().chain(swr).collect(),
    };
    Ok(ff)
}

/// The loaded libraries, or why they could not be loaded (then probing is unavailable and the
/// app relies on what mpv reports during playback).
pub fn ffmpeg() -> Result<&'static Ffmpeg, &'static str> {
    static FF: OnceLock<Result<Ffmpeg, String>> = OnceLock::new();
    FF.get_or_init(load).as_ref().map_err(String::as_str)
}
