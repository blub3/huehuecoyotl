//! Add to Library (3.0, issue #154, SPEC 7): the import job. One file per run, like the library's
//! move: a copy is written under a temporary name, compared, renamed, and only then joins its
//! item. The original is only read. Nothing that already exists at the destination is replaced.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use huehuecoyotl_library::imports::{self, Plan, Work};
use huehuecoyotl_library::{catalog, copies, copy_meta, jobs};
use huehuecoyotl_scanner::copy_tags;
use huehuecoyotl_vfs::{LocalSource, Source, VPath, VfsError};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::managed::{Scoped, reader, same_content, source_connection};
use crate::{CoreError, ErrorKind, Inner};

type CResult<T> = Result<T, CoreError>;

const CHUNK: u32 = 1 << 20;
const RETRY_SECONDS: i64 = 15;

pub(crate) async fn enqueue(inner: &Arc<Inner>) {
    let _ = inner
        .lib
        .write(|c| {
            jobs::enqueue(
                c,
                "library_import",
                "{}",
                jobs::priority::NORMAL,
                Some("library_import"),
            )
        })
        .await;
    inner.wake.notify_waiters();
}

/// The file to read: its saved offline copy when there is one, else the original.
async fn open_original(inner: &Arc<Inner>, file: i64) -> CResult<(Arc<dyn Source>, VPath, u64)> {
    let (row, saved) = inner
        .lib
        .read(move |c| {
            let row: (i64, String, i64) = c.query_row(
                "SELECT source_id, path, coalesce(size, 0) FROM file WHERE id = ?1",
                [file],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            Ok((row, copies::saved_for_file(c, file)?))
        })
        .await?;
    if let Some(s) = saved.filter(|s| s.state == "done")
        && let Ok(m) = tokio::fs::metadata(&s.path).await
    {
        return Ok((Arc::new(LocalSource), VPath::new(s.path), m.len()));
    }
    let (source, rel, size) = row;
    let src_row = inner
        .lib
        .read(move |c| catalog::source(c, source))
        .await?
        .ok_or_else(|| CoreError::new(ErrorKind::NotFound, "the file's source"))?;
    let root = VPath::new(src_row.root.unwrap_or_else(|| "/".into()));
    let src = source_connection(inner, source).await?;
    Ok((src, root.join(&rel), size as u64))
}

/// Download to `path`, continuing a partial file.
async fn fetch(src: &dyn Source, from: &VPath, size: u64, path: &Path) -> Result<(), VfsError> {
    let mut at = tokio::fs::metadata(path).await.map_or(0, |m| m.len());
    if at > size {
        tokio::fs::remove_file(path).await?;
        at = 0;
    }
    let mut f = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    while at < size {
        let b = src.read_at(from, at, CHUNK).await?;
        if b.is_empty() {
            return Err(VfsError::Internal(format!("{from} ended early")));
        }
        f.write_all(&b).await?;
        at += b.len() as u64;
    }
    f.flush().await?;
    f.sync_all().await?;
    Ok(())
}

fn local_chunks(path: PathBuf, size: u64) -> huehuecoyotl_vfs::Chunks {
    reader(
        Arc::new(LocalSource),
        VPath::new(path.to_string_lossy()),
        size,
    )
}

/// Tags and cover for a file of this job.
async fn tags_for(
    inner: &Arc<Inner>,
    w: &Work,
) -> CResult<(Option<copy_meta::CopyTags>, Option<Vec<u8>>)> {
    let (kind, item, album) = (w.kind.clone(), w.item, w.album);
    let write = inner
        .lib
        .read(move |c| {
            let t = huehuecoyotl_library::managed::type_settings(c)?;
            Ok(t[&kind]["writeTags"].as_bool().unwrap_or(true))
        })
        .await?;
    if !write {
        return Ok((None, None));
    }
    let kind = w.kind.clone();
    let (tags, poster) = inner
        .lib
        .read(move |c| {
            let tags = if kind == "music" {
                copy_meta::track_tags(c, item)?
            } else {
                copy_meta::book_tags(c, item)?
            };
            let owner = if kind == "music" { album.unwrap_or(item) } else { item };
            let poster: Option<String> = c
                .query_row(
                    "SELECT local_path FROM artwork WHERE item_id = ?1 AND role = 'poster' AND local_path IS NOT NULL LIMIT 1",
                    [owner],
                    |r| r.get(0),
                )
                .ok();
            Ok((tags, poster))
        })
        .await?;
    let cover = match poster {
        Some(p) => tokio::fs::read(p).await.ok(),
        None => None,
    };
    Ok((tags, cover))
}

enum Step {
    Copied,
    /// Something else is at the destination: the user decides.
    Clash,
}

/// Copy one file into the library.
async fn copy_one(inner: &Arc<Inner>, w: &Work) -> CResult<Step> {
    let kind = w.kind.clone();
    let target: i64 = inner
        .lib
        .read(move |c| {
            Ok(
                c.query_row("SELECT id FROM source WHERE managed = ?1", [&kind], |r| {
                    r.get(0)
                })?,
            )
        })
        .await
        .map_err(|_: huehuecoyotl_library::DbError| {
            CoreError::new(ErrorKind::Unsupported, "no library folder is set up")
        })?;
    let row = inner
        .lib
        .read(move |c| catalog::source(c, target))
        .await?
        .ok_or_else(|| CoreError::new(ErrorKind::NotFound, "the library folder"))?;
    let root = VPath::new(row.root.clone().unwrap_or_else(|| "/".into()));
    let to = source_connection(inner, target).await?;
    let scoped = Scoped::new(to.clone(), root.clone());
    let dest = root.join(&w.dest);
    if let Some(dir) = dest.parent() {
        scoped.ensure_dir(&dir).await?;
    }
    let (from, old, size) = open_original(inner, w.file).await?;
    let existing = to.stat(&dest).await.ok();

    if w.kind == "music" || w.kind == "audiobooks" {
        // Tagging happens on a local copy; the share only ever sees the finished file.
        let dir = inner.data_dir.join("imports");
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(VfsError::from)?;
        let (part, ready) = (
            dir.join(format!("{}-{}.part", w.job, w.row)),
            dir.join(format!(
                "{}-{}.{}",
                w.job,
                w.row,
                w.dest.rsplit('.').next().unwrap_or("bin")
            )),
        );
        if tokio::fs::metadata(&ready).await.is_err() {
            fetch(&*from, &old, size, &part).await?;
            let (tags, cover) = tags_for(inner, w).await?;
            if let Some(t) = tags {
                let (p, r) = (part.clone(), ready.clone());
                tokio::task::spawn_blocking(move || {
                    // lofty picks the format by content and extension.
                    std::fs::rename(&p, &r)?;
                    copy_tags::write_tags(&r, &t, cover.as_deref())
                        .map_err(|e| std::io::Error::other(e.to_string()))
                })
                .await
                .map_err(|e| CoreError::new(ErrorKind::Internal, e.to_string()))?
                .map_err(|e| CoreError::new(ErrorKind::Internal, format!("tags: {e}")))?;
            } else {
                tokio::fs::rename(&part, &ready)
                    .await
                    .map_err(VfsError::from)?;
            }
        }
        let local_size = tokio::fs::metadata(&ready)
            .await
            .map_err(VfsError::from)?
            .len();
        let local = VPath::new(ready.to_string_lossy());
        match existing {
            Some(e)
                if e.size == local_size
                    && same_content(&LocalSource, &local, &*to, &dest, local_size).await? => {}
            Some(_) => return Ok(Step::Clash),
            None => {
                scoped
                    .write(&dest, local_chunks(ready.clone(), local_size), None)
                    .await?;
                if !same_content(&LocalSource, &local, &*to, &dest, local_size).await? {
                    let _ = to.remove(&dest).await;
                    return Err(CoreError::new(
                        ErrorKind::Internal,
                        "the copy differs from what was written",
                    ));
                }
            }
        }
        let _ = tokio::fs::remove_file(&ready).await;
    } else {
        match existing {
            Some(e) if e.size == size && same_content(&*from, &old, &*to, &dest, size).await? => {}
            Some(_) => return Ok(Step::Clash),
            None => {
                scoped
                    .write(&dest, reader(from.clone(), old.clone(), size), None)
                    .await?;
                if !same_content(&*from, &old, &*to, &dest, size).await? {
                    let _ = to.remove(&dest).await;
                    return Err(CoreError::new(
                        ErrorKind::Internal,
                        "the copy differs from the original",
                    ));
                }
            }
        }
        write_nfo(inner, &scoped, &*to, w, &dest).await?;
    }
    if w.kind == "music" {
        write_cover(inner, &scoped, &*to, w, &dest).await;
    }
    let st = to.stat(&dest).await?;
    let work = w.clone();
    inner
        .lib
        .write(move |c| imports::add_copy(c, &work, target, st.size as i64, st.mtime))
        .await?;
    Ok(Step::Copied)
}

async fn one_small_file(scoped: &Scoped, path: &VPath, body: Vec<u8>) -> Result<(), VfsError> {
    let (tx, rx) = mpsc::channel(1);
    let _ = tx.send(Ok(Bytes::from(body))).await;
    drop(tx);
    scoped.write(path, rx, None).await.map(|_| ())
}

/// The `.nfo` beside a movie or episode; one that is already there stays.
async fn write_nfo(
    inner: &Arc<Inner>,
    scoped: &Scoped,
    to: &dyn Source,
    w: &Work,
    dest: &VPath,
) -> Result<(), VfsError> {
    let item = w.item;
    let Ok(Some(nfo)) = inner.lib.read(move |c| copy_meta::video_nfo(c, item)).await else {
        return Ok(());
    };
    let name = dest.as_str();
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let path = VPath::new(format!("{stem}.nfo"));
    if to.stat(&path).await.is_ok() {
        return Ok(());
    }
    one_small_file(scoped, &path, nfo.into_bytes()).await
}

/// `cover.jpg` once per album folder. A failure here does not stop the import.
async fn write_cover(inner: &Arc<Inner>, scoped: &Scoped, to: &dyn Source, w: &Work, dest: &VPath) {
    let Some(album) = w.album else { return };
    let Some(dir) = dest.parent() else { return };
    let path = dir.join("cover.jpg");
    if to.stat(&path).await.is_ok() {
        return;
    }
    let poster: Option<String> = inner
        .lib
        .read(move |c| {
            Ok(c.query_row(
                "SELECT local_path FROM artwork WHERE item_id = ?1 AND role = 'poster' AND local_path IS NOT NULL LIMIT 1",
                [album],
                |r| r.get(0),
            )
            .ok())
        })
        .await
        .unwrap_or(None);
    if let Some(bytes) = match poster {
        Some(p) => tokio::fs::read(p).await.ok(),
        None => None,
    } && let Err(e) = one_small_file(scoped, &path, bytes).await
    {
        tracing::warn!("no cover.jpg for an imported album: {e}");
    }
}

/// The persisted job: one file per run. Returns the seconds until the next run, `None` when
/// nothing is left.
pub(crate) async fn run(inner: &Arc<Inner>) -> Option<i64> {
    let Ok(next) = inner.lib.read(imports::next).await else {
        return Some(RETRY_SECONDS);
    };
    let Some(w) = next else {
        let _ = inner.lib.write(|c| imports::settle(c)).await;
        return None;
    };
    let job = w.job;
    let result = copy_one(inner, &w).await;
    let update = match result {
        Ok(Step::Copied) => inner.lib.write(|c| imports::settle(c)).await,
        Ok(Step::Clash) => {
            inner
                .lib
                .write(move |c| imports::set_state(c, job, "review", Some("clash"), ""))
                .await
        }
        // The source or the library is away: the job waits and goes on from this file.
        Err(e) if e.kind == ErrorKind::Unreachable => {
            let note = e.message.clone();
            let _ = inner
                .lib
                .write(move |c| {
                    c.execute(
                        "UPDATE import_job SET note = ?2 WHERE id = ?1",
                        huehuecoyotl_library::rusqlite::params![job, note],
                    )?;
                    Ok(())
                })
                .await;
            inner.mark_changed();
            return Some(RETRY_SECONDS);
        }
        Err(e) => {
            let note = e.message;
            inner
                .lib
                .write(move |c| imports::set_state(c, job, "failed", None, &note))
                .await
        }
    };
    if let Err(e) = update {
        tracing::error!("import {job}: {e}");
    }
    inner.mark_changed();
    Some(0)
}

/// What the sheet shows before anything is copied.
async fn sheet(inner: &Arc<Inner>, item: i64) -> CResult<Value> {
    let plans: Vec<Plan> = inner.lib.read(move |c| imports::plan(c, item)).await?;
    let place = inner.lib.read(huehuecoyotl_library::managed::place).await?;
    let files: usize = plans.iter().map(|p| p.files.len()).sum();
    let size: i64 = plans.iter().flat_map(|p| &p.files).map(|f| f.size).sum();
    let already: usize = plans.iter().map(|p| p.already).sum();
    let review = plans.iter().filter(|p| p.review.is_some()).count();
    let mut from = String::new();
    let mut tag_rows = Vec::new();
    if let Some(first) = plans.iter().find_map(|p| p.files.first().map(|f| (p, f))) {
        let (p, f) = first;
        let file = f.file;
        let display = inner
            .lib
            .read(move |c| {
                let (s, rel): (i64, String) = c.query_row(
                    "SELECT source_id, path FROM file WHERE id = ?1",
                    [file],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                let row = catalog::source(c, s)?;
                Ok(row.map(|r| {
                    let url = crate::network::display_url(
                        &r.kind,
                        &r.config_json,
                        r.root.as_deref().unwrap_or(""),
                    );
                    let dir = rel.rsplit_once('/').map_or("", |(d, _)| d);
                    if dir.is_empty() {
                        url
                    } else {
                        format!("{url}/{dir}")
                    }
                }))
            })
            .await?;
        from = display.unwrap_or_default();
        if p.kind == "music" || p.kind == "audiobooks" {
            tag_rows = tag_preview(inner, p, f.file, f.item).await;
        }
    }
    let root = place.as_ref().map(|p| p.root.clone()).unwrap_or_default();
    let list: Vec<Value> = plans
        .iter()
        .map(|p| {
            let folder = huehuecoyotl_library::managed::TYPES
                .iter()
                .find(|(k, ..)| *k == p.kind)
                .map_or("", |(_, f, _)| *f);
            json!({
                "item": p.item, "kind": p.kind, "title": p.title, "subtitle": p.subtitle,
                "folder": p.folder, "to": format!("{}/{folder}/{}", root.trim_end_matches('/'), p.folder),
                "files": p.files.len(), "already": p.already, "review": p.review,
                "names": p.files.iter().take(6)
                    .map(|f| f.dest.rsplit('/').next().unwrap_or("").to_string())
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({
        "item": item,
        "configured": place.is_some(),
        "plans": list,
        "files": files,
        "size": size,
        "already": already,
        "review": review,
        "from": from,
        "tags": tag_rows,
    }))
}

/// "In the file now" and "In the copy" for the first file.
async fn tag_preview(inner: &Arc<Inner>, p: &Plan, file: i64, item: i64) -> Vec<Value> {
    let kind = p.kind.clone();
    let Ok(Some(after)) = inner
        .lib
        .read(move |c| {
            if kind == "music" {
                copy_meta::track_tags(c, item)
            } else {
                copy_meta::book_tags(c, item)
            }
        })
        .await
    else {
        return vec![];
    };
    let before = match open_original(inner, file).await {
        Ok((s, path, size)) => huehuecoyotl_scanner::tags::read(&*s, &path, size)
            .await
            .ok()
            .map(|r| r.tags),
        Err(_) => None,
    }
    .unwrap_or_default();
    let names = |v: &[huehuecoyotl_scanner::tags::Credit]| {
        v.iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let number = |n: Option<u32>, t: Option<u32>| match (n, t) {
        (Some(n), Some(t)) => format!("{n} / {t}"),
        (Some(n), None) => n.to_string(),
        _ => String::new(),
    };
    let ids_now = [
        &before.mb_release,
        &before.mb_release_group,
        &before.mb_recording,
        &before.mb_track,
    ]
    .iter()
    .filter(|x| x.is_some())
    .count()
        + usize::from(!before.mb_artists.is_empty());
    let ids_then = [
        &after.mb_release,
        &after.mb_release_group,
        &after.mb_recording,
        &after.mb_track,
    ]
    .iter()
    .filter(|x| !x.is_empty())
    .count()
        + usize::from(!after.mb_artists.is_empty());
    let keep = |then: String, now: &str| {
        if then.is_empty() {
            now.to_string()
        } else {
            then
        }
    };
    let row = |field: &str, now: String, then: String| {
        let then = keep(then, &now);
        json!({ "field": field, "now": now, "then": then, "changed": now != then })
    };
    vec![
        row(
            "title",
            before.title.clone().unwrap_or_default(),
            after.title.clone(),
        ),
        row("artist", names(&before.artists), after.artists.join(", ")),
        row(
            "album",
            before.album.clone().unwrap_or_default(),
            after.album.clone(),
        ),
        row(
            "date",
            before.date.clone().unwrap_or_default(),
            after.date.clone(),
        ),
        row(
            "track",
            number(before.track_no, before.track_total),
            number(after.track, after.track_total),
        ),
        row(
            "ids",
            if ids_now > 0 {
                ids_now.to_string()
            } else {
                String::new()
            },
            if ids_then > 0 {
                ids_then.to_string()
            } else {
                String::new()
            },
        ),
        row("isrc", before.isrc.join(", "), after.isrc.join(", ")),
        row(
            "barcode",
            before.barcode.clone().unwrap_or_default(),
            after.barcode.clone(),
        ),
    ]
}

/// Issue #154's actions of `library_call`.
pub(crate) async fn action(
    inner: &Arc<Inner>,
    name: &str,
    args: &Value,
) -> CResult<Option<(&'static str, Value)>> {
    let id = |k: &str| args[k].as_i64().unwrap_or(0);
    Ok(Some(match name {
        "importPlan" => ("importSheet", sheet(inner, id("item")).await?),
        "importStart" => {
            let item = id("item");
            let configured = inner
                .lib
                .read(huehuecoyotl_library::managed::place)
                .await?
                .is_some();
            if !configured {
                return Err(CoreError::new(
                    ErrorKind::Unsupported,
                    "choose a library folder under Settings → Library first",
                ));
            }
            let plans = inner.lib.read(move |c| imports::plan(c, item)).await?;
            inner
                .lib
                .write(move |c| imports::start(c, &plans, false))
                .await?;
            enqueue(inner).await;
            inner.mark_changed();
            ("imports", inner.lib.read(imports::list).await?)
        }
        "importSource" => {
            let source = id("source");
            let items = inner
                .lib
                .read(move |c| imports::items_of_source(c, source))
                .await?;
            for item in items {
                let plans = inner.lib.read(move |c| imports::plan(c, item)).await?;
                inner
                    .lib
                    .write(move |c| imports::start(c, &plans, false))
                    .await?;
            }
            enqueue(inner).await;
            inner.mark_changed();
            ("imports", inner.lib.read(imports::list).await?)
        }
        "imports" => {
            let list = inner.lib.read(imports::list).await?;
            // A job added just as the worker finished is picked up here.
            if list["active"].as_u64().unwrap_or(0) > 0 {
                enqueue(inner).await;
            }
            ("imports", list)
        }
        "importPause" | "importResume" | "importRetry" | "importSkip" => {
            let job = id("job");
            let (state, note) = match name {
                "importPause" => ("paused", ""),
                "importSkip" => ("skipped", ""),
                _ => ("queued", ""),
            };
            inner
                .lib
                .write(move |c| imports::set_state(c, job, state, None, note))
                .await?;
            if state == "queued" {
                enqueue(inner).await;
            }
            inner.mark_changed();
            ("imports", inner.lib.read(imports::list).await?)
        }
        _ => return Ok(None),
    }))
}
