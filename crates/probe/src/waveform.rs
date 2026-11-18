//! Issue #82: a track's waveform for the Now Playing seek bar: the file is decoded with FFmpeg
//! (through the same custom I/O as the probe) and every sample's absolute level is folded into
//! `bars` peaks over the track's length, 0 to 255. It reads the whole file, so it runs as a
//! background job, once per file.

use core::ffi::{c_int, c_void};

use crate::ffi::{self, ffmpeg};
use crate::{IoState, ProbeError, ReadAt, read_cb, seek_cb};

/// The peak (0.0 to 1.0) of each of a frame's samples' channels, at its position.
unsafe fn frame_peak(f: &ffi::AVFrame) -> f32 {
    let n = f.nb_samples.max(0) as usize;
    let ch = f.ch_layout.nb_channels.max(1) as usize;
    let planar = f.format >= ffi::AVSampleFormat_AV_SAMPLE_FMT_U8P;
    let base = if planar {
        f.format - ffi::AVSampleFormat_AV_SAMPLE_FMT_U8P
    } else {
        f.format
    };
    let planes = if planar { ch } else { 1 };
    let per_plane = if planar { n } else { n * ch };
    let mut peak = 0f32;
    for p in 0..planes {
        // SAFETY: `extended_data` holds one pointer per plane of `per_plane` samples of the
        // frame's format, as FFmpeg documents for decoded audio.
        let data = unsafe { *f.extended_data.add(p) };
        if data.is_null() {
            continue;
        }
        macro_rules! scan {
            ($t:ty, $norm:expr) => {{
                // SAFETY: see above.
                let s = unsafe { std::slice::from_raw_parts(data as *const $t, per_plane) };
                for v in s {
                    let x: f32 = $norm(*v);
                    if x > peak {
                        peak = x;
                    }
                }
            }};
        }
        match base {
            ffi::AVSampleFormat_AV_SAMPLE_FMT_U8 => {
                scan!(u8, |v: u8| (f32::from(v) - 128.0).abs() / 128.0)
            }
            ffi::AVSampleFormat_AV_SAMPLE_FMT_S16 => {
                scan!(i16, |v: i16| f32::from(v).abs() / 32768.0)
            }
            ffi::AVSampleFormat_AV_SAMPLE_FMT_S32 => {
                scan!(i32, |v: i32| (v as f32).abs() / 2_147_483_648.0)
            }
            ffi::AVSampleFormat_AV_SAMPLE_FMT_FLT => scan!(f32, |v: f32| v.abs()),
            ffi::AVSampleFormat_AV_SAMPLE_FMT_DBL => scan!(f64, |v: f64| v.abs() as f32),
            _ => {}
        }
    }
    peak.min(1.0)
}

/// Fold (samples, peak) of each frame in order into `bars` peaks, 0 to 255.
pub fn fold(frames: &[(u64, f32)], bars: usize) -> Vec<u8> {
    let total: u64 = frames.iter().map(|f| f.0).sum();
    let mut out = vec![0f32; bars];
    if total == 0 || bars == 0 {
        return vec![0; bars];
    }
    let mut touched = vec![false; bars];
    let mut at = 0u64;
    for (n, p) in frames {
        let i = ((at * bars as u64) / total).min(bars as u64 - 1) as usize;
        touched[i] = true;
        if *p > out[i] {
            out[i] = *p;
        }
        at += n;
    }
    // Bars no frame started in (frames longer than a bar) take their neighbour's level; real
    // silence stays silent.
    for i in 1..bars {
        if !touched[i] {
            out[i] = out[i - 1];
        }
    }
    out.iter().map(|p| (p * 255.0).round() as u8).collect()
}

