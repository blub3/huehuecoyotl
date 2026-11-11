//! WebDAV over HTTP(S): PROPFIND for listings, ranged GET for reads (SPEC 6).
//! Built on reqwest with rustls on the ring provider and the OS certificate store.

use std::sync::Once;
use std::time::{Duration, UNIX_EPOCH};

use async_trait::async_trait;
use bytes::Bytes;
use percent_encoding::percent_decode_str;
use quick_xml::Reader;
use quick_xml::events::Event;
use reqwest::{Method, StatusCode, Url};

use crate::remote::{RemoteConfig, RemoteSource, rel};
use crate::{Caps, Entry, Result, Source, VPath, VfsError};

const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:"><D:prop><D:resourcetype/><D:getcontentlength/><D:getlastmodified/></D:prop></D:propfind>"#;

pub struct WebdavSource {
    cfg: RemoteConfig,
    http: reqwest::Client,
    base: Url,
}

fn install_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

impl WebdavSource {
    pub fn new(cfg: RemoteConfig) -> Result<Self> {
        install_crypto_provider();
        let scheme = if cfg.tls { "https" } else { "http" };
        let base = Url::parse(&format!("{scheme}://{}:{}/", cfg.host, cfg.port()))
            .map_err(|e| VfsError::Unsupported(format!("invalid address: {e}")))?;
        let http = reqwest::Client::builder()
            .user_agent(concat!("Huehuecoyotl/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(60))
            .pool_idle_timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| VfsError::Internal(e.to_string()))?;
        Ok(Self { cfg, http, base })
    }

    fn url(&self, path: &VPath, dir: bool) -> Url {
        let mut u = self.base.clone();
        {
            let mut segs = u.path_segments_mut().expect("http URL");
            segs.clear();
            for s in self
                .cfg
                .share
                .split('/')
                .chain(rel(path).split('/'))
                .filter(|s| !s.is_empty())
            {
                segs.push(s);
            }
            if dir {
                segs.push(""); // trailing slash for collections
            }
        }
        u
    }

    fn request(&self, method: Method, url: Url) -> reqwest::RequestBuilder {
        let rb = self.http.request(method, url);
        if self.cfg.user.is_empty() {
            rb
        } else {
            rb.basic_auth(&self.cfg.user, Some(&self.cfg.password))
        }
    }

    async fn propfind(&self, path: &VPath, depth: &str) -> Result<Vec<(String, bool, u64, i64)>> {
        let url = self.url(path, depth == "1");
        let resp = self
            .request(Method::from_bytes(b"PROPFIND").expect("method"), url)
            .header("Depth", depth)
            .header("Content-Type", "application/xml")
            .body(PROPFIND_BODY)
            .send()
            .await
            .map_err(err)?;
        match resp.status() {
            StatusCode::MULTI_STATUS | StatusCode::OK => {}
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(VfsError::Auth(format!("HTTP {}", resp.status())));
            }
            StatusCode::NOT_FOUND => return Err(VfsError::NotFound(path.to_string())),
            s => return Err(VfsError::Internal(format!("PROPFIND answered HTTP {s}"))),
        }
        let xml = resp.text().await.map_err(err)?;
        parse_multistatus(&xml)
    }
}

