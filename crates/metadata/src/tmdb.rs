//! TMDB client (SPEC 7). Requests the user's language with an English fallback for empty
//! overviews, combines detail calls with `append_to_response`, rate-limits on the client and
//! backs off on HTTP 429. The API key never appears in logs or errors.

use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use serde_json::Value;

use crate::{
    Candidate, Details, EpisodeDetails, MediaType, MetaError, MetadataProvider, Result,
    SeasonDetails, SeasonRef,
};

pub const API_BASE: &str = "https://api.themoviedb.org/3";
pub const IMAGE_BASE: &str = "https://image.tmdb.org/t/p";
/// TMDB allows roughly 50 requests per second; stay well below.
const MIN_INTERVAL: Duration = Duration::from_millis(30);

pub struct Tmdb {
    http: reqwest::Client,
    key: String,
    language: String,
    api_base: String,
    image_base: String,
    next_slot: Mutex<Instant>,
}

fn install_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn year_of(date: Option<&str>) -> Option<u16> {
    date.and_then(|d| d.get(..4))
        .and_then(|y| y.parse().ok())
        .filter(|y| *y > 1800)
}

fn s(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn opt_s(v: &Value, k: &str) -> Option<String> {
    v.get(k)
        .and_then(Value::as_str)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
}

impl Tmdb {
    /// `language` is a BCP-47 tag like "de-DE"; English is used as the fallback.
    pub fn new(key: impl Into<String>, language: impl Into<String>) -> Result<Self> {
        Self::with_base(key, language, API_BASE, IMAGE_BASE)
    }

    pub fn with_base(
        key: impl Into<String>,
        language: impl Into<String>,
        api: &str,
        images: &str,
    ) -> Result<Self> {
        install_crypto_provider();
        let http = reqwest::Client::builder()
            .user_agent(concat!("Huehuecoyotl/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| MetaError::Other(e.to_string()))?;
        Ok(Self {
            http,
            key: key.into().trim().to_string(),
            language: language.into(),
            api_base: api.trim_end_matches('/').to_string(),
            image_base: images.trim_end_matches('/').to_string(),
            next_slot: Mutex::new(Instant::now()),
        })
    }

    async fn throttle(&self) {
        let wait = {
            let mut next = self.next_slot.lock().unwrap();
            let now = Instant::now();
            let slot = (*next).max(now);
            *next = slot + MIN_INTERVAL;
            slot - now
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }

    fn authed(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        // v4 "API Read Access Tokens" are JWTs; v3 keys are 32 hex characters.
        if self.key.starts_with("eyJ") {
            rb.bearer_auth(&self.key)
        } else {
            rb.query(&[("api_key", self.key.as_str())])
        }
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.throttle().await;
        let url = format!("{}{}", self.api_base, path);
        let resp = self
            .authed(self.http.get(&url))
            .query(query)
            .send()
            .await
            .map_err(|e| MetaError::Network(e.without_url().to_string()))?;
        match resp.status().as_u16() {
            200..=299 => resp
                .json()
                .await
                .map_err(|e| MetaError::Other(e.without_url().to_string())),
            401 => Err(MetaError::Auth),
            404 => Err(MetaError::NotFound),
            429 => {
                let secs = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(10);
                Err(MetaError::RateLimited(secs))
            }
            code => Err(MetaError::Network(format!("HTTP {code} for {path}"))),
        }
    }

    fn candidate(ty: MediaType, v: &Value) -> Candidate {
        let (t, o, d) = match ty {
            MediaType::Movie => ("title", "original_title", "release_date"),
            MediaType::Show => ("name", "original_name", "first_air_date"),
        };
        Candidate {
            id: v.get("id").and_then(Value::as_i64).unwrap_or(0),
            title: s(v, t),
            original_title: s(v, o),
            year: year_of(v.get(d).and_then(Value::as_str)),
            overview: s(v, "overview"),
            poster_path: opt_s(v, "poster_path"),
            popularity: v.get("popularity").and_then(Value::as_f64).unwrap_or(0.0),
            origin_country: v["origin_country"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    fn is_english(&self) -> bool {
        self.language.starts_with("en")
    }
}

#[async_trait]
impl MetadataProvider for Tmdb {
    async fn validate(&self) -> Result<()> {
        self.get("/authentication", &[]).await.map(|_| ())
    }

    async fn search(
        &self,
        ty: MediaType,
        title: &str,
        year: Option<u16>,
    ) -> Result<Vec<Candidate>> {
        let (path, year_key) = match ty {
            MediaType::Movie => ("/search/movie", "year"),
            MediaType::Show => ("/search/tv", "first_air_date_year"),
        };
        let mut q = vec![
            ("query", title.to_string()),
            ("language", self.language.clone()),
            ("include_adult", "false".into()),
        ];
        if let Some(y) = year {
            q.push((year_key, y.to_string()));
        }
        let mut v = self.get(path, &q).await?;
        // A year from a file name can be off by one (release vs. premiere); retry without it.
        if year.is_some() && v["results"].as_array().is_none_or(Vec::is_empty) {
            q.retain(|(k, _)| *k != year_key);
            v = self.get(path, &q).await?;
        }
        Ok(v["results"]
            .as_array()
            .map(|a| a.iter().take(10).map(|r| Self::candidate(ty, r)).collect())
            .unwrap_or_default())
    }

    async fn details(&self, ty: MediaType, id: i64) -> Result<Details> {
        let path = match ty {
            MediaType::Movie => format!("/movie/{id}"),
            MediaType::Show => format!("/tv/{id}"),
        };
        let q = [
            ("language", self.language.clone()),
            ("append_to_response", "credits,external_ids".to_string()),
        ];
        let v = self.get(&path, &q).await?;
        let c = Self::candidate(ty, &v);
        let mut overview = c.overview.clone();
        if overview.is_empty() && !self.is_english() {
            if let Ok(en) = self.get(&path, &[("language", "en-US".to_string())]).await {
                overview = s(&en, "overview");
            }
        }
        let runtime = match ty {
            MediaType::Movie => v.get("runtime").and_then(Value::as_u64),
            MediaType::Show => v["episode_run_time"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(Value::as_u64),
        };
        Ok(Details {
            id,
            title: c.title,
            original_title: c.original_title,
            year: c.year,
            overview,
            rating: v
                .get("vote_average")
                .and_then(Value::as_f64)
                .filter(|r| *r > 0.0),
            runtime_min: runtime.map(|r| r as u32).filter(|r| *r > 0),
            genres: v["genres"]
                .as_array()
                .map(|g| g.iter().map(|x| s(x, "name")).collect())
                .unwrap_or_default(),
            imdb_id: opt_s(&v, "imdb_id").or_else(|| opt_s(&v["external_ids"], "imdb_id")),
            poster_path: c.poster_path,
            backdrop_path: opt_s(&v, "backdrop_path"),
            cast: v["credits"]["cast"]
                .as_array()
                .map(|a| a.iter().take(12).map(|x| s(x, "name")).collect())
                .unwrap_or_default(),
            seasons: v["seasons"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|x| SeasonRef {
                            season_no: x.get("season_number").and_then(Value::as_u64).unwrap_or(0)
                                as u16,
                            name: s(x, "name"),
                            poster_path: opt_s(x, "poster_path"),
                            episode_count: x
                                .get("episode_count")
                                .and_then(Value::as_u64)
                                .unwrap_or(0) as u16,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    async fn season(&self, show_id: i64, season_no: u16) -> Result<SeasonDetails> {
        let path = format!("/tv/{show_id}/season/{season_no}");
        let v = self
            .get(&path, &[("language", self.language.clone())])
            .await?;
        let needs_en = !self.is_english()
            && v["episodes"]
                .as_array()
                .is_some_and(|e| e.iter().any(|x| s(x, "overview").is_empty()));
        let en = if needs_en {
            self.get(&path, &[("language", "en-US".to_string())])
                .await
                .ok()
        } else {
            None
        };
        let episodes = v["episodes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .enumerate()
                    .map(|(i, x)| {
                        let mut overview = s(x, "overview");
                        if overview.is_empty() {
                            if let Some(e) = en.as_ref().and_then(|e| e["episodes"].get(i)) {
                                overview = s(e, "overview");
                            }
                        }
                        EpisodeDetails {
                            episode_no: x.get("episode_number").and_then(Value::as_u64).unwrap_or(0)
                                as u16,
                            title: s(x, "name"),
                            overview,
                            still_path: opt_s(x, "still_path"),
                            year: year_of(x.get("air_date").and_then(Value::as_str)),
                            runtime_min: x.get("runtime").and_then(Value::as_u64).map(|r| r as u32),
                            rating: x
                                .get("vote_average")
                                .and_then(Value::as_f64)
                                .filter(|r| *r > 0.0),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(SeasonDetails {
            season_no,
            name: s(&v, "name"),
            overview: s(&v, "overview"),
            poster_path: opt_s(&v, "poster_path"),
            episodes,
        })
    }

    async fn image(&self, path: &str, size: &str) -> Result<Bytes> {
        self.throttle().await;
        let url = format!("{}/{}{}", self.image_base, size, path);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| MetaError::Network(e.without_url().to_string()))?;
        if !resp.status().is_success() {
            return Err(MetaError::Network(format!(
                "HTTP {} for image",
                resp.status().as_u16()
            )));
        }
        resp.bytes()
            .await
            .map_err(|e| MetaError::Network(e.without_url().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn server() -> (MockServer, Tmdb) {
        let srv = MockServer::start().await;
        let t = Tmdb::with_base(
            "0123456789abcdef0123456789abcdef",
            "de-DE",
            &srv.uri(),
            &format!("{}/img", srv.uri()),
        )
        .unwrap();
        (srv, t)
    }

    #[tokio::test]
    async fn search_with_year_fallback() {
        let (srv, t) = server().await;
        Mock::given(method("GET"))
            .and(path("/search/movie"))
            .and(query_param("year", "2017"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": []})))
            .mount(&srv)
            .await;
        Mock::given(method("GET")).and(path("/search/movie")).and(query_param_is_missing("year"))
            .and(query_param("api_key", "0123456789abcdef0123456789abcdef")).and(query_param("language", "de-DE"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": [
                {"id": 335984, "title": "Blade Runner 2049", "original_title": "Blade Runner 2049",
                 "release_date": "2017-10-04", "overview": "", "poster_path": "/p.jpg", "popularity": 80.5}
            ]})))
            .mount(&srv).await;
        let r = t
            .search(MediaType::Movie, "Blade Runner 2049", Some(2017))
            .await
            .unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(
            (r[0].id, r[0].year, r[0].poster_path.as_deref()),
            (335984, Some(2017), Some("/p.jpg"))
        );
    }

    #[tokio::test]
    async fn details_with_english_overview_fallback_and_bearer_token() {
        let srv = MockServer::start().await;
        let t = Tmdb::with_base(
            "eyJhbGciOiJIUzI1NiJ9.token",
            "de-DE",
            &srv.uri(),
            &srv.uri(),
        )
        .unwrap();
        Mock::given(path("/tv/1396")).and(query_param("language", "de-DE")).and(header("authorization", "Bearer eyJhbGciOiJIUzI1NiJ9.token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "Breaking Bad", "original_name": "Breaking Bad", "first_air_date": "2008-01-20",
                "overview": "", "vote_average": 8.9, "episode_run_time": [47], "genres": [{"name": "Drama"}],
                "external_ids": {"imdb_id": "tt0903747"}, "credits": {"cast": [{"name": "Bryan Cranston"}]},
                "seasons": [{"season_number": 1, "name": "Staffel 1", "poster_path": "/s1.jpg", "episode_count": 7}]
            })))
            .mount(&srv).await;
        Mock::given(path("/tv/1396"))
            .and(query_param("language", "en-US"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"overview": "A chemistry teacher..."})),
            )
            .mount(&srv)
            .await;
        let d = t.details(MediaType::Show, 1396).await.unwrap();
        assert_eq!(d.overview, "A chemistry teacher...");
        assert_eq!(
            (d.year, d.runtime_min, d.imdb_id.as_deref()),
            (Some(2008), Some(47), Some("tt0903747"))
        );
        assert_eq!(d.cast, ["Bryan Cranston"]);
        assert_eq!(d.seasons[0].name, "Staffel 1");
        assert_eq!(d.seasons[0].episode_count, 7);
    }

    #[tokio::test]
    async fn errors_are_typed_and_keys_not_leaked() {
        let (srv, t) = server().await;
        Mock::given(path("/authentication"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&srv)
            .await;
        Mock::given(path("/movie/1"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "7"))
            .mount(&srv)
            .await;
        assert!(matches!(t.validate().await, Err(MetaError::Auth)));
        let e = t.details(MediaType::Movie, 1).await.unwrap_err();
        assert!(matches!(e, MetaError::RateLimited(7)));
        assert!(!format!("{e:?}").contains("0123456789abcdef"));
        // network error text must not contain the URL with the key either
        let dead = Tmdb::with_base(
            "0123456789abcdef0123456789abcdef",
            "en",
            "http://127.0.0.1:9",
            "http://127.0.0.1:9",
        )
        .unwrap();
        let e = dead.validate().await.unwrap_err();
        assert!(!e.to_string().contains("0123456789abcdef"), "{e}");
    }

    #[tokio::test]
    async fn season_episodes() {
        let (srv, t) = server().await;
        Mock::given(path("/tv/5/season/2")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "Staffel 2", "overview": "x", "poster_path": null,
            "episodes": [{"episode_number": 1, "name": "Eins", "overview": "o", "still_path": "/e1.jpg", "air_date": "2010-05-01", "runtime": 45}]
        }))).mount(&srv).await;
        let s = t.season(5, 2).await.unwrap();
        assert_eq!(s.episodes[0].title, "Eins");
        assert_eq!(s.episodes[0].year, Some(2010));
        assert_eq!(s.poster_path, None);
    }
}
