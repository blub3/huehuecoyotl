//! Open Library (issue #67, SPEC 7 "Book providers"). No key. Without a contact address in the
//! User-Agent, Open Library allows one request per second; the app sends its name, version and
//! project URL on every request and keeps one limiter for all of them, backing off on 403 and
//! 429. Books are looked up one at a time on demand, never in bulk; covers only by cover id.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use serde_json::Value;

use crate::books::{Author, BookHit, BookProvider, BookQuery, Work};
use crate::musicbrainz::{Limiter, user_agent};
use crate::{MetaError, Result};

pub const API_BASE: &str = "https://openlibrary.org";
pub const COVERS_BASE: &str = "https://covers.openlibrary.org";
/// A little over one second: one request per second without a contact address (SPEC v1.12).
pub const INTERVAL: Duration = Duration::from_millis(1100);
const RETRIES: u32 = 3;
const FIELDS: &str = "key,title,author_name,author_key,first_publish_year,cover_i,language";

pub struct OpenLibrary {
    http: reqwest::Client,
    api_base: String,
    covers_base: String,
    limiter: Arc<Limiter>,
}

impl OpenLibrary {
    /// `version` is the app's version, sent in the User-Agent.
    pub fn new(version: &str) -> Result<Self> {
        Self::with_bases(version, API_BASE, COVERS_BASE, INTERVAL)
    }

    /// For tests: other servers and pace.
    pub fn with_bases(version: &str, api: &str, covers: &str, interval: Duration) -> Result<Self> {
        crate::tmdb::install_crypto_provider();
        let http = reqwest::Client::builder()
            .user_agent(user_agent(version))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| MetaError::Other(e.to_string()))?;
        Ok(Self {
            http,
            api_base: api.trim_end_matches('/').to_string(),
            covers_base: covers.trim_end_matches('/').to_string(),
            limiter: Arc::new(Limiter::new(interval)),
        })
    }

    /// GET at the limiter's pace; 403 and 429 back off and retry, then fail as rate limited
    /// (the job is retried later). `None` for 404.
    async fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<Option<reqwest::Response>> {
        let mut backoff = Duration::from_secs(5);
        for attempt in 0..=RETRIES {
            self.limiter.wait().await;
            let res = self
                .http
                .get(url)
                .query(query)
                .send()
                .await
                .map_err(|e| MetaError::Network(e.without_url().to_string()))?;
            match res.status().as_u16() {
                200 => return Ok(Some(res)),
                404 => return Ok(None),
                403 | 429 if attempt < RETRIES => {
                    let wait = res
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse().ok())
                        .map(Duration::from_secs)
                        .unwrap_or(backoff);
                    tracing::info!(?wait, "Open Library asks to slow down; waiting");
                    tokio::time::sleep(wait).await;
                    backoff *= 2;
                }
                403 | 429 => return Err(MetaError::RateLimited(300)),
                s => {
                    return Err(MetaError::Network(format!(
                        "Open Library answered HTTP {s}"
                    )));
                }
            }
        }
        Err(MetaError::RateLimited(300))
    }

    async fn json(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let url = format!("{}{path}", self.api_base);
        match self.get(&url, query).await? {
            Some(res) => res
                .json()
                .await
                .map_err(|e| MetaError::Other(e.to_string())),
            None => Err(MetaError::NotFound),
        }
    }
}

fn text(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn strings(v: &Value, k: &str) -> Vec<String> {
    v.get(k)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// "/works/OL66554W" → "OL66554W".
fn work_id(key: &str) -> String {
    key.rsplit('/').next().unwrap_or(key).to_string()
}

/// A search's JSON (`search.json`).
pub fn parse_search(v: &Value) -> Vec<BookHit> {
    v.get("docs")
        .and_then(Value::as_array)
        .map(|docs| {
            docs.iter()
                .map(|d| BookHit {
                    id: work_id(&text(d, "key")),
                    title: text(d, "title"),
                    authors: strings(d, "author_name"),
                    year: d
                        .get("first_publish_year")
                        .and_then(Value::as_u64)
                        .map(|y| y as u16),
                    cover_id: d.get("cover_i").and_then(Value::as_i64).filter(|c| *c > 0),
                    languages: strings(d, "language"),
                    author_ids: strings(d, "author_key"),
                })
                .filter(|h| !h.id.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// A work's JSON (`/works/<id>.json`). The description is text or `{ "value": text }`.
pub fn parse_work(v: &Value) -> Work {
    let description = match v.get("description") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(o) => o.get("value").and_then(Value::as_str).map(str::to_string),
        None => None,
    }
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty());
    let year = v
        .get("first_publish_date")
        .and_then(Value::as_str)
        .and_then(|d| {
            d.split(|c: char| !c.is_ascii_digit())
                .find(|p| p.len() == 4)
                .and_then(|y| y.parse().ok())
        });
    Work {
        id: work_id(&text(v, "key")),
        title: text(v, "title"),
        description,
        subjects: strings(v, "subjects").into_iter().take(12).collect(),
        year,
        cover_ids: v
            .get("covers")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_i64)
                    .filter(|c| *c > 0)
                    .collect()
            })
            .unwrap_or_default(),
        author_ids: v
            .get("authors")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        x.get("author")
                            .and_then(|y| y.get("key"))
                            .and_then(Value::as_str)
                    })
                    .map(work_id)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Text or `{ "value": text }`, trimmed; `None` when empty.
fn text_value(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) => Some(s.clone()),
        Some(o) => o.get("value").and_then(Value::as_str).map(str::to_string),
        None => None,
    }
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
}

