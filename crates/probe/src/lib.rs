//! Media probe (issue #33): container, video, audio and subtitle properties of a file, read with
//! FFmpeg's libavformat through a caller-supplied reader, so files on SMB, NFS, WebDAV, FTP and
//! SFTP are probed like local ones. Only the start of a file is read (and whatever the container
//! points at, such as Matroska's tags), never the whole file; a byte budget caps it.
//!
//! The result is stored as `file.probe_json` ([`ProbeInfo`], with a schema version).

mod ffi;
#[rustfmt::skip]
mod ffi_types;

use core::ffi::{CStr, c_char, c_int, c_void};
use std::io;

use serde::{Deserialize, Serialize};

pub use ffi::ffmpeg;

/// Bump when fields are added or their meaning changes; files with an older version are probed
/// again in the background.
pub const SCHEMA: u32 = 1;

/// At most this many bytes are read from one file (the probe usually needs far less).
pub const READ_BUDGET: u64 = 24 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProbeInfo {
    pub schema: u32,
    /// "ffmpeg", or "mpv" when it comes from playback.
    pub source: String,
    /// "Matroska", "MP4", "MPEG-TS", "AVI", ...
    pub container: String,
    pub duration_ms: u64,
    pub size: u64,
    /// Overall bit rate in bit/s.
    pub bitrate: Option<Bitrate>,
    pub chapters: u32,
    /// Issue #65 (audiobooks): the chapters with their times and titles. Empty in data written
    /// before 2.5; the schema stays 1, since nothing else changed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chapter_list: Vec<Chapter>,
    pub video: Vec<Video>,
    pub audio: Vec<Audio>,
    pub subtitles: Vec<Subtitle>,
    /// Why the file could not be analysed (then it is not tried again until it changes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A chapter of the file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    pub title: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// A bit rate in bit/s; `estimated` when it was derived from size and duration rather than
/// read from the stream or the container's statistics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bitrate {
    pub bps: u64,
    pub estimated: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Video {
    /// FFmpeg codec name: "hevc", "h264", "av1", ...
    pub codec: String,
    /// "Main 10", "High", ...
    pub profile: Option<String>,
    /// "5.1", "4.1"
    pub level: Option<String>,
    pub width: u32,
    pub height: u32,
    /// Display aspect ratio, e.g. "16:9", "2.39:1".
    pub dar: Option<String>,
    pub fps: Option<f64>,
    /// "progressive" or "interlaced"; None when unknown.
    pub scan: Option<String>,
    pub bit_depth: Option<u32>,
    /// "4:2:0", "4:2:2", "4:4:4"
    pub chroma: Option<String>,
    /// "Dolby Vision", "HDR10", "HLG"; None for SDR.
    pub hdr: Option<String>,
    /// Dolby Vision profile (5, 7, 8) and its base-layer compatibility (e.g. "8.1").
    pub dv_profile: Option<String>,
    pub primaries: Option<String>,
    pub transfer: Option<String>,
    pub bitrate: Option<Bitrate>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Audio {
    pub language: Option<String>,
    pub title: Option<String>,
    pub codec: String,
    /// FFmpeg's profile name when it signals more than the codec ("DTS-HD MA + DTS:X",
    /// "Dolby TrueHD + Dolby Atmos", "Dolby Digital Plus + Dolby Atmos").
    pub profile: Option<String>,
    /// "7.1", "5.1", "stereo"
    pub layout: Option<String>,
    pub channels: u32,
    pub sample_rate: u32,
    pub bit_depth: Option<u32>,
    pub bitrate: Option<Bitrate>,
    pub default: bool,
    pub forced: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Subtitle {
    pub language: Option<String>,
    pub title: Option<String>,
    /// "SRT", "ASS", "PGS", "VobSub", "WebVTT", "mov_text", ...
    pub format: String,
    pub external: bool,
    pub default: bool,
    pub forced: bool,
    pub sdh: bool,
}

/// Random-access reads from the file being probed.
pub trait ReadAt {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize>;
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("FFmpeg is not available for probing: {0}")]
    Unavailable(&'static str),
    #[error("not a media file FFmpeg can read ({0})")]
    Unreadable(i32),
    #[error("reading the file failed: {0}")]
    Io(String),
}

/// What the probe read: the result and how many bytes it took.
pub struct Probed {
    pub info: ProbeInfo,
    pub bytes_read: u64,
}

struct IoState<'a> {
    reader: &'a mut dyn ReadAt,
    pos: u64,
    size: u64,
    read: u64,
    budget: u64,
    error: Option<String>,
}

unsafe extern "C" fn read_cb(opaque: *mut c_void, buf: *mut u8, len: c_int) -> c_int {
    // SAFETY: `opaque` is the `IoState` that `probe` keeps alive for the AVIOContext's lifetime.
    let st = unsafe { &mut *(opaque as *mut IoState<'_>) };
    let left = st.budget.saturating_sub(st.read);
    let want = (len.max(0) as u64)
        .min(left)
        .min(st.size.saturating_sub(st.pos));
    if want == 0 {
        return ffi::AVERROR_EOF;
    }
    // SAFETY: FFmpeg hands a buffer of at least `len` bytes; `want <= len`.
    let out = unsafe { std::slice::from_raw_parts_mut(buf, want as usize) };
    match st.reader.read_at(st.pos, out) {
        Ok(0) => ffi::AVERROR_EOF,
        Ok(n) => {
            st.pos += n as u64;
            st.read += n as u64;
            n as c_int
        }
        Err(e) => {
            st.error = Some(e.to_string());
            ffi::AVERROR_EIO
        }
    }
}

unsafe extern "C" fn seek_cb(opaque: *mut c_void, offset: i64, whence: c_int) -> i64 {
    // SAFETY: as in `read_cb`.
    let st = unsafe { &mut *(opaque as *mut IoState<'_>) };
    if whence & ffi::AVSEEK_SIZE as c_int != 0 {
        return st.size as i64;
    }
    let base = match whence & 3 {
        0 => 0,
        1 => st.pos as i64,
        2 => st.size as i64,
        _ => return -1,
    };
    let target = base + offset;
    if target < 0 {
        return -1;
    }
    st.pos = target as u64;
    target
}

/// Probe a file of `size` bytes named `name` (for the container label) through `reader`.
pub fn probe(reader: &mut dyn ReadAt, size: u64, name: &str) -> Result<Probed, ProbeError> {
    probe_with_budget(reader, size, name, READ_BUDGET)
}

pub fn probe_with_budget(
    reader: &mut dyn ReadAt,
    size: u64,
    name: &str,
    budget: u64,
) -> Result<Probed, ProbeError> {
    let ff = ffmpeg().map_err(ProbeError::Unavailable)?;
    let mut st = IoState {
        reader,
        pos: 0,
        size,
        read: 0,
        budget,
        error: None,
    };
    const BUF: usize = 64 * 1024;
    // SAFETY: the FFmpeg calls follow the documented custom-I/O pattern: an av_malloc'ed buffer
    // owned by the AVIOContext, the context attached to an AVFormatContext with
    // AVFMT_FLAG_CUSTOM_IO, and everything freed in reverse order below, also on errors.
    unsafe {
        let buf = (ff.av_malloc)(BUF) as *mut u8;
        let mut avio = (ff.avio_alloc_context)(
            buf,
            BUF as c_int,
            0,
            &mut st as *mut IoState<'_> as *mut c_void,
            Some(read_cb),
            None,
            Some(seek_cb),
        );
        let mut ctx = (ff.avformat_alloc_context)();
        (*ctx).pb = avio;
        (*ctx).flags |= ffi::AVFMT_FLAG_CUSTOM_IO as c_int;
        // Limits for the analysis itself (the budget above caps the bytes on top).
        let mut opts: *mut c_void = std::ptr::null_mut();
        for (k, v) in [(c"probesize", c"5000000"), (c"analyzeduration", c"5000000")] {
            (ff.av_dict_set)(&mut opts, k.as_ptr(), v.as_ptr(), 0);
        }
        let open = (ff.avformat_open_input)(&mut ctx, c"".as_ptr(), std::ptr::null(), &mut opts);
        (ff.av_dict_free)(&mut opts);
        let result = if open < 0 {
            // avformat_open_input frees the context on failure.
            Err(open)
        } else {
            let r = (ff.avformat_find_stream_info)(ctx, std::ptr::null_mut());
            let info = (r >= 0).then(|| read_info(ff, ctx, size, name));
            (ff.avformat_close_input)(&mut ctx);
            info.ok_or(r)
        };
        // The AVIOContext's buffer may have been replaced; it is the context's first field.
        if !avio.is_null() {
            (ff.av_freep)(avio as *mut c_void);
            (ff.avio_context_free)(&mut avio);
        }
        let read = st.read;
        match (result, st.error) {
            (Ok(info), _) => Ok(Probed {
                info,
                bytes_read: read,
            }),
            (Err(_), Some(e)) => Err(ProbeError::Io(e)),
            (Err(code), None) => Err(ProbeError::Unreadable(code)),
        }
    }
}

fn cstr(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    // SAFETY: FFmpeg returns NUL-terminated static or owned strings.
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    (!s.is_empty()).then_some(s)
}

/// A metadata tag; Matroska writes statistics per language ("BPS-eng").
unsafe fn tag(ff: &ffi::Ffmpeg, dict: *const ffi::AVDictionary, keys: &[&CStr]) -> Option<String> {
    for k in keys {
        // SAFETY: `dict` belongs to a live stream or context.
        let e = unsafe { (ff.av_dict_get)(dict as *const c_void, k.as_ptr(), std::ptr::null(), 0) };
        if !e.is_null() {
            // SAFETY: a returned entry has a valid value.
            if let Some(v) = cstr(unsafe { (*e).value }) {
                return Some(v);
            }
        }
    }
    None
}

fn rate(r: ffi::AVRational) -> Option<f64> {
    (r.num > 0 && r.den > 0).then(|| f64::from(r.num) / f64::from(r.den))
}

fn container_name(format: &str, name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match format.split(',').next().unwrap_or(format) {
        "matroska" if ext == "webm" => "WebM".into(),
        "matroska" | "mkv" => "Matroska".into(),
        "mp4" => "MP4".into(),
        "mov" if ext == "mov" => "QuickTime".into(),
        "mov" => "MP4".into(),
        "mpegts" => "MPEG-TS".into(),
        "mpeg" => "MPEG-PS".into(),
        "avi" => "AVI".into(),
        "flv" => "FLV".into(),
        "asf" => "WMV".into(),
        "ogg" => "Ogg".into(),
        other => other.to_string(),
    }
}

/// "16:9" for common ratios, else "2.39:1".
pub fn aspect(w: f64, h: f64) -> Option<String> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = w / h;
    for (name, v) in [("4:3", 4.0 / 3.0), ("16:9", 16.0 / 9.0), ("1:1", 1.0)] {
        if (r - v).abs() < 0.01 {
            return Some(name.into());
        }
    }
    Some(format!("{r:.2}:1"))
}

/// "5.1", "4.1": HEVC levels are 30 × the level, the others 10 ×.
fn level(codec: &str, level: c_int) -> Option<String> {
    if level <= 0 {
        return None;
    }
    let l = if codec == "hevc" {
        f64::from(level) / 30.0
    } else if matches!(codec, "h264" | "mpeg4") {
        f64::from(level) / 10.0
    } else {
        return None;
    };
    Some(if l.fract() == 0.0 {
        format!("{l:.0}")
    } else {
        format!("{l:.1}")
    })
}

/// Dolby Vision profile with base-layer compatibility: "8.1", "5", "7.6".
pub fn dv_label(profile: u8, compat: u8) -> String {
    if compat == 0 {
        profile.to_string()
    } else {
        format!("{profile}.{compat}")
    }
}

fn subtitle_format(codec: &str) -> String {
    match codec {
        "subrip" | "srt" => "SRT",
        "ass" | "ssa" => "ASS",
        "hdmv_pgs_subtitle" => "PGS",
        "dvd_subtitle" => "VobSub",
        "dvb_subtitle" => "DVB",
        "webvtt" => "WebVTT",
        "mov_text" => "MP4 text",
        other => return other.to_string(),
    }
    .into()
}

/// Read everything from an opened context.
unsafe fn read_info(
    ff: &ffi::Ffmpeg,
    ctx: *mut ffi::AVFormatContext,
    size: u64,
    name: &str,
) -> ProbeInfo {
    // SAFETY: `ctx` is an open AVFormatContext after a successful find_stream_info.
    let c = unsafe { &*ctx };
    let format = if c.iformat.is_null() {
        String::new()
    } else {
        // SAFETY: a non-null input format has a name.
        cstr(unsafe { (*c.iformat).name }).unwrap_or_default()
    };
    let duration_ms = if c.duration > 0 {
        (c.duration / 1000) as u64
    } else {
        0
    };
    let bitrate = if c.bit_rate > 0 {
        Some(Bitrate {
            bps: c.bit_rate as u64,
            estimated: false,
        })
    } else if duration_ms > 0 && size > 0 {
        Some(Bitrate {
            bps: size * 8 * 1000 / duration_ms,
            estimated: true,
        })
    } else {
        None
    };
    let mut info = ProbeInfo {
        schema: SCHEMA,
        source: "ffmpeg".into(),
        container: container_name(&format, name),
        duration_ms,
        size,
        bitrate,
        chapters: c.nb_chapters,
        ..Default::default()
    };
    for i in 0..c.nb_chapters as usize {
        // SAFETY: `chapters` holds `nb_chapters` valid pointers.
        let ch = unsafe { &**c.chapters.add(i) };
        let tb = ch.time_base;
        let ms = |t: i64| -> u64 {
            if tb.den == 0 || t < 0 {
                0
            } else {
                (t as i128 * tb.num as i128 * 1000 / tb.den as i128) as u64
            }
        };
        // SAFETY: the chapter's metadata dictionary is live.
        let title =
            unsafe { tag(ff, ch.metadata, &[c"title"]) }.unwrap_or_else(|| format!("{}", i + 1));
        info.chapter_list.push(Chapter {
            title,
            start_ms: ms(ch.start),
            end_ms: ms(ch.end),
        });
    }
    for i in 0..c.nb_streams as usize {
        // SAFETY: `streams` holds `nb_streams` valid pointers.
        let s = unsafe { &**c.streams.add(i) };
        // SAFETY: every stream has codec parameters.
        let p = unsafe { &*s.codecpar };
        let disp = s.disposition as u32;
        // SAFETY: names for any codec id are static strings.
        let codec = cstr(unsafe { (ff.avcodec_get_name)(p.codec_id as c_int) }).unwrap_or_default();
        // SAFETY: the stream's metadata dictionary is live.
        let lang = unsafe { tag(ff, s.metadata, &[c"language"]) }.filter(|l| l != "und");
        // SAFETY: as above.
        let title = unsafe { tag(ff, s.metadata, &[c"title"]) };
        // Stream bit rate, else Matroska's statistics tags.
        let stream_rate = if p.bit_rate > 0 {
            Some(Bitrate {
                bps: p.bit_rate as u64,
                estimated: false,
            })
        } else {
            // SAFETY: as above.
            unsafe { tag(ff, s.metadata, &[c"BPS", c"BPS-eng"]) }
                .and_then(|v| v.parse().ok())
                .map(|bps| Bitrate {
                    bps,
                    estimated: false,
                })
        };
        match p.codec_type {
            ffi::AVMediaType_AVMEDIA_TYPE_VIDEO => {
                if disp & ffi::AV_DISPOSITION_ATTACHED_PIC != 0 {
                    continue; // cover art
                }
                // SAFETY: a pixel format id is valid for av_pix_fmt_desc_get (null if unknown).
                let desc = unsafe { (ff.av_pix_fmt_desc_get)(p.format) };
                let (bit_depth, chroma) = if desc.is_null() {
                    (None, None)
                } else {
                    // SAFETY: non-null descriptor.
                    let d = unsafe { &*desc };
                    let chroma = if d.flags & u64::from(ffi::AV_PIX_FMT_FLAG_RGB) != 0 {
                        None
                    } else {
                        Some(
                            match (d.log2_chroma_w, d.log2_chroma_h) {
                                (1, 1) => "4:2:0",
                                (1, 0) => "4:2:2",
                                (0, 0) => "4:4:4",
                                (2, 0) => "4:1:1",
                                _ => "",
                            }
                            .to_string(),
                        )
                        .filter(|c| !c.is_empty())
                    };
                    (Some(d.comp[0].depth as u32).filter(|d| *d > 0), chroma)
                };
                // SAFETY: coded side data is an array of `nb_coded_side_data` entries.
                let dovi = unsafe {
                    (ff.av_packet_side_data_get)(
                        p.coded_side_data,
                        p.nb_coded_side_data,
                        ffi::AVPacketSideDataType_AV_PKT_DATA_DOVI_CONF as c_int,
                    )
                };
                let dv = if dovi.is_null() {
                    None
                } else {
                    // SAFETY: DOVI_CONF side data holds an AVDOVIDecoderConfigurationRecord.
                    let r =
                        unsafe { &*((*dovi).data as *const ffi::AVDOVIDecoderConfigurationRecord) };
                    Some(dv_label(r.dv_profile, r.dv_bl_signal_compatibility_id))
                };
                let hdr = if dv.is_some() {
                    Some("Dolby Vision".to_string())
                } else {
                    match p.color_trc {
                        ffi::AVColorTransferCharacteristic_AVCOL_TRC_SMPTE2084 => {
                            Some("HDR10".into())
                        }
                        ffi::AVColorTransferCharacteristic_AVCOL_TRC_ARIB_STD_B67 => {
                            Some("HLG".into())
                        }
                        _ => None,
                    }
                };
                let sar = rate(p.sample_aspect_ratio).unwrap_or(1.0);
                let fps = rate(s.avg_frame_rate)
                    .or_else(|| rate(p.framerate))
                    .or_else(|| rate(s.r_frame_rate))
                    .map(|f| (f * 1000.0).round() / 1000.0);
                info.video.push(Video {
                    // SAFETY: profile names are static strings (null when unknown).
                    profile: cstr(unsafe {
                        (ff.avcodec_profile_name)(p.codec_id as c_int, p.profile)
                    }),
                    level: level(&codec, p.level),
                    width: p.width.max(0) as u32,
                    height: p.height.max(0) as u32,
                    dar: aspect(f64::from(p.width) * sar, f64::from(p.height)),
                    fps,
                    scan: match p.field_order {
                        ffi::AVFieldOrder_AV_FIELD_PROGRESSIVE => Some("progressive".into()),
                        0 => None,
                        _ => Some("interlaced".into()),
                    },
                    bit_depth,
                    chroma,
                    hdr,
                    dv_profile: dv,
                    // SAFETY: names for any colour id are static strings (null when unknown).
                    primaries: cstr(unsafe {
                        (ff.av_color_primaries_name)(p.color_primaries as c_int)
                    })
                    .filter(|n| n != "unknown" && n != "reserved"),
                    // SAFETY: as above.
                    transfer: cstr(unsafe { (ff.av_color_transfer_name)(p.color_trc as c_int) })
                        .filter(|n| n != "unknown" && n != "reserved"),
                    bitrate: stream_rate,
                    codec,
                });
            }
            ffi::AVMediaType_AVMEDIA_TYPE_AUDIO => {
                let mut buf = [0 as c_char; 64];
                // SAFETY: the buffer is large enough; FFmpeg NUL-terminates.
                let n = unsafe {
                    (ff.av_channel_layout_describe)(&p.ch_layout, buf.as_mut_ptr(), buf.len())
                };
                let layout = (n > 0).then(|| cstr(buf.as_ptr())).flatten().map(|l| {
                    // "5.1(side)" -> "5.1"
                    l.split('(').next().unwrap_or(&l).to_string()
                });
                info.audio.push(Audio {
                    language: lang,
                    title,
                    // SAFETY: as for video.
                    profile: cstr(unsafe {
                        (ff.avcodec_profile_name)(p.codec_id as c_int, p.profile)
                    }),
                    layout,
                    channels: p.ch_layout.nb_channels.max(0) as u32,
                    sample_rate: p.sample_rate.max(0) as u32,
                    bit_depth: Some(p.bits_per_raw_sample as u32).filter(|b| *b > 0),
                    bitrate: stream_rate,
                    default: disp & ffi::AV_DISPOSITION_DEFAULT != 0,
                    forced: disp & ffi::AV_DISPOSITION_FORCED != 0,
                    codec,
                });
            }
            ffi::AVMediaType_AVMEDIA_TYPE_SUBTITLE => info.subtitles.push(Subtitle {
                language: lang,
                title,
                format: subtitle_format(&codec),
                external: false,
                default: disp & ffi::AV_DISPOSITION_DEFAULT != 0,
                forced: disp & ffi::AV_DISPOSITION_FORCED != 0,
                sdh: disp & ffi::AV_DISPOSITION_HEARING_IMPAIRED != 0,
            }),
            _ => {}
        }
    }
    // The overall rate from size and duration when the container has none, and the video's
    // from the overall minus the audio when only that is known: both labelled "estimated".
    if info.video.len() == 1
        && info.video[0].bitrate.is_none()
        && let Some(total) = info.bitrate
    {
        let audio: u64 = info
            .audio
            .iter()
            .filter_map(|a| a.bitrate.map(|b| b.bps))
            .sum();
        let all_audio_known = info.audio.iter().all(|a| a.bitrate.is_some());
        if all_audio_known && total.bps > audio {
            info.video[0].bitrate = Some(Bitrate {
                bps: total.bps - audio,
                estimated: true,
            });
        }
    }
    info
}

/// The same model from what mpv reports during playback (the fallback when a file could not be
/// probed, or FFmpeg is not available to the probe): `track-list`, `video-params`,
/// `file-format`, `duration` (s) and `file-size`, as `MpvPlayer::mediaInfoJson` collects them.
/// Values mpv does not know stay empty; bit rates come only from the demuxer.
pub fn from_mpv(v: &serde_json::Value) -> ProbeInfo {
    use serde_json::Value;
    let s = |v: &Value, k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|s| !s.is_empty())
    };
    let n = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64);
    let b = |v: &Value, k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    let size = v.get("file-size").and_then(Value::as_u64).unwrap_or(0);
    let duration_ms = (n(v, "duration").unwrap_or(0.0) * 1000.0) as u64;
    let format = s(v, "file-format").unwrap_or_default();
    let mut info = ProbeInfo {
        schema: SCHEMA,
        source: "mpv".into(),
        container: container_name(&format, ""),
        duration_ms,
        size,
        bitrate: (duration_ms > 0 && size > 0).then(|| Bitrate {
            bps: size * 8 * 1000 / duration_ms,
            estimated: false,
        }),
        ..Default::default()
    };
    let vp = v.get("video-params").cloned().unwrap_or(Value::Null);
    let rate = |t: &Value| {
        n(t, "demux-bitrate").filter(|r| *r > 0.0).map(|r| Bitrate {
            bps: r as u64,
            estimated: false,
        })
    };
    for t in v
        .get("track-list")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let lang = s(t, "lang").filter(|l| l != "und");
        match t.get("type").and_then(Value::as_str) {
            Some("video") if !b(t, "albumart") && !b(t, "image") => {
                let gamma = s(&vp, "gamma").unwrap_or_default();
                let pix = s(&vp, "pixelformat").unwrap_or_default();
                let w = n(t, "demux-w").or_else(|| n(&vp, "w")).unwrap_or(0.0);
                let h = n(t, "demux-h").or_else(|| n(&vp, "h")).unwrap_or(0.0);
                info.video.push(Video {
                    codec: s(t, "codec").unwrap_or_default(),
                    profile: s(t, "codec-profile"),
                    width: w as u32,
                    height: h as u32,
                    dar: n(&vp, "aspect")
                        .and_then(|a| aspect(a, 1.0))
                        .or_else(|| aspect(w, h)),
                    fps: n(t, "demux-fps").map(|f| (f * 1000.0).round() / 1000.0),
                    bit_depth: (!pix.is_empty()).then(|| {
                        if pix.contains("10") || pix.contains("p010") {
                            10
                        } else if pix.contains("12") {
                            12
                        } else {
                            8
                        }
                    }),
                    hdr: match gamma.as_str() {
                        "pq" => Some("HDR10".into()),
                        "hlg" => Some("HLG".into()),
                        _ => None,
                    },
                    primaries: s(&vp, "primaries"),
                    transfer: Some(gamma).filter(|g| !g.is_empty()),
                    bitrate: rate(t),
                    ..Default::default()
                });
            }
            Some("audio") => {
                let layout =
                    s(t, "demux-channels").map(|l| l.split('(').next().unwrap_or(&l).to_string());
                info.audio.push(Audio {
                    language: lang,
                    title: s(t, "title"),
                    codec: s(t, "codec").unwrap_or_default(),
                    profile: s(t, "codec-profile"),
                    layout,
                    channels: n(t, "demux-channel-count").unwrap_or(0.0) as u32,
                    sample_rate: n(t, "demux-samplerate").unwrap_or(0.0) as u32,
                    bitrate: rate(t),
                    default: b(t, "default"),
                    forced: b(t, "forced"),
                    ..Default::default()
                });
            }
            Some("sub") => info.subtitles.push(Subtitle {
                language: lang,
                title: s(t, "title"),
                format: subtitle_format(&s(t, "codec").unwrap_or_default()),
                external: b(t, "external"),
                default: b(t, "default"),
                forced: b(t, "forced"),
                sdh: b(t, "hearing-impaired"),
            }),
            _ => {}
        }
    }
    info
}

/// A local file as a [`ReadAt`] (the CLI and tests).
pub struct FileReader(pub std::fs::File);

impl ReadAt for FileReader {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        use std::io::{Read, Seek, SeekFrom};
        self.0.seek(SeekFrom::Start(offset))?;
        self.0.read(buf)
    }
}
