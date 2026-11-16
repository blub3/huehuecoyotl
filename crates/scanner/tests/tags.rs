//! Issue #7: tags and audio properties of every 2.0 format, read through a `Source`.
mod common;

use huehuecoyotl_scanner::tags;
use huehuecoyotl_vfs::{LocalSource, VPath};

async fn read(path: &std::path::Path) -> tags::Read_ {
    let size = std::fs::metadata(path).unwrap().len();
    tags::read(&LocalSource, &VPath::new(path.to_string_lossy()), size)
        .await
        .unwrap()
}

#[tokio::test]
async fn every_format_is_read() {
    if !common::ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let tags = [
        ("title", "Tone"),
        ("artist", "Test Artist"),
        ("album", "Test Album"),
        ("album_artist", "Test Artist"),
        ("track", "3/9"),
        ("disc", "1/2"),
        ("date", "2021-05-04"),
        ("genre", "Test"),
    ];
    // (file, ffmpeg codec options, sample rate, codec, lossless, bit depth)
    type Case<'a> = (&'a str, &'a [&'a str], u32, &'a str, bool, Option<u8>);
    let cases: &[Case] = &[
        (
            "t.flac",
            &["-c:a", "flac", "-sample_fmt", "s32"],
            96000,
            "flac",
            true,
            Some(24),
        ),
        ("alac.m4a", &["-c:a", "alac"], 44100, "alac", true, Some(16)),
        (
            "t.wav",
            &["-c:a", "pcm_s24le"],
            48000,
            "pcm",
            true,
            Some(24),
        ),
        (
            "t.aiff",
            &["-c:a", "pcm_s16be", "-write_id3v2", "1"],
            44100,
            "pcm",
            true,
            Some(16),
        ),
        (
            "t.mp3",
            &["-c:a", "libmp3lame", "-b:a", "128k"],
            44100,
            "mp3",
            false,
            None,
        ),
        (
            "aac.m4a",
            &["-c:a", "aac", "-b:a", "128k"],
            44100,
            "aac",
            false,
            None,
        ),
        (
            "t.ogg",
            &["-c:a", "libvorbis"],
            44100,
            "vorbis",
            false,
            None,
        ),
        ("t.opus", &["-c:a", "libopus"], 48000, "opus", false, None),
        (
            "t.wv",
            &["-c:a", "wavpack"],
            44100,
            "wavpack",
            true,
            Some(16),
        ),
    ];
    for (name, codec, rate, want_codec, lossless, depth) in cases {
        // APE tags (WavPack) name the date "Year", as taggers write it.
        let mut tags = tags.to_vec();
        if name.ends_with(".wv") {
            tags.push(("year", "2021"));
        }
        let f = common::tone(d.path(), name, codec, *rate, 1.0, &tags, None);
        let r = read(&f).await;
        let t = &r.tags;
        assert_eq!(t.audio.codec, *want_codec, "{name}");
        assert_eq!(t.audio.lossless, *lossless, "{name}");
        assert_eq!(t.audio.sample_rate, Some(*rate), "{name}");
        if depth.is_some() {
            assert_eq!(t.audio.bit_depth, *depth, "{name}");
        }
        assert!(
            (900..=1100).contains(&t.audio.duration_ms),
            "{name}: {}",
            t.audio.duration_ms
        );
        assert_eq!(t.title.as_deref(), Some("Tone"), "{name}");
        assert_eq!(t.artists[0].name, "Test Artist", "{name}");
        assert_eq!(t.album.as_deref(), Some("Test Album"), "{name}");
        // ffmpeg writes no track number or date into a WAV's INFO chunk.
        if *name != "t.wav" {
            assert_eq!(t.track_no, Some(3), "{name}");
            assert_eq!(t.year, Some(2021), "{name}");
        }
    }
}

/// A 48 MiB FLAC with a large embedded cover: the reader fetches its header blocks only.
#[tokio::test]
async fn reads_only_the_tag_blocks() {
    if !common::ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let cover = common::png(&d.path().join("c.png"), [200, 30, 30]);
    let f = common::noise(
        d.path(),
        "long.flac",
        &["-c:a", "flac", "-sample_fmt", "s32"],
        192000,
        60.0,
        &[("title", "Long"), ("artist", "A")],
        Some(&cover),
    );
    let size = std::fs::metadata(&f).unwrap().len();
    let r = read(&f).await;
    assert!(size > 20 << 20, "fixture too small: {size}");
    assert!(
        r.bytes_read <= 4 * 64 * 1024,
        "read {} of {size} bytes",
        r.bytes_read
    );
    assert_eq!(r.tags.title.as_deref(), Some("Long"));
    assert!(
        r.tags
            .cover
            .as_ref()
            .is_some_and(|c| c.starts_with(b"\x89PNG"))
    );
    assert!((59_000..=61_000).contains(&r.tags.audio.duration_ms));
}

/// A broken file is an error for that file, never a panic or a hang.
#[tokio::test]
async fn broken_files_fail_cleanly() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("junk.flac");
    std::fs::write(&f, vec![0x55u8; 300_000]).unwrap();
    let size = std::fs::metadata(&f).unwrap().len();
    assert!(
        tags::read(&LocalSource, &VPath::new(f.to_string_lossy()), size)
            .await
            .is_err()
    );
    let f = d.path().join("empty.mp3");
    std::fs::write(&f, b"").unwrap();
    let _ = tags::read(&LocalSource, &VPath::new(f.to_string_lossy()), 0).await;
}
