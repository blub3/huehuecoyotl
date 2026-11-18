//! MusicBrainz and the Cover Art Archive (issue #11, SPEC 7 "Providers"). MusicBrainz asks for
//! at most about one request per second per IP and answers HTTP 503 above that; every request
//! carries a User-Agent naming the app, its real version and the project URL. One limiter per
//! client, and the app has one client, keeps the whole app to that pace; 503 backs off and retries.
//! Only MusicBrainz core data (CC0) is used. The Cover Art Archive has no rate limit.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::music::{
    Artist, ArtistCredit, Medium, MusicProvider, Recording, Release, ReleaseHit, Track,
};
use crate::{MetaError, Result};

pub const API_BASE: &str = "https://musicbrainz.org/ws/2";
pub const ART_BASE: &str = "https://coverartarchive.org";
pub const PROJECT_URL: &str = "https://github.com/blub3/huehuecoyotl";
/// A little over one second, so the average stays below one request per second.
pub const INTERVAL: Duration = Duration::from_millis(1100);
/// 503s in a row before a request fails (it is retried later as a job).
const RETRIES: u32 = 4;

/// Spaces requests at least `interval` apart, across every task that uses it.
pub struct Limiter {
    interval: Duration,
    next: Mutex<Instant>,
}

impl Limiter {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            next: Mutex::new(Instant::now()),
        }
    }

    /// Wait for the next free slot. The lock is held while waiting, so slots are handed out in
    /// order and never two at once.
    pub async fn wait(&self) {
        let mut next = self.next.lock().await;
        let now = Instant::now();
        if *next > now {
            tokio::time::sleep_until(*next).await;
        }
        *next = Instant::now() + self.interval;
    }
}

/// The User-Agent MusicBrainz asks for: `Name/version ( contact URL )`.
pub fn user_agent(version: &str) -> String {
    format!("Huehuecoyotl/{version} ( {PROJECT_URL} )")
}

pub struct MusicBrainz {
    http: reqwest::Client,
    api_base: String,
    art_base: String,
    limiter: Arc<Limiter>,
}

impl MusicBrainz {
    /// `version` is the app's version, sent in the User-Agent.
    pub fn new(version: &str) -> Result<Self> {
        Self::with_bases(version, API_BASE, ART_BASE, INTERVAL)
    }

    /// For tests: other servers and pace.
    pub fn with_bases(version: &str, api: &str, art: &str, interval: Duration) -> Result<Self> {
        crate::tmdb::install_crypto_provider();
        let http = reqwest::Client::builder()
            .user_agent(user_agent(version))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| MetaError::Other(e.to_string()))?;
        Ok(Self {
            http,
            api_base: api.trim_end_matches('/').to_string(),
            art_base: art.trim_end_matches('/').to_string(),
            limiter: Arc::new(Limiter::new(interval)),
        })
    }

    /// GET a MusicBrainz JSON resource at the limiter's pace, backing off on 503.
    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let url = format!("{}{path}", self.api_base);
        let mut backoff = Duration::from_secs(2);
        for attempt in 0..=RETRIES {
            self.limiter.wait().await;
            let res = self
                .http
                .get(&url)
                .query(query)
                .query(&[("fmt", "json")])
                .send()
                .await
                .map_err(|e| MetaError::Network(e.without_url().to_string()))?;
            match res.status().as_u16() {
                200 => {
                    return res
                        .json()
                        .await
                        .map_err(|e| MetaError::Other(e.to_string()));
                }
                404 => return Err(MetaError::NotFound),
                503 | 429 if attempt < RETRIES => {
                    let wait = res
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse().ok())
                        .map(Duration::from_secs)
                        .unwrap_or(backoff);
                    tracing::info!(?wait, "MusicBrainz is busy; waiting");
                    tokio::time::sleep(wait).await;
                    backoff *= 2;
                }
                503 | 429 => return Err(MetaError::RateLimited(60)),
                s => return Err(MetaError::Network(format!("MusicBrainz answered HTTP {s}"))),
            }
        }
        Err(MetaError::RateLimited(60))
    }
}

