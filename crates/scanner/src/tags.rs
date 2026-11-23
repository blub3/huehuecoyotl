//! Audio tags and properties (issue #7, SPEC 7 "Music library", scan step 2), read with `lofty`
//! through `Source::read_at`: only the blocks lofty asks for are fetched, never whole files.
//!
//! lofty reads synchronously, the sources asynchronously. So lofty parses a sparse in-memory view
//! of the file; when it touches a block that is not loaded yet, the read fails, the block is
//! fetched and the parse starts again. Header and tag blocks are a few, so this converges quickly.
//! Ranges already read for the file's fingerprint (its first and last 64 KiB) are reused.

use std::collections::BTreeMap;
use std::io::{self, Read, Seek, SeekFrom};

use huehuecoyotl_vfs::{Source, VPath};
use lofty::config::{ParseOptions, ParsingMode};
use lofty::file::{FileType, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::prelude::{Accessor, AudioFile, ItemKey};
use lofty::probe::Probe;
use lofty::tag::Tag;

const BLOCK: u64 = 64 * 1024;
/// A parse that keeps asking for new blocks after this many is given up (a broken file).
const MAX_FETCHES: usize = 96;

/// What the scanner keeps of one audio file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackTags {
    pub title: Option<String>,
    /// In order; several values when the tag holds several (`ARTISTS`, repeated fields), or one
    /// credit split at "feat." (with the join phrase kept).
    pub artists: Vec<Credit>,
    pub album: Option<String>,
    pub album_artists: Vec<Credit>,
    pub track_no: Option<u32>,
    pub track_total: Option<u32>,
    pub disc_no: Option<u32>,
    pub disc_total: Option<u32>,
    /// As tagged: "2024", "2024-03" or "2024-03-15".
    pub date: Option<String>,
    pub year: Option<u16>,
    pub genre: Option<String>,
    pub compilation: bool,
    pub release_type: Option<String>,
    pub mb_release: Option<String>,
    pub mb_release_group: Option<String>,
    pub mb_recording: Option<String>,
    /// Issue #153: the track on the release, its ISRCs and the release's barcode.
    pub mb_track: Option<String>,
    pub isrc: Vec<String>,
    pub barcode: Option<String>,
    pub mb_artists: Vec<String>,
    pub mb_album_artists: Vec<String>,
    pub rg_track_gain: Option<f64>,
    pub rg_track_peak: Option<f64>,
    pub rg_album_gain: Option<f64>,
    pub rg_album_peak: Option<f64>,
    pub audio: AudioInfo,
    /// The front cover (or the first picture), when the tags hold one.
    pub cover: Option<Vec<u8>>,
    /// Audiobooks (2.5): the narrators (composer), the series and the position in it (movement
    /// name and number, or `SERIES` and `SERIES-PART`), publisher, description, language, ISBN
    /// and ASIN.
    pub book: BookTags,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BookTags {
    pub narrators: Vec<String>,
    pub series: Option<String>,
    pub series_part: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub asin: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Credit {
    pub name: String,
    /// What follows the name before the next one ("feat.", "&"), "" for the last.
    pub join: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioInfo {
    /// flac, alac, pcm, mp3, aac, vorbis, opus, wavpack
    pub codec: String,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
    pub lossless: bool,
    /// kbit/s
    pub bitrate: Option<u32>,
    pub duration_ms: u64,
}

/// The result of reading one file, and how many bytes it took.
#[derive(Debug)]
pub struct Read_ {
    pub tags: TrackTags,
    pub bytes_read: u64,
}

/// A file as far as it has been fetched: ranges by their start (fetched blocks are aligned to
/// 64 KiB, reused ranges need not be). A read of a missing part fails and remembers its block.
struct Sparse<'a> {
    size: u64,
    pos: u64,
    ranges: &'a BTreeMap<u64, Vec<u8>>,
    missing: &'a std::cell::Cell<Option<u64>>,
}

impl Read for Sparse<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.size || buf.is_empty() {
            return Ok(0);
        }
        let hit = self
            .ranges
            .range(..=self.pos)
            .next_back()
            .filter(|(start, data)| self.pos < **start + data.len() as u64);
        let Some((start, data)) = hit else {
            self.missing.set(Some(self.pos - self.pos % BLOCK));
            return Err(io::Error::other("block not loaded"));
        };
        let off = (self.pos - start) as usize;
        let n = buf.len().min(data.len() - off);
        buf[..n].copy_from_slice(&data[off..off + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Sparse<'_> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let to = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(d) => self.size as i128 + d as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if to < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before start",
            ));
        }
        self.pos = to as u64;
        Ok(self.pos)
    }
}

