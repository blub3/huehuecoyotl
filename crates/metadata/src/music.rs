//! Music metadata (2.0, SPEC 7 "Providers"): the `MusicProvider` trait, what it returns, and how
//! a search result becomes a match (issue #11). Providers fill in what tags lack; the decision
//! here never matches an album to a release that disagrees with it.

use async_trait::async_trait;
use bytes::Bytes;

use crate::Result;

/// One artist of a credit, with what joins it to the next ("feat.", " & ").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArtistCredit {
    pub name: String,
    pub id: String,
    pub join: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    /// 1-based position on its medium.
    pub position: u32,
    pub title: String,
    pub recording_id: String,
    pub length_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Medium {
    /// 1-based disc number.
    pub position: u32,
    pub tracks: Vec<Track>,
}

/// A release as one lookup returns it: artists, release group and track list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Release {
    pub id: String,
    pub title: String,
    /// "1959", "1959-08" or "1959-08-17".
    pub date: Option<String>,
    pub artists: Vec<ArtistCredit>,
    pub group_id: Option<String>,
    /// album, single, ep, broadcast, other (lower case); `compilation` from the secondary types.
    pub release_type: Option<String>,
    pub compilation: bool,
    pub media: Vec<Medium>,
}

/// One search result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReleaseHit {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub date: Option<String>,
    pub track_count: Option<u32>,
    pub group_id: Option<String>,
    /// 0 to 100.
    pub score: u32,
    /// Issue #102: what tells editions apart: country ("DE", "XW"), the first label, the media
    /// formats ("CD", "Digital Media") and the primary type ("album").
    pub country: Option<String>,
    pub label: Option<String>,
    pub formats: Vec<String>,
    pub release_type: Option<String>,
}

/// Issue #103: a recording (a song at MusicBrainz), with the releases it appears on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recording {
    pub id: String,
    pub title: String,
    pub artists: Vec<ArtistCredit>,
    pub length_ms: Option<u64>,
    pub isrcs: Vec<String>,
    /// (release id, title, date) of the releases it is on.
    pub releases: Vec<(String, String, Option<String>)>,
    /// 0 to 100 in a search; 100 for a lookup.
    pub score: u32,
}

/// Issue #105: an artist as a search or lookup returns it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Artist {
    pub id: String,
    pub name: String,
    /// "Norwegian string quartet": what tells artists of one name apart.
    pub disambiguation: String,
    /// "Group", "Person", ...
    pub kind: String,
    pub country: Option<String>,
    /// Years active or lived: "2009", "1961".
    pub begin: Option<String>,
    pub end: Option<String>,
    pub score: u32,
}

/// Issue #12: an album's images from an optional provider (URLs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlbumArt {
    pub cover: Option<String>,
    pub disc: Option<String>,
}

/// Issue #12: what an optional provider knows about an artist (image URLs, biographies), and
/// the art of the artist's albums by release group when it comes with the artist.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArtistExtras {
    pub image: Option<String>,
    pub background: Option<String>,
    pub logo: Option<String>,
    /// (ISO 639-1 language, text); "en" for English.
    pub biographies: Vec<(String, String)>,
    pub albums: Vec<(String, AlbumArt)>,
}

impl ArtistExtras {
    /// The biography in `language` ("de-DE" or "de"), else in English.
    pub fn biography(&self, language: &str) -> Option<&str> {
        let want = language
            .split(['-', '_'])
            .next()
            .unwrap_or("en")
            .to_ascii_lowercase();
        let find = |l: &str| {
            self.biographies
                .iter()
                .find(|(k, t)| k == l && !t.trim().is_empty())
                .map(|(_, t)| t.as_str())
        };
        find(&want).or_else(|| find("en"))
    }

    /// Fill what is missing here from `other` (a provider later in the order).
    pub fn merge(&mut self, other: ArtistExtras) {
        self.image = self.image.take().or(other.image);
        self.background = self.background.take().or(other.background);
        self.logo = self.logo.take().or(other.logo);
        for (l, t) in other.biographies {
            if !self.biographies.iter().any(|(k, _)| *k == l) {
                self.biographies.push((l, t));
            }
        }
        for (g, a) in other.albums {
            match self.albums.iter_mut().find(|(k, _)| *k == g) {
                Some((_, mine)) => {
                    mine.cover = mine.cover.take().or(a.cover);
                    mine.disc = mine.disc.take().or(a.disc);
                }
                None => self.albums.push((g, a)),
            }
        }
    }
}

