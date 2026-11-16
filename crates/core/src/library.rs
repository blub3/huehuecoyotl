//! M2 requests: library folders, paged browsing, detail pages, search, the review list,
//! manual matching and the TMDB key.

use std::sync::Arc;

use huehuecoyotl_library::catalog::{self, Query, Sort};
use huehuecoyotl_library::jobs;
use huehuecoyotl_metadata::tmdb::Tmdb;
use huehuecoyotl_metadata::{MediaType, MetaError, MetadataProvider};
use serde_json::json;

use crate::secrets::{self, KeySource};
use crate::{Core, CoreError, ErrorKind, Event, Inner, LibraryInfo, ReviewItem, worker};

fn internal(e: impl std::fmt::Display) -> CoreError {
    CoreError {
        kind: ErrorKind::Internal,
        message: e.to_string(),
    }
}

pub(crate) async fn library_list(inner: &Arc<Inner>) -> Result<Vec<LibraryInfo>, CoreError> {
    let rows = inner.lib.read(catalog::libraries).await?;
    let scanning = inner.scanning.lock().unwrap().clone();
    let offline = inner.offline.lock().unwrap().clone();
    Ok(rows
        .into_iter()
        .map(|l| LibraryInfo {
            scanning: scanning.contains(&l.id),
            online: !offline.contains(&l.id),
            id: l.id,
            root: crate::network::display_url(&l.kind, &l.config_json, &l.root),
            kind: l.kind,
            name: l.name,
            files: l.files,
            last_scan_ms: l.last_scan.unwrap_or(0),
            content: l.content,
        })
        .collect())
}

pub(crate) async fn enqueue_scan(inner: &Arc<Inner>, source_id: i64, prio: i64) {
    let payload = json!({ "source_id": source_id }).to_string();
    let _ = inner
        .lib
        .write(move |c| {
            jobs::enqueue(
                c,
                "scan",
                &payload,
                prio,
                Some(&format!("scan:{source_id}")),
            )
        })
        .await;
    inner.wake.notify_waiters();
}

impl Core {
    pub fn libraries(&self, request_id: u64) {
        self.spawn(request_id, move |inner| async move {
            Ok(Event::Libraries {
                request_id,
                libraries: library_list(&inner).await?,
            })
        });
    }

    /// Add a folder to the library and scan it. `content`: `video` or `music` (SPEC 6).
    pub fn add_library(&self, request_id: u64, path: &str, content: &str) {
        let root = huehuecoyotl_vfs::VPath::new(path.trim_end_matches(['/', '\\'])).to_string();
        let content = content_type(content);
        self.spawn(request_id, move |inner| async move {
            let meta = tokio::fs::metadata(&root)
                .await
                .map_err(|e| CoreError::from(huehuecoyotl_vfs::VfsError::from(e)))?;
            if !meta.is_dir() {
                return Err(CoreError {
                    kind: ErrorKind::Unsupported,
                    message: format!("{root} is not a folder"),
                });
            }
            let name = root
                .rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or(&root)
                .to_string();
            let r = root.clone();
            let id = inner
                .lib
                .write(move |c| catalog::add_library(c, &name, &r, content))
                .await?;
            enqueue_scan(&inner, id, jobs::priority::USER).await;
            Ok(Event::Libraries {
                request_id,
                libraries: library_list(&inner).await?,
            })
        });
    }

    pub fn remove_library(&self, request_id: u64, source_id: i64) {
        self.spawn(request_id, move |inner| async move {
            let row = inner
                .lib
                .read(move |c| catalog::source(c, source_id))
                .await?;
            // Saved copies of its files go with it (issue #3).
            crate::download::delete_saved(&inner, Some(source_id)).await?;
            inner
                .lib
                .write(move |c| catalog::remove_library(c, source_id))
                .await?;
            inner.forget_source(source_id);
            if let Some(r) = row.and_then(|r| r.credential_ref) {
                let _ = tokio::task::spawn_blocking(move || secrets::delete_password(&r)).await;
            }
            inner.mark_changed();
            Ok(Event::Libraries {
                request_id,
                libraries: library_list(&inner).await?,
            })
        });
    }

