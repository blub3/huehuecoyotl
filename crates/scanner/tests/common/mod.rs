//! Issue #7: audio fixtures made in the test setup (sine tones with tags), never committed music.
//! They are written with the system's `ffmpeg` command, a test tool only; tests that need them
//! are skipped with a note when it is missing.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// One tagged tone. `ext` picks the format: flac, m4a (ALAC with `alac`), wav, aiff, mp3,
/// m4a (AAC), ogg, opus, wv. `tags` are ffmpeg metadata pairs. `cover` embeds a picture.
pub fn tone(
    dir: &Path,
    rel: &str,
    codec: &[&str],
    rate: u32,
    seconds: f32,
    tags: &[(&str, &str)],
    cover: Option<&Path>,
) -> PathBuf {
    let src = format!("sine=frequency=440:sample_rate={rate}:duration={seconds}");
    lavfi(dir, rel, &src, codec, tags, cover)
}

/// White noise, which no codec can shrink: for fixtures that must be large.
pub fn noise(
    dir: &Path,
    rel: &str,
    codec: &[&str],
    rate: u32,
    seconds: f32,
    tags: &[(&str, &str)],
    cover: Option<&Path>,
) -> PathBuf {
    let src = format!("anoisesrc=color=white:sample_rate={rate}:duration={seconds}");
    lavfi(dir, rel, &src, codec, tags, cover)
}

fn lavfi(
    dir: &Path,
    rel: &str,
    src: &str,
    codec: &[&str],
    tags: &[(&str, &str)],
    cover: Option<&Path>,
) -> PathBuf {
    let out = dir.join(rel);
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(src);
    if let Some(c) = cover {
        cmd.arg("-i").arg(c).args([
            "-map",
            "0:a",
            "-map",
            "1:v",
            "-disposition:v",
            "attached_pic",
            "-c:v",
            "copy",
        ]);
    }
    cmd.args(codec);
    for (k, v) in tags {
        cmd.arg("-metadata").arg(format!("{k}={v}"));
    }
    let status = cmd.arg(&out).status().unwrap();
    assert!(status.success(), "ffmpeg failed for {rel}");
    out
}

/// A small PNG for embedded and sidecar covers.
pub fn png(path: &Path, rgb: [u8; 3]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let status = Command::new("ffmpeg")
        .args(["-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!(
            "color=c=0x{:02x}{:02x}{:02x}:s=64x64",
            rgb[0], rgb[1], rgb[2]
        ))
        .args(["-frames:v", "1"])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
    path.to_path_buf()
}
