//! Writing canonical tags into a managed copy (3.0, issue #153, SPEC 7), with `lofty`: Vorbis
//! comments (FLAC, Ogg Vorbis, Opus), ID3v2.4 (MP3) and MP4 atoms (M4A, M4B). A field with no
//! value is not touched, so technical tags such as ReplayGain, and everything else in the file
//! the app has no opinion on, stay. Audio data and embedded chapters are not rewritten.

use std::io::Cursor;
use std::path::Path;

use huehuecoyotl_library::copy_meta::CopyTags;
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::{Picture, PictureType};
use lofty::prelude::{Accessor, ItemKey};
use lofty::probe::Probe;
use lofty::tag::{ItemValue, Tag, TagItem};

/// Why a copy could not be tagged.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct WriteError(String);

fn fail(e: impl std::fmt::Display) -> WriteError {
    WriteError(e.to_string())
}

fn set_one(tag: &mut Tag, key: ItemKey, value: &str) {
    if !value.is_empty() {
        tag.remove_key(key);
        tag.insert_text(key, value.to_string());
    }
}

fn set_many(tag: &mut Tag, key: ItemKey, values: &[String]) {
    if values.is_empty() {
        return;
    }
    tag.remove_key(key);
    for v in values {
        tag.push(TagItem::new(key, ItemValue::Text(v.clone())));
    }
}