    /// Rescan one library folder, or all of them with `source_id == 0`.
    pub fn rescan(&self, source_id: i64) {
        let inner = self.inner.clone();
        self.rt.spawn(async move {
            let ids: Vec<i64> = match source_id {
                0 => inner
                    .lib
                    .read(catalog::libraries)
                    .await
                    .map(|l| l.into_iter().map(|l| l.id).collect())
                    .unwrap_or_default(),
                id => vec![id],
            };
            for id in ids {
                enqueue_scan(&inner, id, jobs::priority::USER).await;
            }
        });
    }

    /// One page of movies or shows (`kind`), or the children of `parent` (> 0).
    #[allow(clippy::too_many_arguments)]
    pub fn browse_items(
        &self,
        request_id: u64,
        kind: &str,
        parent: i64,
        sort: &str,
        filter: &str,
        offset: u32,
        limit: u32,
    ) {
        let q = Query {
            kind: kind.to_string(),
            parent: (parent > 0).then_some(parent),
            sort: match sort {
                "added" => Sort::Added,
                "year" => Sort::Year,
                "played" => Sort::Played,
                _ => Sort::Title,
            },
            // "" (everything), "unwatched", or "saved" (issue #4).
            unwatched_only: filter == "unwatched",
            saved_only: filter == "saved",
            offset,
            limit: limit.clamp(1, 500),
        };
        self.spawn(request_id, move |inner| async move {
            let (total, items) = inner.lib.read(move |c| catalog::browse(c, &q)).await?;
            let ids: Vec<i64> = items.iter().map(|s| s.id).collect();
            // Items on screen are processed first (SPEC 7).
            // Queued, not awaited: the writer may be busy with the job queue, and the answer must
            // not wait for it.
            drop(inner.lib.submit(move |c| jobs::bump_items(c, &ids)));
            Ok(Event::Page {
                request_id,
                total,
                offset,
                items,
            })
        });
    }

    pub fn recently_added(&self, request_id: u64, limit: u32) {
        self.spawn(request_id, move |inner| async move {
            let items = inner
                .lib
                .read(move |c| catalog::recently_added(c, limit))
                .await?;
            Ok(Event::Page {
                request_id,
                total: items.len() as u32,
                offset: 0,
                items,
            })
        });
    }

    /// `kind`: movie, show, artist, album or track; "" for all.
    pub fn search(&self, request_id: u64, text: &str, kind: &str) {
        let (text, kind) = (text.to_string(), kind.to_string());
        self.spawn(request_id, move |inner| async move {
            let items = inner
                .lib
                .read(move |c| catalog::search(c, &text, &kind, 60))
                .await?;
            Ok(Event::Page {
                request_id,
                total: items.len() as u32,
                offset: 0,
                items,
            })
        });
    }