/// Read the tags and audio properties of `path` (`size` bytes). The extension names the format,
/// so a file is never guessed into a wrong type.
pub async fn read(src: &dyn Source, path: &VPath, size: u64) -> Result<Read_, String> {
    read_seeded(src, path, size, Vec::new()).await
}

/// As [`read`], starting from ranges of the file that are already in memory ((offset, bytes)).
/// They are not counted in `bytes_read`.
pub async fn read_seeded(
    src: &dyn Source,
    path: &VPath,
    size: u64,
    seed: Vec<(u64, Vec<u8>)>,
) -> Result<Read_, String> {
    let name = path.as_str().rsplit('/').next().unwrap_or("");
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let file_type = ext
        .as_deref()
        .and_then(FileType::from_ext)
        .ok_or_else(|| format!("not an audio file: {name}"))?;
    let mut blocks: BTreeMap<u64, Vec<u8>> =
        seed.into_iter().filter(|(_, d)| !d.is_empty()).collect();
    let mut bytes_read = 0u64;
    // Most formats keep their tags at the start; ID3v1 and APE tags sit at the end.
    let mut want = if blocks.is_empty() {
        vec![0, size.saturating_sub(1) - size.saturating_sub(1) % BLOCK]
    } else {
        vec![]
    };
    for _ in 0..MAX_FETCHES {
        for start in want.drain(..) {
            if start < size && !blocks.contains_key(&start) {
                let len = BLOCK.min(size - start) as u32;
                let data = src
                    .read_at(path, start, len)
                    .await
                    .map_err(|e| e.to_string())?;
                bytes_read += data.len() as u64;
                blocks.insert(start, data.to_vec());
            }
        }
        let missing = std::cell::Cell::new(None);
        let parsed = {
            let reader = Sparse {
                size,
                pos: 0,
                ranges: &blocks,
                missing: &missing,
            };
            Probe::with_file_type(reader, file_type)
                .options(
                    ParseOptions::new()
                        .read_properties(true)
                        .read_cover_art(true)
                        .parsing_mode(ParsingMode::Relaxed),
                )
                .read()
        };
        match missing.get() {
            // lofty may swallow a failed read and carry on: whatever it touched is fetched and
            // the parse runs again, so the result never rests on a missing block.
            Some(start) => want.push(start),
            None => {
                let file = parsed.map_err(|e| e.to_string())?;
                return Ok(Read_ {
                    tags: extract(&file, file_type),
                    bytes_read,
                });
            }
        }
    }
    Err(format!("{name}: gave up after {MAX_FETCHES} reads"))
}

