//! Issue #33: the probe against files generated here with the system `ffmpeg` (skipped when it
//! is not installed; nothing copyrighted, no video in the repository). The probe itself loads
//! FFmpeg 8.1 from the libmpv bundle (third_party/mpv/out/lib), as the app ships it.
use std::path::{Path, PathBuf};
use std::process::Command;

use huehuecoyotl_probe::{FileReader, ProbeInfo, ReadAt, probe, probe_with_budget, waveform};

fn setup() -> Option<tempfile::TempDir> {
    // A directory set from outside wins: the FFmpeg 7.1 build is tested on the runtime's libraries.
    if std::env::var_os("COYOTL_FFMPEG_DIR").is_none() {
        // SAFETY: set before the probe loads FFmpeg; tests in this file all set the same value.
        unsafe {
            std::env::set_var(
                "COYOTL_FFMPEG_DIR",
                concat!(env!("CARGO_MANIFEST_DIR"), "/../../third_party/mpv/out/lib"),
            )
        };
    }
    if huehuecoyotl_probe::ffmpeg().is_err()
        || Command::new("ffmpeg").arg("-version").output().is_err()
    {
        eprintln!(
            "skipped: needs the libmpv bundle (third_party/mpv/build.sh) and an ffmpeg command: {:?}",
            huehuecoyotl_probe::ffmpeg().err()
        );
        return None;
    }
    Some(tempfile::tempdir().unwrap())
}

fn ffmpeg(args: &[&str]) {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn probe_file(p: &Path) -> ProbeInfo {
    let size = std::fs::metadata(p).unwrap().len();
    let name = p.file_name().unwrap().to_string_lossy().to_string();
    let mut r = FileReader(std::fs::File::open(p).unwrap());
    probe(&mut r, size, &name).unwrap().info
}

/// MKV: HEVC 10-bit HDR10, AC-3 5.1 (English, default) and AAC stereo (German commentary),
/// SRT (English, forced) and ASS (German).
fn mkv(dir: &Path) -> PathBuf {
    let srt = dir.join("en.srt");
    std::fs::write(&srt, "1\n00:00:00,000 --> 00:00:01,000\nHello\n").unwrap();
    let out = dir.join("Example.2160p.HDR10.mkv");
    let o = out.to_string_lossy().to_string();
    let s = srt.to_string_lossy().to_string();
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=640x360:rate=24000/1001:duration=2",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:duration=2",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=660:duration=2",
        "-i",
        &s,
        "-i",
        &s,
        "-map",
        "0:v",
        "-map",
        "1:a",
        "-map",
        "2:a",
        "-map",
        "3:s",
        "-map",
        "4:s",
        "-c:v",
        "libx265",
        "-pix_fmt",
        "yuv420p10le",
        "-preset",
        "ultrafast",
        "-x265-params",
        "log-level=error:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:hdr10=1:max-cll=1000,400:master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1)",
        "-color_primaries",
        "bt2020",
        "-color_trc",
        "smpte2084",
        "-colorspace",
        "bt2020nc",
        "-c:a:0",
        "ac3",
        "-ac:a:0",
        "6",
        "-b:a:0",
        "448k",
        "-c:a:1",
        "aac",
        "-ac:a:1",
        "2",
        "-b:a:1",
        "128k",
        "-c:s:0",
        "srt",
        "-c:s:1",
        "ass",
        "-metadata:s:a:0",
        "language=eng",
        "-metadata:s:a:1",
        "language=ger",
        "-metadata:s:a:1",
        "title=Kommentar",
        "-metadata:s:s:0",
        "language=eng",
        "-metadata:s:s:1",
        "language=ger",
        "-disposition:a:0",
        "default",
        "-disposition:a:1",
        "0",
        "-disposition:s:0",
        "forced",
        "-disposition:s:1",
        "0",
        &o,
    ]);
    out
}