/// Music metadata providers. MusicBrainz and the Cover Art Archive (issue #11) do releases and
/// covers; the optional TheAudioDB and Fanart.tv (issue #12) do artist images, biographies,
/// covers for albums without one, and disc art. Each implements what it offers; the rest
/// answers "not offered" (`NotFound`, `None`).
#[async_trait]
pub trait MusicProvider: Send + Sync {
    /// A release by its id, in one request with artists, recordings and the release group.
    async fn release(&self, _id: &str) -> Result<Release> {
        Err(crate::MetaError::NotFound)
    }
    /// Releases by album artist, title and (when known) track count, best first.
    async fn search_releases(
        &self,
        _artist: &str,
        _title: &str,
        _tracks: Option<u32>,
    ) -> Result<Vec<ReleaseHit>> {
        Ok(vec![])
    }
    /// The front cover of a release, else of its release group, at most 1200 px. `None` when
    /// neither has one.
    async fn front_cover(&self, _release: &str, _group: Option<&str>) -> Result<Option<Bytes>> {
        Ok(None)
    }
    /// Issue #12: an artist by MusicBrainz artist id; `None` when the provider has nothing.
    async fn artist_extras(&self, _artist: &str) -> Result<Option<ArtistExtras>> {
        Ok(None)
    }
    /// Issue #12: an album's art by MusicBrainz release group id.
    async fn album_art(&self, _group: &str) -> Result<Option<AlbumArt>> {
        Ok(None)
    }
    /// Issue #12: download an image the provider named.
    async fn image(&self, _url: &str) -> Result<Bytes> {
        Err(crate::MetaError::NotFound)
    }
    /// Issue #12: check the key with a cheap request (`Auth` when it is rejected).
    async fn validate(&self) -> Result<()> {
        Ok(())
    }
    /// Issue #102: releases of a release group, as search hits.
    async fn group_releases(&self, _group: &str) -> Result<Vec<ReleaseHit>> {
        Ok(vec![])
    }
    /// Issue #103: recordings by title and artist, best first.
    async fn search_recordings(&self, _title: &str, _artist: &str) -> Result<Vec<Recording>> {
        Ok(vec![])
    }
    /// Issue #103: a recording by id, with its artists, ISRCs and releases.
    async fn recording(&self, _id: &str) -> Result<Recording> {
        Err(crate::MetaError::NotFound)
    }
    /// Issue #105: artists by name, best first.
    async fn search_artists(&self, _name: &str) -> Result<Vec<Artist>> {
        Ok(vec![])
    }
    /// Issue #105: an artist by id.
    async fn artist(&self, _id: &str) -> Result<Artist> {
        Err(crate::MetaError::NotFound)
    }
    /// Issue #105: the titles of an artist's release groups (albums, EPs, singles), to see
    /// which of the user's albums are theirs.
    async fn artist_groups(&self, _id: &str) -> Result<Vec<String>> {
        Ok(vec![])
    }
}

/// What a search means for an album.
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    /// Clearly this release (or an edition of it): look it up.
    Matched(String),
    /// Possible, not certain: these go to "Review matches".
    Review(Vec<ReleaseHit>),
    /// Nothing that looks like it.
    None,
}

/// Lower-case letters and digits only: "The Wall" and "the wall!" compare equal.
pub fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// A hit agrees with the album when its title and artist are the album's (any artist for
/// "Various Artists") and, when both are known, it has as many tracks.
fn agrees(h: &ReleaseHit, artist: &str, title: &str, tracks: Option<u32>) -> bool {
    let various = norm(artist) == "variousartists";
    norm(&h.title) == norm(title)
        && (various || norm(&h.artist) == norm(artist))
        && match (tracks, h.track_count) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
}

/// Decide from search results (issue #11): matched only when the hits that score 90 or more and
/// agree with the album all belong to one release group (editions of one album); then the first
/// of them. Anything else that scored 60 or more is for review; never a guess.
pub fn decide(hits: &[ReleaseHit], artist: &str, title: &str, tracks: Option<u32>) -> Decision {
    let good: Vec<&ReleaseHit> = hits
        .iter()
        .filter(|h| h.score >= 90 && agrees(h, artist, title, tracks))
        .collect();
    if let Some(first) = good.first() {
        let one_group =
            first.group_id.is_some() && good.iter().all(|h| h.group_id == first.group_id);
        if one_group {
            return Decision::Matched(first.id.clone());
        }
    }
    let review: Vec<ReleaseHit> = hits
        .iter()
        .filter(|h| h.score >= 60)
        .take(8)
        .cloned()
        .collect();
    if review.is_empty() {
        Decision::None
    } else {
        Decision::Review(review)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(
        id: &str,
        title: &str,
        artist: &str,
        tracks: u32,
        group: &str,
        score: u32,
    ) -> ReleaseHit {
        ReleaseHit {
            id: id.into(),
            title: title.into(),
            artist: artist.into(),
            track_count: Some(tracks),
            group_id: Some(group.into()),
            score,
            ..Default::default()
        }
    }

    #[test]
    fn editions_of_one_album_match() {
        let hits = [
            hit("a", "Kind of Blue", "Miles Davis", 5, "g1", 100),
            hit("b", "Kind of Blue", "Miles Davis", 5, "g1", 100),
        ];
        assert_eq!(
            decide(&hits, "Miles Davis", "Kind of Blue", Some(5)),
            Decision::Matched("a".into())
        );
    }

    #[test]
    fn never_a_confident_wrong_match() {
        // Same title by another artist; another track count; two different albums.
        let other_artist = [hit("a", "Greatest Hits", "Queen", 17, "g1", 100)];
        assert!(matches!(
            decide(&other_artist, "ABBA", "Greatest Hits", Some(17)),
            Decision::Review(_)
        ));
        let other_count = [hit("a", "Kind of Blue", "Miles Davis", 9, "g1", 100)];
        assert!(matches!(
            decide(&other_count, "Miles Davis", "Kind of Blue", Some(5)),
            Decision::Review(_)
        ));
        let two_albums = [
            hit("a", "Live", "Band", 10, "g1", 100),
            hit("b", "Live", "Band", 10, "g2", 98),
        ];
        assert!(matches!(
            decide(&two_albums, "Band", "Live", Some(10)),
            Decision::Review(_)
        ));
        assert_eq!(
            decide(&[hit("a", "x", "y", 1, "g", 40)], "Band", "Live", None),
            Decision::None
        );
        assert_eq!(decide(&[], "Band", "Live", None), Decision::None);
    }

    #[test]
    fn compilations_match_any_artist() {
        let hits = [hit("a", "Now 42", "Various Artists", 20, "g1", 95)];
        assert_eq!(
            decide(&hits, "Various Artists", "NOW 42!", Some(20)),
            Decision::Matched("a".into())
        );
    }
}