fn strings(tag: &Tag, key: ItemKey) -> Vec<String> {
    tag.get_strings(key)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn first(tag: &Tag, key: ItemKey) -> Option<String> {
    strings(tag, key).into_iter().next()
}

/// Several artists: one per value; a single value is split at "feat." and its variants, which
/// never occur inside a band name. "&", "and" and "," are left alone ("Simon & Garfunkel").
pub fn credits(values: &[String]) -> Vec<Credit> {
    if values.len() > 1 {
        let n = values.len();
        return values
            .iter()
            .enumerate()
            .map(|(i, v)| Credit {
                name: v.clone(),
                join: if i + 1 < n {
                    if i + 2 == n {
                        " & ".into()
                    } else {
                        ", ".into()
                    }
                } else {
                    String::new()
                },
            })
            .collect();
    }
    let Some(one) = values.first() else {
        return vec![];
    };
    for sep in [
        " feat. ",
        " featuring ",
        " ft. ",
        " Feat. ",
        " Ft. ",
        " FEAT. ",
    ] {
        if let Some((a, b)) = one.split_once(sep) {
            let mut out = vec![Credit {
                name: a.trim().to_string(),
                join: sep.to_string(),
            }];
            out.extend(credits(&[b.trim().to_string()]));
            return out;
        }
    }
    vec![Credit {
        name: one.clone(),
        join: String::new(),
    }]
}

fn gain(tag: &Tag, key: ItemKey) -> Option<f64> {
    first(tag, key).and_then(|v| {
        v.trim_end_matches("dB")
            .trim_end_matches("db")
            .trim()
            .parse()
            .ok()
    })
}

pub(crate) fn extract(file: &lofty::file::TaggedFile, file_type: FileType) -> TrackTags {
    let props = file.properties();
    let bit_depth = props.bit_depth();
    let (codec, lossless) = match file_type {
        FileType::Flac => ("flac", true),
        // ALAC has a bit depth, AAC does not.
        FileType::Mp4 if bit_depth.is_some() => ("alac", true),
        FileType::Mp4 | FileType::Aac => ("aac", false),
        FileType::Mpeg => ("mp3", false),
        FileType::Vorbis => ("vorbis", false),
        FileType::Opus => ("opus", false),
        FileType::Wav | FileType::Aiff => ("pcm", true),
        // WavPack's hybrid lossy mode is rare; the bit depth says it is PCM-like.
        FileType::WavPack => ("wavpack", true),
        _ => ("other", false),
    };
    let audio = AudioInfo {
        codec: codec.into(),
        sample_rate: props.sample_rate(),
        bit_depth,
        channels: props.channels(),
        lossless,
        bitrate: props.audio_bitrate().or(props.overall_bitrate()),
        duration_ms: props.duration().as_millis() as u64,
    };
    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else {
        return TrackTags {
            audio,
            ..Default::default()
        };
    };
    let artists = {
        let many = strings(tag, ItemKey::TrackArtists);
        if many.is_empty() {
            strings(tag, ItemKey::TrackArtist)
        } else {
            many
        }
    };
    let album_artists = {
        let many = strings(tag, ItemKey::AlbumArtists);
        if many.is_empty() {
            strings(tag, ItemKey::AlbumArtist)
        } else {
            many
        }
    };
    let date = first(tag, ItemKey::ReleaseDate)
        .or_else(|| first(tag, ItemKey::RecordingDate))
        .or_else(|| first(tag, ItemKey::Year))
        .or_else(|| tag.date().map(|t| t.year.to_string()));
    let year = date
        .as_deref()
        .and_then(|d| d.get(..4))
        .and_then(|y| y.parse().ok())
        .filter(|y: &u16| *y > 1000);
    let compilation = first(tag, ItemKey::FlagCompilation)
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    let pictures = tag.pictures();
    let cover = pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())
        .map(|p| p.data().to_vec());
    TrackTags {
        title: tag
            .title()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty()),
        artists: credits(&artists),
        album: tag
            .album()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty()),
        album_artists: credits(&album_artists),
        track_no: tag.track(),
        track_total: tag.track_total(),
        disc_no: tag.disk(),
        disc_total: tag.disk_total(),
        date,
        year,
        genre: tag
            .genre()
            .map(|g| g.trim().to_string())
            .filter(|g| !g.is_empty()),
        compilation,
        release_type: first(tag, ItemKey::MusicBrainzReleaseType).map(|t| t.to_lowercase()),
        mb_release: first(tag, ItemKey::MusicBrainzReleaseId),
        mb_release_group: first(tag, ItemKey::MusicBrainzReleaseGroupId),
        mb_recording: first(tag, ItemKey::MusicBrainzRecordingId),
        mb_track: first(tag, ItemKey::MusicBrainzTrackId),
        isrc: strings(tag, ItemKey::Isrc),
        barcode: first(tag, ItemKey::Barcode),
        mb_artists: strings(tag, ItemKey::MusicBrainzArtistId),
        mb_album_artists: strings(tag, ItemKey::MusicBrainzReleaseArtistId),
        rg_track_gain: gain(tag, ItemKey::ReplayGainTrackGain),
        rg_track_peak: gain(tag, ItemKey::ReplayGainTrackPeak),
        rg_album_gain: gain(tag, ItemKey::ReplayGainAlbumGain),
        rg_album_peak: gain(tag, ItemKey::ReplayGainAlbumPeak),
        audio,
        cover,
        book: BookTags {
            narrators: strings(tag, ItemKey::Composer),
            // lofty's generic tag carries no free-form keys such as SERIES; the movement fields
            // (MP4 ©mvn/©mvi, ID3 MVNM/MVIN, Vorbis MOVEMENTNAME/MOVEMENT) are the common carriers.
            series: first(tag, ItemKey::Movement),
            series_part: first(tag, ItemKey::MovementNumber),
            publisher: first(tag, ItemKey::Publisher).or_else(|| first(tag, ItemKey::Label)),
            description: first(tag, ItemKey::Description)
                .or_else(|| first(tag, ItemKey::Comment))
                .filter(|d| d.len() > 20),
            language: first(tag, ItemKey::Language),
            isbn: None,
            asin: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_split_only_where_safe() {
        let c = credits(&["Simon & Garfunkel".into()]);
        assert_eq!(c.len(), 1);
        let c = credits(&["Daft Punk feat. Pharrell Williams".into()]);
        assert_eq!(
            c,
            [
                Credit {
                    name: "Daft Punk".into(),
                    join: " feat. ".into()
                },
                Credit {
                    name: "Pharrell Williams".into(),
                    join: String::new()
                }
            ]
        );
        let c = credits(&["A".into(), "B".into(), "C".into()]);
        assert_eq!(
            c.iter()
                .map(|c| (c.name.as_str(), c.join.as_str()))
                .collect::<Vec<_>>(),
            [("A", ", "), ("B", " & "), ("C", "")]
        );
        assert!(credits(&[]).is_empty());
    }
}