/// Write `t` (and a front cover, when given) into the file at `path`.
pub fn write_tags(path: &Path, t: &CopyTags, cover: Option<&[u8]>) -> Result<(), WriteError> {
    let mut file = Probe::open(path)
        .map_err(fail)?
        .guess_file_type()
        .map_err(fail)?
        .read()
        .map_err(fail)?;
    let kind = file.primary_tag_type();
    if file.tag(kind).is_none() {
        file.insert_tag(Tag::new(kind));
    }
    let tag = file
        .tag_mut(kind)
        .ok_or_else(|| fail("this file type cannot carry tags"))?;
    if !t.title.is_empty() {
        tag.set_title(t.title.clone());
    }
    if !t.album.is_empty() {
        tag.set_album(t.album.clone());
    }
    if !t.genre.is_empty() {
        tag.set_genre(t.genre.clone());
    }
    // The list fields go first: readers prefer them to the single one.
    if !t.artists.is_empty() {
        tag.remove_key(ItemKey::TrackArtists);
    }
    if !t.album_artists.is_empty() {
        tag.remove_key(ItemKey::AlbumArtists);
    }
    set_many(tag, ItemKey::TrackArtist, &t.artists);
    set_many(tag, ItemKey::AlbumArtist, &t.album_artists);
    if !t.date.is_empty() {
        tag.remove_key(ItemKey::Year);
        tag.remove_key(ItemKey::ReleaseDate);
        set_one(tag, ItemKey::RecordingDate, &t.date);
    }
    if let Some(n) = t.track {
        tag.set_track(n);
    }
    if let Some(n) = t.track_total {
        tag.set_track_total(n);
    }
    if let Some(n) = t.disc {
        tag.set_disk(n);
    }
    if let Some(n) = t.disc_total {
        tag.set_disk_total(n);
    }
    if t.compilation {
        set_one(tag, ItemKey::FlagCompilation, "1");
    }
    set_many(tag, ItemKey::MusicBrainzArtistId, &t.mb_artists);
    set_many(
        tag,
        ItemKey::MusicBrainzReleaseArtistId,
        &t.mb_album_artists,
    );
    set_one(tag, ItemKey::MusicBrainzReleaseGroupId, &t.mb_release_group);
    set_one(tag, ItemKey::MusicBrainzReleaseId, &t.mb_release);
    set_one(tag, ItemKey::MusicBrainzRecordingId, &t.mb_recording);
    set_one(tag, ItemKey::MusicBrainzTrackId, &t.mb_track);
    set_many(tag, ItemKey::Isrc, &t.isrc);
    set_one(tag, ItemKey::Barcode, &t.barcode);
    // Audiobooks: the same carriers the scanner reads them from.
    set_many(tag, ItemKey::Composer, &t.narrators);
    set_one(tag, ItemKey::Movement, &t.series);
    set_one(tag, ItemKey::MovementNumber, &t.series_part);
    if let Some(bytes) = cover {
        let mut p = Picture::from_reader(&mut Cursor::new(bytes)).map_err(fail)?;
        p.set_pic_type(PictureType::CoverFront);
        tag.remove_picture_type(PictureType::CoverFront);
        tag.push_picture(p);
    }
    file.save_to_path(path, WriteOptions::default())
        .map_err(fail)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::TrackTags;
    use std::process::Command;

    fn ffmpeg(args: &[&str]) -> bool {
        Command::new("ffmpeg")
            .args(["-nostdin", "-loglevel", "error", "-y"])
            .args(args)
            .status()
            .is_ok_and(|s| s.success())
    }

    fn tags() -> CopyTags {
        CopyTags {
            title: "SAOKO".into(),
            artists: vec!["Rosalía".into()],
            album: "MOTOMAMI".into(),
            album_artists: vec!["Rosalía".into()],
            date: "2022-03-18".into(),
            track: Some(3),
            track_total: Some(16),
            disc: Some(1),
            disc_total: Some(2),
            genre: "Pop".into(),
            mb_artists: vec!["a1111111-1111-4111-8111-111111111111".into()],
            mb_album_artists: vec!["a1111111-1111-4111-8111-111111111111".into()],
            mb_release_group: "b2222222-2222-4222-8222-222222222222".into(),
            mb_release: "c3333333-3333-4333-8333-333333333333".into(),
            mb_recording: "d4444444-4444-4444-8444-444444444444".into(),
            mb_track: "e5555555-5555-4555-8555-555555555555".into(),
            isrc: vec!["ESA012200001".into()],
            barcode: "0194399999999".into(),
            ..Default::default()
        }
    }

    fn back(path: &Path) -> (TrackTags, Tag) {
        let f = Probe::open(path)
            .unwrap()
            .guess_file_type()
            .unwrap()
            .read()
            .unwrap();
        let tag = f.primary_tag().unwrap().clone();
        (crate::tags::extract(&f, f.file_type()), tag)
    }

    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 1, 0,
        0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 13, b'I', b'D', b'A', b'T', 0x78,
        0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0, 0, 3, 1, 1, 0, 0x18, 0xdd, 0x8d, 0xb0, 0, 0, 0, 0, b'I',
        b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn every_format_round_trips() {
        let d = tempfile::tempdir().unwrap();
        for (ext, codec) in [
            ("flac", "flac"),
            ("ogg", "libvorbis"),
            ("opus", "libopus"),
            ("mp3", "libmp3lame"),
            ("m4a", "aac"),
        ] {
            let p = d.path().join(format!("a.{ext}"));
            let ps = p.to_str().unwrap();
            if !ffmpeg(&[
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1:sample_rate=48000",
                "-c:a",
                codec,
                "-metadata",
                "REPLAYGAIN_TRACK_GAIN=-3.21 dB",
                ps,
            ]) {
                eprintln!("skipped {ext}: this ffmpeg cannot write it");
                continue;
            }
            write_tags(&p, &tags(), Some(PNG)).unwrap_or_else(|e| panic!("{ext}: {e}"));
            let (t, raw) = back(&p);
            assert_eq!(t.title.as_deref(), Some("SAOKO"), "{ext}");
            assert_eq!(t.album.as_deref(), Some("MOTOMAMI"), "{ext}");
            assert_eq!(t.artists[0].name, "Rosalía", "{ext}");
            assert_eq!(t.album_artists[0].name, "Rosalía", "{ext}");
            assert_eq!(
                (t.track_no, t.track_total, t.disc_no, t.disc_total),
                (Some(3), Some(16), Some(1), Some(2)),
                "{ext}"
            );
            assert_eq!(t.year, Some(2022), "{ext}");
            assert_eq!(
                t.mb_release.as_deref(),
                Some(tags().mb_release.as_str()),
                "{ext}"
            );
            assert_eq!(
                t.mb_release_group.as_deref(),
                Some(tags().mb_release_group.as_str()),
                "{ext}"
            );
            assert_eq!(
                t.mb_recording.as_deref(),
                Some(tags().mb_recording.as_str()),
                "{ext}"
            );
            assert_eq!(
                t.mb_track.as_deref(),
                Some(tags().mb_track.as_str()),
                "{ext}"
            );
            assert_eq!(t.mb_artists, tags().mb_artists, "{ext}");
            assert_eq!(
                t.isrc.first().map(String::as_str),
                Some("ESA012200001"),
                "{ext}"
            );
            assert_eq!(t.barcode.as_deref(), Some("0194399999999"), "{ext}");
            assert_eq!(t.cover.as_deref(), Some(PNG), "{ext}");
            let _ = raw;
        }
    }

    #[test]
    fn empty_fields_leave_the_file_alone() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.flac");
        if !ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "sine=duration=1",
            "-metadata",
            "title=Mine",
            "-metadata",
            "REPLAYGAIN_TRACK_GAIN=-3.21 dB",
            p.to_str().unwrap(),
        ]) {
            return;
        }
        write_tags(
            &p,
            &CopyTags {
                album: "New".into(),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        let (t, _) = back(&p);
        assert_eq!(
            t.title.as_deref(),
            Some("Mine"),
            "a locked or empty field stays"
        );
        assert_eq!(t.album.as_deref(), Some("New"));
        assert!(t.rg_track_gain.is_some(), "ReplayGain stays");
    }

    fn chapters(p: &Path) -> usize {
        let o = Command::new("ffprobe")
            .args(["-v", "error", "-show_chapters", "-of", "csv=p=0"])
            .arg(p)
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).lines().count()
    }

    #[test]
    fn a_one_file_audiobook_keeps_its_chapters() {
        let d = tempfile::tempdir().unwrap();
        for ext in ["m4b", "mp3"] {
            let meta = d.path().join("ch.txt");
            std::fs::write(&meta, ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1000\ntitle=One\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=1000\nEND=2000\ntitle=Two\n").unwrap();
            let p = d.path().join(format!("b.{ext}"));
            let codec = if ext == "m4b" { "aac" } else { "libmp3lame" };
            let mut args = vec![
                "-f",
                "lavfi",
                "-i",
                "sine=duration=2",
                "-i",
                meta.to_str().unwrap(),
                "-map_metadata",
                "1",
                "-map_chapters",
                "1",
                "-c:a",
                codec,
            ];
            if ext == "m4b" {
                args.extend(["-f", "ipod"]);
            }
            args.push(p.to_str().unwrap());
            if !ffmpeg(&args) || chapters(&p) != 2 {
                eprintln!("skipped {ext}");
                continue;
            }
            let t = CopyTags {
                title: "Emma".into(),
                narrators: vec!["Reader".into()],
                series: "Austen".into(),
                series_part: "2".into(),
                ..Default::default()
            };
            write_tags(&p, &t, None).unwrap();
            assert_eq!(chapters(&p), 2, "{ext}: chapters survive");
            let (b, _) = back(&p);
            assert_eq!(b.title.as_deref(), Some("Emma"), "{ext}");
            assert_eq!(b.book.narrators, ["Reader"], "{ext}");
            assert_eq!(b.book.series.as_deref(), Some("Austen"), "{ext}");
            assert_eq!(b.book.series_part.as_deref(), Some("2"), "{ext}");
        }
    }
}