/// Decode the first audio stream of a file of `size` bytes and return `bars` peaks.
pub fn waveform(reader: &mut dyn ReadAt, size: u64, bars: usize) -> Result<Vec<u8>, ProbeError> {
    let ff = ffmpeg().map_err(ProbeError::Unavailable)?;
    let mut st = IoState {
        reader,
        pos: 0,
        size,
        read: 0,
        budget: u64::MAX,
        error: None,
    };
    const BUF: usize = 64 * 1024;
    let mut frames: Vec<(u64, f32)> = Vec::new();
    // SAFETY: the custom-I/O setup is the probe's (see `probe_with_budget`); the decoder, packet
    // and frame are allocated and freed here in reverse order, also on every error path.
    let code = unsafe {
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
        let mut code = (ff.avformat_open_input)(
            &mut ctx,
            c"".as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
        );
        if code >= 0 {
            code = (ff.avformat_find_stream_info)(ctx, std::ptr::null_mut());
            if code >= 0 {
                code = decode(ff, ctx, &mut frames);
            }
            (ff.avformat_close_input)(&mut ctx);
        }
        if !avio.is_null() {
            (ff.av_freep)(avio as *mut c_void);
            (ff.avio_context_free)(&mut avio);
        }
        code
    };
    if let Some(e) = st.error {
        return Err(ProbeError::Io(e));
    }
    if code < 0 && frames.is_empty() {
        return Err(ProbeError::Unreadable(code));
    }
    Ok(fold(&frames, bars))
}

/// The decode loop over an open context (0 or an FFmpeg error code).
unsafe fn decode(
    ff: &ffi::Ffmpeg,
    ctx: *mut ffi::AVFormatContext,
    frames: &mut Vec<(u64, f32)>,
) -> c_int {
    // SAFETY: `ctx` is open with its streams found; every pointer below comes from FFmpeg and
    // is checked for null before use.
    unsafe {
        let n = (*ctx).nb_streams as usize;
        let mut index = None;
        for i in 0..n {
            let s = *(*ctx).streams.add(i);
            if !s.is_null()
                && !(*s).codecpar.is_null()
                && (*(*s).codecpar).codec_type == ffi::AVMediaType_AVMEDIA_TYPE_AUDIO
            {
                index = Some(i);
                break;
            }
        }
        let Some(index) = index else { return -1 };
        let par = (**(*ctx).streams.add(index)).codecpar;
        let codec = (ff.avcodec_find_decoder)((*par).codec_id);
        if codec.is_null() {
            return -1;
        }
        let mut dec = (ff.avcodec_alloc_context3)(codec);
        if dec.is_null() {
            return -1;
        }
        let mut code = (ff.avcodec_parameters_to_context)(dec, par);
        if code >= 0 {
            code = (ff.avcodec_open2)(dec, codec, std::ptr::null_mut());
        }
        if code < 0 {
            (ff.avcodec_free_context)(&mut dec);
            return code;
        }
        let mut pkt = (ff.av_packet_alloc)();
        let mut frame = (ff.av_frame_alloc)();
        let mut drain = |dec: *mut ffi::AVCodecContext| loop {
            let r = (ff.avcodec_receive_frame)(dec, frame);
            if r < 0 {
                break;
            }
            frames.push(((*frame).nb_samples.max(0) as u64, frame_peak(&*frame)));
        };
        loop {
            let r = (ff.av_read_frame)(ctx, pkt);
            if r < 0 {
                break;
            }
            if (*pkt).stream_index as usize == index && (ff.avcodec_send_packet)(dec, pkt) >= 0 {
                drain(dec);
            }
            (ff.av_packet_unref)(pkt);
        }
        // Flush what the decoder still holds.
        if (ff.avcodec_send_packet)(dec, std::ptr::null()) >= 0 {
            drain(dec);
        }
        (ff.av_frame_free)(&mut frame);
        (ff.av_packet_free)(&mut pkt);
        (ff.avcodec_free_context)(&mut dec);
        0
    }
}

#[cfg(test)]
mod tests {
    use super::fold;

    #[test]
    fn frames_fold_into_bars() {
        // Loud first half, quiet second half, four frames of 100 samples.
        let f = [(100, 1.0), (100, 0.9), (100, 0.1), (100, 0.0)];
        assert_eq!(fold(&f, 4), [255, 230, 26, 0]);
        // One long frame over three bars.
        assert_eq!(fold(&[(300, 0.5)], 3), [128, 128, 128]);
        assert_eq!(fold(&f, 2), [255, 26]);
        assert_eq!(fold(&[], 3), [0, 0, 0]);
    }
}