#[test]
fn mkv_hevc_hdr10_with_tracks() {
    let Some(d) = setup() else { return };
    let i = probe_file(&mkv(d.path()));
    assert_eq!(
        (i.schema, i.source.as_str(), i.container.as_str()),
        (1, "ffmpeg", "Matroska")
    );
    assert!((1900..2200).contains(&i.duration_ms), "{}", i.duration_ms);
    let v = &i.video[0];
    assert_eq!(
        (
            v.codec.as_str(),
            v.profile.as_deref(),
            v.width,
            v.height,
            v.bit_depth,
            v.chroma.as_deref()
        ),
        ("hevc", Some("Main 10"), 640, 360, Some(10), Some("4:2:0"))
    );
    assert_eq!(
        (v.hdr.as_deref(), v.dar.as_deref(), v.fps),
        (Some("HDR10"), Some("16:9"), Some(23.976))
    );
    assert_eq!(v.transfer.as_deref(), Some("smpte2084"));
    let a: Vec<_> = i
        .audio
        .iter()
        .map(|a| {
            (
                a.codec.as_str(),
                a.layout.as_deref(),
                a.language.as_deref(),
                a.title.as_deref(),
                a.default,
            )
        })
        .collect();
    assert_eq!(
        a,
        [
            ("ac3", Some("5.1"), Some("eng"), None, true),
            ("aac", Some("stereo"), Some("ger"), Some("Kommentar"), false)
        ]
    );
    assert_eq!(
        i.audio[0].bitrate.map(|b| (b.bps, b.estimated)),
        Some((448_000, false))
    );
    assert_eq!(i.audio[1].bitrate, None, "no rate is invented");
    let s: Vec<_> = i
        .subtitles
        .iter()
        .map(|s| (s.format.as_str(), s.language.as_deref(), s.forced))
        .collect();
    assert_eq!(s, [("SRT", Some("eng"), true), ("ASS", Some("ger"), false)]);
}

#[test]
fn mp4_h264_aac_and_mpeg_ts() {
    let Some(d) = setup() else { return };
    let mp4 = d.path().join("Example.1080p.mp4");
    let ts = d.path().join("Example.ts");
    for (out, extra) in [
        (&mp4, ["-c:v", "libx264", "-c:a", "aac", "-b:a", "160k"]),
        (&ts, ["-c:v", "libx264", "-c:a", "mp2", "-b:a", "192k"]),
    ] {
        let o = out.to_string_lossy().to_string();
        let mut args = vec![
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1280x720:rate=25:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=duration=2",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "ultrafast",
        ];
        args.extend(extra);
        args.push(&o);
        ffmpeg(&args);
    }
    let m = probe_file(&mp4);
    assert_eq!(m.container, "MP4");
    assert_eq!(
        (
            m.video[0].codec.as_str(),
            m.video[0].width,
            m.video[0].bit_depth
        ),
        ("h264", 1280, Some(8))
    );
    assert_eq!(
        (m.video[0].hdr.as_deref(), m.video[0].fps),
        (None, Some(25.0))
    );
    assert!(
        m.video[0].bitrate.is_some_and(|b| !b.estimated),
        "MP4 stores the video rate"
    );
    assert_eq!(m.audio[0].codec, "aac");
    let t = probe_file(&ts);
    assert_eq!(t.container, "MPEG-TS");
    assert_eq!(
        (t.video[0].codec.as_str(), t.audio[0].codec.as_str()),
        ("h264", "mp2")
    );
}

/// A reader that is a 50 GB file to the probe: the fixture's bytes first, zeros after. Counts
/// what is read, as a network share would.
struct Huge {
    head: Vec<u8>,
    read: u64,
}

impl ReadAt for Huge {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = self.head.get(offset as usize + i).copied().unwrap_or(0);
        }
        self.read += buf.len() as u64;
        Ok(buf.len())
    }
}