    /// "This is a different movie" (issue #34): the version becomes its own item, locked against
    /// regrouping and sent to the review list. Answers with the new item's detail.
    pub fn split_file(&self, request_id: u64, file_id: i64) {
        self.spawn(request_id, move |inner| async move {
            let (new, old) = inner
                .lib
                .write(move |c| {
                    let (path, old): (String, i64) = c.query_row(
                        "SELECT path, item_id FROM file WHERE id = ?1",
                        [file_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )?;
                    let p = huehuecoyotl_scanner::parse::parse(&path);
                    let title = if p.title.is_empty() {
                        path.clone()
                    } else {
                        p.title.clone()
                    };
                    let new = catalog::split_file(c, file_id, &title, p.year)?;
                    jobs::enqueue(
                        c,
                        "match",
                        &serde_json::json!({ "item_id": new }).to_string(),
                        jobs::priority::USER,
                        Some(&format!("match:{new}")),
                    )?;
                    Ok((new, old))
                })
                .await?;
            inner.wake.notify_waiters();
            inner.sink.post(Event::ItemChanged { item_id: old });
            inner.mark_changed();
            let d = inner
                .lib
                .read(move |c| catalog::detail(c, new))
                .await?
                .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("item {new}")))?;
            Ok(Event::Detail {
                request_id,
                detail: Box::new(d),
            })
        });
    }

    /// "Analyse now" (issue #33): probe this file before the others.
    pub fn probe_now(&self, file_id: i64) {
        let inner = self.inner.clone();
        self.rt
            .spawn(async move { crate::probe::probe_now(&inner, file_id).await });
    }

    pub fn item_detail(&self, request_id: u64, item_id: i64) {
        self.spawn(request_id, move |inner| async move {
            let d = inner.lib.read(move |c| catalog::detail(c, item_id)).await?;
            let mut d = d.ok_or_else(|| CoreError {
                kind: ErrorKind::NotFound,
                message: format!("item {item_id}"),
            })?;
            let offline = inner.offline.lock().unwrap().clone();
            decorate(&mut d.files, &offline);
            drop(inner.lib.submit(move |c| jobs::bump_items(c, &[item_id])));
            Ok(Event::Detail {
                request_id,
                detail: Box::new(d),
            })
        });
    }

    /// Issue #48: one page of a season's episodes with their versions (probe data trimmed to
    /// labels and the summary line) and saved-copy state. The full probe of an episode comes with
    /// `item_detail`, when its details panel is opened.
    pub fn season_episodes(&self, request_id: u64, season_id: i64, offset: u32, limit: u32) {
        let limit = limit.clamp(1, 500);
        self.spawn(request_id, move |inner| async move {
            let (total, mut items) = inner
                .lib
                .read(move |c| catalog::episodes(c, season_id, offset, limit))
                .await?;
            let offline = inner.offline.lock().unwrap().clone();
            for e in &mut items {
                decorate(&mut e.files, &offline);
                for f in &mut e.files {
                    f.probe_json = compact_probe(&f.probe_json);
                }
            }
            let ids: Vec<i64> = items.iter().map(|e| e.item.id).collect();
            // Episodes on screen are matched and probed first (SPEC 7).
            // Queued, not awaited: the writer may be busy with the job queue, and the answer must
            // not wait for it.
            drop(inner.lib.submit(move |c| jobs::bump_items(c, &ids)));
            Ok(Event::Episodes {
                request_id,
                total,
                offset,
                items,
            })
        });
    }

    pub fn review(&self, request_id: u64) {
        self.spawn(request_id, move |inner| async move {
            let rows = inner.lib.read(catalog::review_list).await?;
            Ok(Event::Review {
                request_id,
                items: rows
                    .into_iter()
                    .map(|(item, candidates)| ReviewItem { item, candidates })
                    .collect(),
            })
        });
    }

    /// Search the provider for a different match (the "fix match" dialog).
    pub fn find_matches(&self, request_id: u64, item_id: i64, title: &str, year: u16) {
        let title = title.trim().to_string();
        self.spawn(request_id, move |inner| async move {
            let p = inner.provider().ok_or_else(|| CoreError {
                kind: ErrorKind::Auth,
                message: "No TMDB API key is set.".into(),
            })?;
            let t = inner
                .lib
                .read(move |c| catalog::match_target(c, item_id))
                .await?
                .ok_or_else(|| CoreError {
                    kind: ErrorKind::NotFound,
                    message: "item".into(),
                })?;
            let ty = if t.kind == "show" {
                MediaType::Show
            } else {
                MediaType::Movie
            };
            let query = if title.is_empty() {
                t.title.clone()
            } else {
                title
            };
            let year = if year > 0 { Some(year) } else { None };
            let mut found = p.search(ty, &query, year).await.map_err(meta_err)?;
            let (_, scores) = huehuecoyotl_metadata::matching::decide(&mut found, &query, year);
            let candidates = found
                .into_iter()
                .zip(scores)
                .map(|(c, s)| catalog::CandidateRow {
                    provider_id: c.id,
                    title: c.title,
                    year: c.year,
                    score: s,
                    poster_path: c.poster_path,
                    overview: c.overview,
                })
                .collect();
            Ok(Event::Candidates {
                request_id,
                item_id,
                candidates,
            })
        });
    }

    /// The user picked a match: lock it and fetch its details.
    pub fn choose_match(&self, item_id: i64, provider_id: i64) {
        let inner = self.inner.clone();
        self.rt.spawn(async move {
            let item_id = match inner
                .lib
                .write(move |c| catalog::lock(c, item_id, Some(provider_id)))
                .await
            {
                Ok(id) => id,
                Err(e) => {
                    tracing::error!("locking match failed: {e}");
                    return;
                }
            };
            worker::enqueue_details(&inner, item_id, provider_id, jobs::priority::USER).await;
            inner.mark_changed();
        });
    }

    /// The TMDB key's state (issue #52). Answers once the start-up has looked for the key, so a
    /// stored key is never reported missing because the keystore answered late.
    pub fn key_status(&self, request_id: u64) {
        self.spawn(request_id, move |inner| async move {
            let mut loaded = inner.key_loaded.subscribe();
            let _ = loaded.wait_for(|done| *done).await;
            Ok(inner.key_status_event(request_id, String::new()))
        });
    }

    /// Validate a TMDB key with a test request, store it in the OS keystore and start fetching.
    /// Without a keystore the key works until the app closes and the UI says why; it is never
    /// kept anywhere else.
    pub fn set_tmdb_key(&self, request_id: u64, key: &str) {
        let key = key.trim().to_string();
        self.spawn(request_id, move |inner| async move {
            if key.is_empty() {
                return Ok(inner.key_status_event(request_id, "Please enter a key.".into()));
            }
            let tmdb = Tmdb::new(key.clone(), inner.language.clone()).map_err(internal)?;
            match tmdb.validate().await {
                Ok(()) => {}
                Err(MetaError::Auth) => {
                    return Ok(inner.key_status_event(request_id, "TMDB rejected this key.".into()));
                }
                Err(e) => {
                    return Ok(inner
                        .key_status_event(request_id, format!("TMDB could not be reached: {e}")));
                }
            }
            let stored = tokio::task::spawn_blocking(move || secrets::store_tmdb_key(&key))
                .await
                .map_err(internal)?;
            let source = match &stored {
                Ok(()) => {
                    *inner.keystore.lock().unwrap() = Ok(());
                    KeySource::Keystore
                }
                Err(why) => {
                    tracing::warn!("the TMDB key could not be stored in the keystore");
                    *inner.keystore.lock().unwrap() = Err(why.clone());
                    KeySource::Session
                }
            };
            inner.set_provider(Some(Arc::new(tmdb)), Some(source));
            inner.set_key_validated(now_s()).await;
            Ok(inner.key_status_event(request_id, stored.err().unwrap_or_default()))
        });
    }

    /// "Test key" (issue #52): ask TMDB whether the key in use is accepted.
    pub fn test_tmdb_key(&self, request_id: u64) {
        self.spawn(request_id, move |inner| async move {
            let Some(p) = inner.provider() else {
                return Ok(inner.key_status_event(request_id, "No key is set.".into()));
            };
            let message = match p.validate().await {
                Ok(()) => {
                    tracing::info!("TMDB key tested: accepted");
                    inner.set_key_validated(now_s()).await;
                    String::new()
                }
                Err(MetaError::Auth) => {
                    tracing::info!("TMDB key tested: rejected");
                    "TMDB rejected this key. Replace it with a new one.".into()
                }
                Err(e) => {
                    tracing::info!("TMDB key tested: TMDB not reached ({e})");
                    format!("TMDB could not be reached: {e}")
                }
            };
            Ok(inner.key_status_event(request_id, message))
        });
    }

    /// "Remove key": the keystore entry goes too, so no key is found after a restart.
    pub fn clear_tmdb_key(&self, request_id: u64) {
        self.spawn(request_id, move |inner| async move {
            let removed = tokio::task::spawn_blocking(secrets::clear_tmdb_key)
                .await
                .map_err(internal)?;
            let from_env = *inner.key_source.lock().unwrap() == Some(KeySource::Environment);
            if !from_env {
                inner.set_provider(None, None);
                inner.set_key_validated(0).await;
            }
            Ok(inner.key_status_event(request_id, removed.err().unwrap_or_default()))
        });
    }

    /// For tests and the CLI: use this provider instead of TMDB.
    pub fn set_provider(&self, p: Arc<dyn MetadataProvider>) {
        let _rt = self.rt.enter();
        self.inner
            .set_provider(Some(p), Some(KeySource::Environment));
    }
}

