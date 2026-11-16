//! TheAudioDB (issue #12, SPEC 7 "Providers"): artist images, biographies in several languages,
//! and album covers and disc art, by MusicBrainz id. Optional: only with the user's own key, which
//! the v1 API takes as a path segment. The key therefore never goes into an error message or a
//! log line: errors drop the URL, and nothing here logs one. Free keys allow 30 requests a minute;
//! one limiter per client keeps below that.

use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use serde_json::Value;

use crate::music::{AlbumArt, ArtistExtras, MusicProvider};
use crate::musicbrainz::{Limiter, user_agent};
use crate::{MetaError, Result};

pub const API_BASE: &str = "https://www.theaudiodb.com/api/v1/json";
/// 30 requests a minute on a free key; a little slower than that.
pub const INTERVAL: Duration = Duration::from_millis(2100);
/// An artist every key can look up, for checking a key (Coldplay).
pub const PROBE_ARTIST: &str = "cc197bad-dc9c-440d-a5b5-d52ba2e14234";

/// TheAudioDB's language suffixes of `strBiography…`, and the ISO 639-1 codes they mean.
const LANGUAGES: &[(&str, &str)] = &[
    ("DE", "de"),
    ("FR", "fr"),
    ("CN", "zh"),
    ("IT", "it"),
    ("JP", "ja"),
    ("RU", "ru"),
    ("ES", "es"),
    ("PT", "pt"),
    ("SE", "sv"),
    ("NL", "nl"),
    ("HU", "hu"),
    ("NO", "no"),
    ("IL", "he"),
    ("PL", "pl"),
];

pub struct TheAudioDb {
    http: reqwest::Client,
    base: String,
    key: String,
    limiter: Limiter,
}

impl TheAudioDb {
    pub fn new(version: &str, key: &str) -> Result<Self> {
        Self::with_base(version, key, API_BASE, INTERVAL)
    }

    /// For tests: another server and pace.
    pub fn with_base(version: &str, key: &str, base: &str, interval: Duration) -> Result<Self> {
        crate::tmdb::install_crypto_provider();
        let http = reqwest::Client::builder()
            .user_agent(user_agent(version))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| MetaError::Other(e.without_url().to_string()))?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            key: key.trim().to_string(),
            limiter: Limiter::new(interval),
        })
    }

    async fn get(&self, endpoint: &str, id: &str) -> Result<Value> {
        self.limiter.wait().await;
        let url = format!("{}/{}/{endpoint}", self.base, self.key);
        let res = self
            .http
            .get(&url)
            .query(&[("i", id)])
            .send()
            .await
            .map_err(|e| MetaError::Network(e.without_url().to_string()))?;
        match res.status().as_u16() {
            200 => res
                .json()
                .await
                .map_err(|e| MetaError::Other(e.without_url().to_string())),
            // An unknown key: TheAudioDB answers 404 with an HTML page.
            401 | 403 | 404 => Err(MetaError::Auth),
            429 => Err(MetaError::RateLimited(60)),
            s => Err(MetaError::Network(format!("TheAudioDB answered HTTP {s}"))),
        }
    }
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// An artist object of `artist-mb.php`.
pub fn parse_artist(a: &Value) -> ArtistExtras {
    let mut bios = Vec::new();
    // English is `strBiography` (the documentation also names `strBiographyEN`).
    if let Some(t) = text(a, "strBiography").or_else(|| text(a, "strBiographyEN")) {
        bios.push(("en".to_string(), t));
    }
    for (suffix, code) in LANGUAGES {
        if let Some(t) = text(a, &format!("strBiography{suffix}")) {
            bios.push((code.to_string(), t));
        }
    }
    ArtistExtras {
        image: text(a, "strArtistThumb"),
        background: text(a, "strArtistFanart").or_else(|| text(a, "strArtistWideThumb")),
        logo: text(a, "strArtistLogo"),
        biographies: bios,
        albums: vec![],
    }
}

/// An album object of `album-mb.php`.
pub fn parse_album(a: &Value) -> AlbumArt {
    AlbumArt {
        cover: text(a, "strAlbumThumbHQ").or_else(|| text(a, "strAlbumThumb")),
        disc: text(a, "strAlbumCDart"),
    }
}

#[async_trait]
impl MusicProvider for TheAudioDb {
    async fn artist_extras(&self, artist: &str) -> Result<Option<ArtistExtras>> {
        let v = self.get("artist-mb.php", artist).await?;
        Ok(v.get("artists")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .map(parse_artist))
    }

    async fn album_art(&self, group: &str) -> Result<Option<AlbumArt>> {
        let v = self.get("album-mb.php", group).await?;
        Ok(v.get("album")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .map(parse_album)
            .filter(|a| a.cover.is_some() || a.disc.is_some()))
    }

    async fn image(&self, url: &str) -> Result<Bytes> {
        let res = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| MetaError::Network(e.without_url().to_string()))?;
        if !res.status().is_success() {
            return Err(MetaError::NotFound);
        }
        res.bytes()
            .await
            .map_err(|e| MetaError::Network(e.without_url().to_string()))
    }

    async fn validate(&self) -> Result<()> {
        self.get("artist-mb.php", PROBE_ARTIST).await.map(|_| ())
    }
}
