//! Fanart.tv (issue #12, SPEC 7 "Providers"): artist thumbs, backgrounds and logos, and album
//! covers and disc art, all in one request per artist by MusicBrainz id. Optional: only with the
//! user's own key, sent in the `api-key` header (never in a URL, so it cannot reach a log). Of
//! several images of a kind the most liked one is taken; for logos, one in the user's language
//! or without text first.

use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use serde_json::Value;

use crate::music::{AlbumArt, ArtistExtras, MusicProvider};
use crate::musicbrainz::{Limiter, user_agent};
use crate::{MetaError, Result};

pub const API_BASE: &str = "https://webservice.fanart.tv/v3";
pub const INTERVAL: Duration = Duration::from_millis(1000);
/// An artist every key can look up, for checking a key (Coldplay).
pub const PROBE_ARTIST: &str = "cc197bad-dc9c-440d-a5b5-d52ba2e14234";

pub struct FanartTv {
    http: reqwest::Client,
    base: String,
    key: String,
    language: String,
    limiter: Limiter,
}

impl FanartTv {
    /// `language`: the UI language ("de-DE"), for logos with text.
    pub fn new(version: &str, key: &str, language: &str) -> Result<Self> {
        Self::with_base(version, key, language, API_BASE, INTERVAL)
    }

    pub fn with_base(
        version: &str,
        key: &str,
        language: &str,
        base: &str,
        interval: Duration,
    ) -> Result<Self> {
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
            language: language
                .split(['-', '_'])
                .next()
                .unwrap_or("en")
                .to_ascii_lowercase(),
            limiter: Limiter::new(interval),
        })
    }

    /// `None` for an id Fanart.tv does not know.
    async fn get(&self, path: &str) -> Result<Option<Value>> {
        self.limiter.wait().await;
        let res = self
            .http
            .get(format!("{}{path}", self.base))
            .header("api-key", &self.key)
            .send()
            .await
            .map_err(|e| MetaError::Network(e.without_url().to_string()))?;
        match res.status().as_u16() {
            200 => res
                .json()
                .await
                .map(Some)
                .map_err(|e| MetaError::Other(e.without_url().to_string())),
            // "Not found" comes as 400 or 404.
            400 | 404 => Ok(None),
            401 | 403 => Err(MetaError::Auth),
            429 => Err(MetaError::RateLimited(60)),
            s => Err(MetaError::Network(format!("Fanart.tv answered HTTP {s}"))),
        }
    }
}

fn likes(e: &Value) -> i64 {
    match e.get("likes") {
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        _ => 0,
    }
}

/// The most liked image of a kind; with `lang`, images in that language or without text
/// ("00", none) come first, then English.
fn best(v: &Value, kind: &str, lang: Option<&str>) -> Option<String> {
    let list = v.get(kind)?.as_array()?;
    let rank = |e: &Value| -> i32 {
        let Some(want) = lang else { return 0 };
        match e.get("lang").and_then(Value::as_str).unwrap_or("") {
            l if l == want => 0,
            "" | "00" => 0,
            "en" => 1,
            _ => 2,
        }
    };
    list.iter()
        .filter(|e| e.get("url").and_then(Value::as_str).is_some())
        .min_by_key(|e| (rank(e), -likes(e)))
        .and_then(|e| e.get("url").and_then(Value::as_str))
        .map(str::to_string)
}

/// A `/music/{artist}` answer.
pub fn parse_artist(v: &Value, language: &str) -> ArtistExtras {
    let mut albums: Vec<(String, AlbumArt)> = v
        .get("albums")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(group, a)| {
                    (
                        group.clone(),
                        AlbumArt {
                            cover: best(a, "albumcover", None),
                            disc: best(a, "cdart", None),
                        },
                    )
                })
                .filter(|(_, a)| a.cover.is_some() || a.disc.is_some())
                .collect()
        })
        .unwrap_or_default();
    albums.sort_by(|a, b| a.0.cmp(&b.0));
    ArtistExtras {
        image: best(v, "artistthumb", None),
        background: best(v, "artistbackground", None),
        logo: best(v, "hdmusiclogo", Some(language))
            .or_else(|| best(v, "musiclogo", Some(language))),
        biographies: vec![],
        albums,
    }
}

#[async_trait]
impl MusicProvider for FanartTv {
    async fn artist_extras(&self, artist: &str) -> Result<Option<ArtistExtras>> {
        Ok(self
            .get(&format!("/music/{artist}"))
            .await?
            .map(|v| parse_artist(&v, &self.language)))
    }

    async fn album_art(&self, group: &str) -> Result<Option<AlbumArt>> {
        let Some(v) = self.get(&format!("/music/albums/{group}")).await? else {
            return Ok(None);
        };
        Ok(parse_artist(&v, &self.language)
            .albums
            .into_iter()
            .find(|(g, _)| g == group)
            .map(|(_, a)| a))
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
        self.get(&format!("/music/{PROBE_ARTIST}"))
            .await
            .map(|_| ())
    }
}