/// Lucene's special characters, escaped inside a quoted phrase.
fn phrase(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn text(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn opt_text(v: &Value, k: &str) -> Option<String> {
    v.get(k)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn credits(v: &Value) -> Vec<ArtistCredit> {
    v.get("artist-credit")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|c| ArtistCredit {
                    name: text(c, "name"),
                    id: c.get("artist").map(|a| text(a, "id")).unwrap_or_default(),
                    join: text(c, "joinphrase"),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn credit_line(cs: &[ArtistCredit]) -> String {
    cs.iter().map(|c| format!("{}{}", c.name, c.join)).collect()
}

/// A release lookup's JSON (`inc=artist-credits+recordings+release-groups`).
pub fn parse_release(v: &Value) -> Release {
    let group = v.get("release-group");
    let secondary: Vec<String> = group
        .and_then(|g| g.get("secondary-types"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_lowercase)
                .collect()
        })
        .unwrap_or_default();
    Release {
        id: text(v, "id"),
        title: text(v, "title"),
        date: opt_text(v, "date"),
        artists: credits(v),
        group_id: group.and_then(|g| opt_text(g, "id")),
        release_type: group
            .and_then(|g| opt_text(g, "primary-type"))
            .map(|t| t.to_lowercase()),
        compilation: secondary.iter().any(|s| s == "compilation"),
        media: v
            .get("media")
            .and_then(Value::as_array)
            .map(|ms| {
                ms.iter()
                    .map(|m| Medium {
                        position: m.get("position").and_then(Value::as_u64).unwrap_or(1) as u32,
                        tracks: m
                            .get("tracks")
                            .and_then(Value::as_array)
                            .map(|ts| {
                                ts.iter()
                                    .map(|t| Track {
                                        position: t
                                            .get("position")
                                            .and_then(Value::as_u64)
                                            .unwrap_or(0)
                                            as u32,
                                        title: text(t, "title"),
                                        recording_id: t
                                            .get("recording")
                                            .map(|r| text(r, "id"))
                                            .unwrap_or_default(),
                                        length_ms: t.get("length").and_then(Value::as_u64),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// A release search's JSON.
pub fn parse_hits(v: &Value) -> Vec<ReleaseHit> {
    v.get("releases")
        .and_then(Value::as_array)
        .map(|rs| {
            rs.iter()
                .map(|r| ReleaseHit {
                    id: text(r, "id"),
                    title: text(r, "title"),
                    artist: credit_line(&credits(r)),
                    date: opt_text(r, "date"),
                    track_count: r
                        .get("track-count")
                        .and_then(Value::as_u64)
                        .map(|n| n as u32),
                    group_id: r.get("release-group").and_then(|g| opt_text(g, "id")),
                    score: score_or(r, 0),
                    country: opt_text(r, "country"),
                    label: r.get("label-info").and_then(Value::as_array).and_then(|l| {
                        l.iter()
                            .find_map(|i| i.get("label").and_then(|l| opt_text(l, "name")))
                    }),
                    formats: formats(r),
                    release_type: r
                        .get("release-group")
                        .and_then(|g| opt_text(g, "primary-type"))
                        .map(|t| t.to_lowercase()),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A search result's score; a lookup has none and counts as `default`.
fn score_or(v: &Value, default: u64) -> u32 {
    v.get("score")
        .and_then(|s| s.as_u64().or_else(|| s.as_str()?.parse().ok()))
        .unwrap_or(default) as u32
}

fn score(v: &Value) -> u32 {
    score_or(v, 100)
}

/// "2×CD", "Digital Media": each medium's format, counted.
fn formats(r: &Value) -> Vec<String> {
    let mut out: Vec<(String, u32)> = Vec::new();
    for m in r
        .get("media")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let f = opt_text(m, "format").unwrap_or_else(|| "Other".into());
        match out.iter_mut().find(|(k, _)| *k == f) {
            Some((_, n)) => *n += 1,
            None => out.push((f, 1)),
        }
    }
    out.into_iter()
        .map(|(f, n)| if n > 1 { format!("{n}×{f}") } else { f })
        .collect()
}

/// A recording from a search or a lookup (`inc=artist-credits+isrcs+releases`).
pub fn parse_recording(r: &Value) -> Recording {
    Recording {
        id: text(r, "id"),
        title: text(r, "title"),
        artists: credits(r),
        length_ms: r.get("length").and_then(Value::as_u64),
        isrcs: r
            .get("isrcs")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        releases: r
            .get("releases")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|x| (text(x, "id"), text(x, "title"), opt_text(x, "date")))
                    .collect()
            })
            .unwrap_or_default(),
        score: score(r),
    }
}

/// An artist from a search or a lookup.
pub fn parse_artist(a: &Value) -> Artist {
    let life = a.get("life-span");
    Artist {
        id: text(a, "id"),
        name: text(a, "name"),
        disambiguation: text(a, "disambiguation"),
        kind: text(a, "type"),
        country: opt_text(a, "country"),
        begin: life
            .and_then(|l| opt_text(l, "begin"))
            .map(|d| d.chars().take(4).collect()),
        end: life
            .and_then(|l| opt_text(l, "end"))
            .map(|d| d.chars().take(4).collect()),
        score: score(a),
    }
}

#[async_trait]
impl MusicProvider for MusicBrainz {
    async fn release(&self, id: &str) -> Result<Release> {
        let v = self
            .get(
                &format!("/release/{id}"),
                &[("inc", "artist-credits recordings release-groups")],
            )
            .await?;
        Ok(parse_release(&v))
    }

    async fn search_releases(
        &self,
        artist: &str,
        title: &str,
        tracks: Option<u32>,
    ) -> Result<Vec<ReleaseHit>> {
        let mut q = format!("release:{} AND artist:{}", phrase(title), phrase(artist));
        if let Some(n) = tracks {
            q.push_str(&format!(" AND tracks:{n}"));
        }
        let v = self
            .get("/release/", &[("query", q.as_str()), ("limit", "10")])
            .await?;
        Ok(parse_hits(&v))
    }

    async fn group_releases(&self, group: &str) -> Result<Vec<ReleaseHit>> {
        let q = format!("rgid:{group}");
        let v = self
            .get("/release/", &[("query", q.as_str()), ("limit", "25")])
            .await?;
        Ok(parse_hits(&v))
    }

    async fn search_recordings(&self, title: &str, artist: &str) -> Result<Vec<Recording>> {
        let mut q = format!("recording:{}", phrase(title));
        if !artist.trim().is_empty() {
            q.push_str(&format!(" AND artist:{}", phrase(artist)));
        }
        let v = self
            .get("/recording/", &[("query", q.as_str()), ("limit", "10")])
            .await?;
        Ok(v.get("recordings")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(parse_recording).collect())
            .unwrap_or_default())
    }

    async fn recording(&self, id: &str) -> Result<Recording> {
        let v = self
            .get(
                &format!("/recording/{id}"),
                &[("inc", "artist-credits isrcs releases")],
            )
            .await?;
        Ok(parse_recording(&v))
    }

    async fn search_artists(&self, name: &str) -> Result<Vec<Artist>> {
        let q = format!("artist:{}", phrase(name));
        let v = self
            .get("/artist/", &[("query", q.as_str()), ("limit", "8")])
            .await?;
        Ok(v.get("artists")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(parse_artist).collect())
            .unwrap_or_default())
    }

    async fn artist(&self, id: &str) -> Result<Artist> {
        let v = self.get(&format!("/artist/{id}"), &[]).await?;
        Ok(parse_artist(&v))
    }

    async fn artist_groups(&self, id: &str) -> Result<Vec<String>> {
        let v = self
            .get("/release-group", &[("artist", id), ("limit", "100")])
            .await?;
        Ok(v.get("release-groups")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|g| text(g, "title")).collect())
            .unwrap_or_default())
    }

    async fn front_cover(&self, release: &str, group: Option<&str>) -> Result<Option<Bytes>> {
        let mut urls = vec![format!("{}/release/{release}/front-1200", self.art_base)];
        if let Some(g) = group {
            urls.push(format!("{}/release-group/{g}/front-1200", self.art_base));
        }
        for url in urls {
            let res = self
                .http
                .get(&url)
                .send()
                .await
                .map_err(|e| MetaError::Network(e.without_url().to_string()))?;
            match res.status().as_u16() {
                200 => {
                    return res
                        .bytes()
                        .await
                        .map(Some)
                        .map_err(|e| MetaError::Network(e.without_url().to_string()));
                }
                404 => continue,
                s => {
                    return Err(MetaError::Network(format!(
                        "Cover Art Archive answered HTTP {s}"
                    )));
                }
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Shaped like MusicBrainz's JSON web service answers (https://musicbrainz.org/doc/MusicBrainz_API).

    #[test]
    fn release_hits_tell_editions_apart() {
        let v = json!({"releases": [{
            "id": "r1", "score": 100, "title": "Winter Rooms", "date": "2019-02-01",
            "country": "DE", "track-count": 9,
            "artist-credit": [{"name": "Halden Quartet", "artist": {"id": "a1"}}],
            "release-group": {"id": "g1", "primary-type": "Album"},
            "label-info": [{"label": {"name": "Nordlicht"}}],
            "media": [{"format": "CD"}, {"format": "CD"}, {"format": "Digital Media"}]
        }]});
        let h = &parse_hits(&v)[0];
        assert_eq!(h.country.as_deref(), Some("DE"));
        assert_eq!(h.label.as_deref(), Some("Nordlicht"));
        assert_eq!(h.formats, ["2×CD", "Digital Media"]);
        assert_eq!(h.release_type.as_deref(), Some("album"));
        // A hit without a score is not a sure one.
        assert_eq!(parse_hits(&json!({"releases": [{"id": "x"}]}))[0].score, 0);
    }

    #[test]
    fn recordings_and_artists() {
        let r = parse_recording(&json!({
            "id": "rec1", "score": 97, "title": "Long Night", "length": 401000,
            "isrcs": ["NOX001900003"],
            "artist-credit": [{"name": "Halden Quartet", "joinphrase": " feat. ", "artist": {"id": "a1"}},
                              {"name": "Mara Ocampo", "artist": {"id": "a2"}}],
            "releases": [{"id": "r1", "title": "Winter Rooms", "date": "2019"}]
        }));
        assert_eq!((r.score, r.length_ms), (97, Some(401_000)));
        assert_eq!(r.artists.len(), 2);
        assert_eq!(r.artists[0].join, " feat. ");
        assert_eq!(r.releases[0].1, "Winter Rooms");
        assert_eq!(r.isrcs, ["NOX001900003"]);
        // A lookup has no score: it counts as sure.
        assert_eq!(parse_recording(&json!({"id": "rec1"})).score, 100);
        let a = parse_artist(&json!({
            "id": "a1", "score": 100, "name": "Halden Quartet", "type": "Group",
            "disambiguation": "Norwegian string quartet", "country": "NO",
            "life-span": {"begin": "2009-03", "ended": null}
        }));
        assert_eq!(a.disambiguation, "Norwegian string quartet");
        assert_eq!((a.begin.as_deref(), a.end), (Some("2009"), None));
        assert_eq!(a.kind, "Group");
    }
}