#[test]
fn a_50_gb_file_is_probed_from_a_small_part() {
    let Some(d) = setup() else { return };
    let mut r = Huge {
        head: std::fs::read(mkv(d.path())).unwrap(),
        read: 0,
    };
    let size = 50 * 1024 * 1024 * 1024;
    let p = probe(&mut r, size, "Huge.mkv").unwrap();
    assert_eq!(p.info.video[0].hdr.as_deref(), Some("HDR10"));
    assert_eq!(p.bytes_read, r.read);
    assert!(r.read < 8 * 1024 * 1024, "read {} bytes of 50 GB", r.read);
    // The budget holds even when the container asks for more.
    let mut r = Huge {
        head: Vec::new(),
        read: 0,
    };
    let _ = probe_with_budget(&mut r, size, "Zeros.mkv", 1024 * 1024);
    assert!(r.read <= 1024 * 1024, "read {} bytes", r.read);
}

#[test]
fn dolby_vision_profiles_are_labelled() {
    use huehuecoyotl_probe::dv_label;
    assert_eq!(dv_label(8, 1), "8.1");
    assert_eq!(dv_label(5, 0), "5");
    assert_eq!(dv_label(7, 6), "7.6");
}

#[test]
fn playback_fallback_from_mpv() {
    let v = serde_json::json!({
        "file-format": "mkv", "duration": 5400.5, "file-size": 8_000_000_000u64,
        "video-params": {"gamma": "pq", "primaries": "bt.2020", "pixelformat": "yuv420p10", "w": 3840, "h": 2160, "aspect": 1.777},
        "track-list": [
            {"type": "video", "codec": "hevc", "demux-w": 3840, "demux-h": 2160, "demux-fps": 23.976023},
            {"type": "audio", "codec": "truehd", "lang": "eng", "demux-channels": "7.1", "demux-channel-count": 8, "demux-samplerate": 48000, "default": true},
            {"type": "sub", "codec": "hdmv_pgs_subtitle", "lang": "ger", "forced": true},
            {"type": "sub", "codec": "subrip", "lang": "eng", "external": true}
        ]
    });
    let i = huehuecoyotl_probe::from_mpv(&v);
    assert_eq!(
        (i.source.as_str(), i.container.as_str(), i.duration_ms),
        ("mpv", "Matroska", 5_400_500)
    );
    let vd = &i.video[0];
    assert_eq!(
        (
            vd.width,
            vd.hdr.as_deref(),
            vd.bit_depth,
            vd.fps,
            vd.dar.as_deref()
        ),
        (3840, Some("HDR10"), Some(10), Some(23.976), Some("16:9"))
    );
    assert_eq!(
        (
            i.audio[0].codec.as_str(),
            i.audio[0].layout.as_deref(),
            i.audio[0].bitrate
        ),
        ("truehd", Some("7.1"), None)
    );
    let subs: Vec<_> = i
        .subtitles
        .iter()
        .map(|s| (s.format.as_str(), s.forced, s.external))
        .collect();
    assert_eq!(subs, [("PGS", true, false), ("SRT", false, true)]);
}

/// Issue #82: the waveform of one second of tone followed by one second of silence, as FLAC
/// and as MP3: the first half of the bars is loud, the second half silent.
#[test]
fn waveform_follows_the_audio() {
    let Some(d) = setup() else { return };
    for (ext, codec) in [("flac", "flac"), ("mp3", "libmp3lame")] {
        let p = d.path().join(format!("half.{ext}"));
        let o = p.to_string_lossy().to_string();
        ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=1,apad=pad_dur=1",
            "-c:a",
            codec,
            &o,
        ]);
        let size = std::fs::metadata(&p).unwrap().len();
        let mut r = FileReader(std::fs::File::open(&p).unwrap());
        let w = waveform(&mut r, size, 20).unwrap();
        assert_eq!(w.len(), 20, "{ext}");
        // FFmpeg's test tone peaks at 1/8 of full scale (32 of 255).
        assert!(w[1..9].iter().all(|v| *v > 20), "{ext}: {w:?}");
        assert!(w[12..].iter().all(|v| *v < 10), "{ext}: {w:?}");
    }
}