fn opt(v: &Value, k: &str) -> Option<String> {
    v.get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// An author search's JSON (`search/authors.json`).
pub fn parse_author_search(v: &Value) -> Vec<Author> {
    v.get("docs")
        .and_then(Value::as_array)
        .map(|docs| {
            docs.iter()
                .map(|d| Author {
                    id: work_id(&text(d, "key")),
                    name: text(d, "name"),
                    birth: opt(d, "birth_date"),
                    death: opt(d, "death_date"),
                    work_count: d
                        .get("work_count")
                        .and_then(Value::as_u64)
                        .map(|n| n as u32),
                    top_work: opt(d, "top_work"),
                    ..Default::default()
                })
                .filter(|a| !a.id.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// An author's JSON (`/authors/<id>.json`).
pub fn parse_author(v: &Value) -> Author {
    Author {
        id: work_id(&text(v, "key")),
        name: opt(v, "name")
            .or_else(|| opt(v, "personal_name"))
            .unwrap_or_default(),
        birth: opt(v, "birth_date"),
        death: opt(v, "death_date"),
        bio: text_value(v.get("bio")),
        photo_ids: v
            .get("photos")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_i64)
                    .filter(|c| *c > 0)
                    .collect()
            })
            .unwrap_or_default(),
        ..Default::default()
    }
}

/// Only letters, digits and X: what an ISBN is made of.
fn clean_isbn(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
        .collect()
}

#[async_trait]
impl BookProvider for OpenLibrary {
    async fn search(&self, q: BookQuery<'_>) -> Result<Vec<BookHit>> {
        let v = match q {
            BookQuery::Isbn(isbn) => {
                let q = format!("isbn:{}", clean_isbn(isbn));
                self.json(
                    "/search.json",
                    &[("q", q.as_str()), ("fields", FIELDS), ("limit", "5")],
                )
                .await?
            }
            BookQuery::TitleAuthor { title, author } => {
                let mut query = vec![("title", title), ("fields", FIELDS), ("limit", "10")];
                if !author.is_empty() {
                    query.push(("author", author));
                }
                self.json("/search.json", &query).await?
            }
        };
        Ok(parse_search(&v))
    }

    async fn work(&self, id: &str) -> Result<Work> {
        let id = work_id(id);
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(MetaError::NotFound);
        }
        let v = self.json(&format!("/works/{id}.json"), &[]).await?;
        Ok(parse_work(&v))
    }

    async fn search_authors(&self, name: &str) -> Result<Vec<Author>> {
        let v = self
            .json("/search/authors.json", &[("q", name), ("limit", "8")])
            .await?;
        Ok(parse_author_search(&v))
    }

    async fn author(&self, id: &str) -> Result<Author> {
        let id = work_id(id);
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(MetaError::NotFound);
        }
        let v = self.json(&format!("/authors/{id}.json"), &[]).await?;
        Ok(parse_author(&v))
    }

    async fn author_works(&self, id: &str) -> Result<Vec<String>> {
        let id = work_id(id);
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(MetaError::NotFound);
        }
        let v = self
            .json(&format!("/authors/{id}/works.json"), &[("limit", "100")])
            .await?;
        Ok(v.get("entries")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|e| text(e, "title"))
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn author_photo(&self, photo_id: i64) -> Result<Option<Bytes>> {
        let url = format!("{}/a/id/{photo_id}-L.jpg", self.covers_base);
        match self.get(&url, &[("default", "false")]).await? {
            Some(res) => res
                .bytes()
                .await
                .map(Some)
                .map_err(|e| MetaError::Network(e.without_url().to_string())),
            None => Ok(None),
        }
    }

    async fn cover(&self, cover_id: i64) -> Result<Option<Bytes>> {
        let url = format!("{}/b/id/{cover_id}-L.jpg", self.covers_base);
        // Without `default=false` a missing cover answers with a blank image.
        match self.get(&url, &[("default", "false")]).await? {
            Some(res) => res
                .bytes()
                .await
                .map(Some)
                .map_err(|e| MetaError::Network(e.without_url().to_string())),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Shaped like Open Library's answers (https://openlibrary.org/developers/api).

    #[test]
    fn authors_and_work_authors() {
        let hits = parse_author_search(&json!({"docs": [
            {"key": "OL1A", "name": "Liv Ostrander", "birth_date": "1974", "work_count": 7,
             "top_work": "The Salt Archive"},
            {"key": "/authors/OL2A", "name": "Liv Ostrander", "birth_date": "1902", "death_date": "1971"}
        ]}));
        assert_eq!(hits.len(), 2);
        assert_eq!((hits[0].id.as_str(), hits[0].work_count), ("OL1A", Some(7)));
        assert_eq!(hits[1].id, "OL2A");
        assert_eq!(hits[1].death.as_deref(), Some("1971"));
        let a = parse_author(&json!({
            "key": "/authors/OL1A", "name": "Liv Ostrander",
            "bio": {"type": "/type/text", "value": " Norwegian novelist. "},
            "photos": [-1, 1234]
        }));
        assert_eq!(a.bio.as_deref(), Some("Norwegian novelist."));
        assert_eq!(a.photo_ids, [1234]);
        assert_eq!(
            parse_author(&json!({"key": "OL3A", "bio": "Plain"}))
                .bio
                .as_deref(),
            Some("Plain")
        );
        let w = parse_work(&json!({
            "key": "/works/OL9W", "title": "The Salt Archive",
            "authors": [{"author": {"key": "/authors/OL1A"}, "type": {"key": "/type/author_role"}}]
        }));
        assert_eq!(
            (w.title.as_str(), w.author_ids.as_slice()),
            ("The Salt Archive", &["OL1A".to_string()][..])
        );
    }
}