/// (decoded href, is collection, size, mtime) per `<response>`.
fn parse_multistatus(xml: &str) -> Result<Vec<(String, bool, u64, i64)>> {
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(true);
    let mut out = Vec::new();
    let (mut href, mut is_dir, mut size, mut mtime) = (String::new(), false, 0u64, 0i64);
    let mut current = String::new();
    loop {
        match r
            .read_event()
            .map_err(|e| VfsError::Internal(format!("WebDAV XML: {e}")))?
        {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.local_name().as_ref().to_ascii_lowercase();
                match name.as_str() {
                    "response" => {
                        (href, is_dir, size, mtime) = (String::new(), false, 0, 0);
                    }
                    "collection" => is_dir = true,
                    _ => {}
                }
                current = name;
            }
            Event::Text(t) => {
                let v = quick_xml::escape::unescape(t.as_ref())
                    .map_err(|e| VfsError::Internal(format!("WebDAV XML: {e}")))?
                    .into_owned();
                match current.as_str() {
                    "href" => href = percent_decode_str(&v).decode_utf8_lossy().into_owned(),
                    "getcontentlength" => size = v.trim().parse().unwrap_or(0),
                    "getlastmodified" => {
                        mtime = httpdate::parse_http_date(v.trim())
                            .ok()
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map_or(0, |d| d.as_secs() as i64)
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                if e.local_name().as_ref().eq_ignore_ascii_case("response") && !href.is_empty() {
                    out.push((href.clone(), is_dir, size, mtime));
                }
                current.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// Map a reqwest error by its kind, not its text (which never says "connect").
fn err(e: reqwest::Error) -> VfsError {
    let unreachable = e.is_connect() || e.is_timeout() || e.is_request();
    let msg = e.without_url().to_string();
    if unreachable {
        VfsError::Unreachable(msg)
    } else {
        crate::remote::err(msg)
    }
}

fn last_segment(href: &str) -> &str {
    let path = href
        .split_once("://")
        .map_or(href, |(_, rest)| rest.find('/').map_or("", |i| &rest[i..]));
    path.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

#[async_trait]
impl Source for WebdavSource {
    async fn list(&self, dir: &VPath) -> Result<Vec<Entry>> {
        let self_path = self.url(dir, true).path().trim_end_matches('/').to_string();
        let self_path = percent_decode_str(&self_path)
            .decode_utf8_lossy()
            .into_owned();
        let mut out = Vec::new();
        for (href, is_dir, size, mtime) in self.propfind(dir, "1").await? {
            let path_part = href
                .split_once("://")
                .map_or(href.as_str(), |(_, r)| r.find('/').map_or("", |i| &r[i..]));
            if path_part.trim_end_matches('/') == self_path {
                continue; // the folder itself
            }
            let name = last_segment(&href).to_string();
            if name.is_empty() || name.starts_with('.') {
                continue;
            }
            out.push(Entry {
                path: dir.join(&name),
                name,
                is_dir,
                size: if is_dir { 0 } else { size },
                mtime,
            });
        }
        Ok(out)
    }

    async fn stat(&self, path: &VPath) -> Result<Entry> {
        let r = self.propfind(path, "0").await?;
        let (_, is_dir, size, mtime) = r
            .into_iter()
            .next()
            .ok_or_else(|| VfsError::NotFound(path.to_string()))?;
        Ok(Entry {
            path: path.clone(),
            name: path.file_name().to_string(),
            is_dir,
            size,
            mtime,
        })
    }

    async fn read_at(&self, path: &VPath, offset: u64, len: u32) -> Result<Bytes> {
        if len == 0 {
            return Ok(Bytes::new());
        }
        let end = offset + u64::from(len) - 1;
        let resp = self
            .request(Method::GET, self.url(path, false))
            .header("Range", format!("bytes={offset}-{end}"))
            .send()
            .await
            .map_err(err)?;
        match resp.status() {
            StatusCode::PARTIAL_CONTENT => resp.bytes().await.map_err(err),
            StatusCode::RANGE_NOT_SATISFIABLE => Ok(Bytes::new()),
            StatusCode::OK if offset == 0 => {
                // Server ignored the range: take what we asked for from the start.
                let b = resp.bytes().await.map_err(err)?;
                Ok(b.slice(..b.len().min(len as usize)))
            }
            StatusCode::OK => Err(VfsError::Unsupported(
                "the WebDAV server does not support range requests".into(),
            )),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                Err(VfsError::Auth(format!("HTTP {}", resp.status())))
            }
            StatusCode::NOT_FOUND => Err(VfsError::NotFound(path.to_string())),
            s => Err(VfsError::Internal(format!("GET answered HTTP {s}"))),
        }
    }

    fn capabilities(&self) -> Caps {
        Caps {
            range_reads: true,
            ..Default::default()
        }
    }
}

impl RemoteSource for WebdavSource {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multistatus_parsing() {
        let xml = r#"<?xml version="1.0"?><D:multistatus xmlns:D="DAV:">
          <D:response><D:href>/media/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop></D:propstat></D:response>
          <D:response><D:href>/media/Alien%20(1979)/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype>
            <D:getlastmodified>Wed, 11 Nov 2026 04:42:31 GMT</D:getlastmodified></D:prop></D:propstat></D:response>
          <D:response><D:href>http://nas/media/a%C3%A9.mkv</D:href><D:propstat><D:prop><D:resourcetype/>
            <D:getcontentlength>1234</D:getcontentlength></D:prop></D:propstat></D:response>
        </D:multistatus>"#;
        let r = parse_multistatus(xml).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!((last_segment(&r[1].0), r[1].1), ("Alien (1979)", true));
        assert!(r[1].3 > 1_700_000_000);
        assert_eq!(
            (last_segment(&r[2].0), r[2].1, r[2].2),
            ("aé.mkv", false, 1234)
        );
    }
}
