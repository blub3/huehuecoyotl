//! Metadata providers (SPEC 7). The scanner only sees [`MetadataProvider`]; TMDB is the first
//! implementation, local `.nfo` files or other databases can follow without touching it.

pub mod artwork;
pub mod matching;
pub mod tmdb;

use async_trait::async_trait;
use bytes::Bytes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaType {
    Movie,
    Show,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Candidate {
    pub id: i64,
    pub title: String,
    pub original_title: String,
    pub year: Option<u16>,
    pub overview: String,
    pub poster_path: Option<String>,
    pub popularity: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeasonRef {
    pub season_no: u16,
    pub name: String,
    pub poster_path: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Details {
    pub id: i64,
    pub title: String,
    pub original_title: String,
    pub year: Option<u16>,
    pub overview: String,
    pub rating: Option<f64>,
    pub runtime_min: Option<u32>,
    pub genres: Vec<String>,
    pub imdb_id: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub cast: Vec<String>,
    pub seasons: Vec<SeasonRef>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EpisodeDetails {
    pub episode_no: u16,
    pub title: String,
    pub overview: String,
    pub still_path: Option<String>,
    pub year: Option<u16>,
    pub runtime_min: Option<u32>,
    pub rating: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeasonDetails {
    pub season_no: u16,
    pub name: String,
    pub overview: String,
    pub poster_path: Option<String>,
    pub episodes: Vec<EpisodeDetails>,
}

#[derive(Debug, thiserror::Error)]
pub enum MetaError {
    #[error("the metadata service rejected the API key")]
    Auth,
    #[error("rate limited; retry in {0} s")]
    RateLimited(u64),
    #[error("not found")]
    NotFound,
    #[error("network: {0}")]
    Network(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, MetaError>;

#[async_trait]
pub trait MetadataProvider: Send + Sync {
    /// Check the credentials with a cheap request.
    async fn validate(&self) -> Result<()>;
    async fn search(&self, ty: MediaType, title: &str, year: Option<u16>)
    -> Result<Vec<Candidate>>;
    async fn details(&self, ty: MediaType, id: i64) -> Result<Details>;
    async fn season(&self, show_id: i64, season_no: u16) -> Result<SeasonDetails>;
    /// Download an image by its provider path at a provider-specific size ("w342", "original").
    async fn image(&self, path: &str, size: &str) -> Result<Bytes>;
}