/// A library folder's content type: `music`, anything else is `video`.
pub(crate) fn content_type(s: &str) -> &'static str {
    if s.eq_ignore_ascii_case("music") {
        "music"
    } else {
        "video"
    }
}

fn now_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Whether each file's source can be reached, and its edition from the file name (issue #34).
fn decorate(files: &mut [catalog::FileInfo], offline: &std::collections::HashSet<i64>) {
    for f in files {
        f.online = !offline.contains(&f.source_id);
        let name = f.path.rsplit('/').next().unwrap_or(&f.path);
        f.edition = huehuecoyotl_scanner::parse::edition(name).unwrap_or_default();
    }
}

/// Issue #48: the part of a probe that version labels, badges and the summary line use (the first
/// video track; codec, profile, layout and default flag of each audio track; the subtitle
/// languages; duration and overall bit rate). "" stays "".
pub(crate) fn compact_probe(json: &str) -> String {
    use serde_json::{Map, Value};
    let Ok(Value::Object(full)) = serde_json::from_str::<Value>(json) else {
        return String::new();
    };
    let pick = |v: &Value, keys: &[&str]| -> Value {
        let mut m = Map::new();
        for k in keys {
            if let Some(x) = v.get(*k)
                && !x.is_null()
            {
                m.insert((*k).to_string(), x.clone());
            }
        }
        Value::Object(m)
    };
    let list = |key: &str, keys: &[&str], first_only: bool| -> Value {
        let items = full
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let n = if first_only {
            items.len().min(1)
        } else {
            items.len()
        };
        Value::Array(items[..n].iter().map(|v| pick(v, keys)).collect())
    };
    let mut out = match pick(
        &Value::Object(full.clone()),
        &["schema", "error", "duration_ms", "bitrate"],
    ) {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    out.insert(
        "video".into(),
        list(
            "video",
            &["codec", "width", "height", "hdr", "dv_profile", "bit_depth"],
            true,
        ),
    );
    out.insert(
        "audio".into(),
        list("audio", &["codec", "profile", "layout", "default"], false),
    );
    out.insert("subtitles".into(), list("subtitles", &["language"], false));
    out.insert("compact".into(), Value::Bool(true));
    Value::Object(out).to_string()
}

fn meta_err(e: MetaError) -> CoreError {
    let kind = match e {
        MetaError::Auth => ErrorKind::Auth,
        MetaError::Network(_) | MetaError::RateLimited(_) => ErrorKind::Unreachable,
        MetaError::NotFound => ErrorKind::NotFound,
        MetaError::Other(_) => ErrorKind::Internal,
    };
    CoreError {
        kind,
        message: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_probe_keeps_what_labels_need() {
        let full = r#"{"schema":3,"source":"ffmpeg","container":"matroska","duration_ms":1200000,"size":5,
            "bitrate":{"bps":8000000,"estimated":false},"chapters":12,
            "video":[{"codec":"hevc","profile":"Main 10","width":3840,"height":2160,"bit_depth":10,"hdr":"Dolby Vision",
                      "dv_profile":"8.1","primaries":"bt2020","fps":23.976},{"codec":"mjpeg","width":1,"height":1}],
            "audio":[{"codec":"truehd","profile":"Dolby TrueHD + Dolby Atmos","layout":"7.1","channels":8,"default":true,
                      "language":"eng","sample_rate":48000},{"codec":"ac3","layout":"5.1","default":false}],
            "subtitles":[{"language":"eng","format":"subrip","forced":false},{"language":"ger","format":"pgs"}]}"#;
        let v: serde_json::Value = serde_json::from_str(&compact_probe(full)).unwrap();
        assert_eq!(v["duration_ms"], 1200000);
        assert_eq!(v["bitrate"]["bps"], 8000000);
        assert_eq!(v["video"].as_array().unwrap().len(), 1);
        assert_eq!(v["video"][0]["dv_profile"], "8.1");
        assert!(v["video"][0].get("primaries").is_none());
        assert_eq!(v["audio"][0]["profile"], "Dolby TrueHD + Dolby Atmos");
        assert!(v["audio"][0].get("sample_rate").is_none());
        assert_eq!(v["audio"].as_array().unwrap().len(), 2);
        assert_eq!(v["subtitles"].as_array().unwrap().len(), 2);
        assert!(v["subtitles"][0].get("format").is_none());
        assert!(v.get("chapters").is_none());
        assert_eq!(v["compact"], true);
        // Not analysed, or unreadable JSON: nothing.
        assert_eq!(compact_probe(""), "");
        let err: serde_json::Value =
            serde_json::from_str(&compact_probe(r#"{"schema":3,"error":"bad file"}"#)).unwrap();
        assert_eq!(err["error"], "bad file");
    }

    #[test]
    fn decorate_marks_offline_sources_and_editions() {
        let mut files = vec![
            catalog::FileInfo {
                source_id: 3,
                path: "/share/Alien (1979) Director's Cut.mkv".into(),
                ..Default::default()
            },
            catalog::FileInfo {
                source_id: 2,
                path: "/m/Alien.mkv".into(),
                ..Default::default()
            },
        ];
        decorate(&mut files, &[3].into_iter().collect());
        assert!(!files[0].online && files[1].online);
        assert_eq!(files[0].edition, "Director's Cut");
    }
}
