//! File-type detection and sidecar subtitle discovery (SPEC 5.5).

use crate::Entry;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    /// Issue #7: the music formats of 2.0 (SPEC 2, "Music in 2.0").
    Audio,
    Subtitle,
    Other,
}

const VIDEO: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "webm", "ts", "m2ts", "mts", "wmv", "mpg", "mpeg", "vob",
    "ogv", "flv", "3gp", "iso",
];
const SUBTITLE: &[&str] = &["srt", "ass", "ssa", "vtt", "sub", "idx", "sup"];
/// `.mp4` counts as video here; music folders also accept audio-only `.mp4` files.
const AUDIO: &[&str] = &[
    "flac", "m4a", "wav", "aif", "aiff", "mp3", "aac", "ogg", "oga", "opus", "wv",
];

fn ext(name: &str) -> Option<String> {
    let (stem, e) = name.rsplit_once('.')?;
    (!stem.is_empty()).then(|| e.to_ascii_lowercase())
}

pub fn media_kind(name: &str) -> MediaKind {
    match ext(name) {
        Some(e) if VIDEO.contains(&e.as_str()) => MediaKind::Video,
        Some(e) if SUBTITLE.contains(&e.as_str()) => MediaKind::Subtitle,
        Some(e) if AUDIO.contains(&e.as_str()) => MediaKind::Audio,
        _ => MediaKind::Other,
    }
}

/// Subtitles next to `video` that share its base name: `Movie.srt`, `Movie.en.srt`,
/// `Movie.de.forced.ass`. Matching is case-insensitive. Sorted by name.
pub fn find_sidecar_subtitles<'a>(video: &str, siblings: &'a [Entry]) -> Vec<&'a Entry> {
    let Some((stem, _)) = video.rsplit_once('.') else {
        return vec![];
    };
    let stem = stem.to_lowercase();
    let mut out: Vec<&Entry> = siblings
        .iter()
        .filter(|e| !e.is_dir && media_kind(&e.name) == MediaKind::Subtitle)
        .filter(|e| {
            let n = e.name.to_lowercase();
            n.strip_prefix(&stem)
                .is_some_and(|rest| rest.starts_with('.'))
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VPath;

    fn f(name: &str) -> Entry {
        Entry {
            path: VPath::new(format!("/m/{name}")),
            name: name.into(),
            is_dir: false,
            size: 1,
            mtime: 0,
        }
    }

    #[test]
    fn kinds() {
        assert_eq!(media_kind("A.MKV"), MediaKind::Video);
        assert_eq!(media_kind("a.en.srt"), MediaKind::Subtitle);
        assert_eq!(media_kind(".mkv"), MediaKind::Other);
        assert_eq!(media_kind("readme"), MediaKind::Other);
    }

    #[test]
    fn sidecars() {
        let s = [
            f("Movie.mkv"),
            f("Movie.srt"),
            f("movie.EN.srt"),
            f("Movie.de.forced.ass"),
            f("Movie 2.srt"),
            f("Moviex.srt"),
            f("Other.srt"),
        ];
        let names: Vec<_> = find_sidecar_subtitles("Movie.mkv", &s)
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, ["Movie.de.forced.ass", "Movie.srt", "movie.EN.srt"]);
    }
}
