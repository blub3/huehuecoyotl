//! The managed library's requests and its move (3.0, issue #150, SPEC 6). The app writes only
//! inside the chosen folder: every write goes through [`Scoped`], which refuses other paths.

use std::sync::Arc;

use bytes::Bytes;
use huehuecoyotl_library::{jobs, managed, naming, shares as share_db};
use huehuecoyotl_vfs::write::check_writable;
use huehuecoyotl_vfs::{Expected, LocalSource, Source, VPath, VfsError, safe_write};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::library::enqueue_scan;
use crate::{Core, CoreError, ErrorKind, Event, Inner};

type CResult<T> = Result<T, CoreError>;

const CHUNK: u32 = 1 << 20;
/// How soon a paused move tries again.
const RETRY_SECONDS: i64 = 15;
pub const LIBRARY_KINDS: &[&str] = &["library_move", "library_import"];

/// Writes confined to one folder.
pub(crate) struct Scoped {
    src: Arc<dyn Source>,
    root: VPath,
}

/// `path` when it lies inside `root`: no `.` or `..` segment can lead out of it.
pub(crate) fn inside(root: &VPath, path: &VPath) -> Result<(), VfsError> {
    let r = root.as_str().trim_end_matches('/');
    let p = path.as_str();
    let within = p == r || p.starts_with(&format!("{r}/"));
    if !within || p.split('/').any(|s| s == ".." || s == ".") {
        return Err(VfsError::ReadOnly(format!(
            "{p} is outside the library folder"
        )));
    }
    Ok(())
}

impl Scoped {
    pub(crate) fn new(src: Arc<dyn Source>, root: VPath) -> Self {
        Self { src, root }
    }

    /// Create the folders down to `dir`, which must be inside the root.
    pub(crate) async fn ensure_dir(&self, dir: &VPath) -> Result<(), VfsError> {
        inside(&self.root, dir)?;
        let mut missing = Vec::new();
        let mut at = dir.clone();
        while at != self.root && self.src.stat(&at).await.is_err() {
            missing.push(at.clone());
            match at.parent() {
                Some(p) => at = p,
                None => break,
            }
        }
        for d in missing.into_iter().rev() {
            if let Err(e) = self.src.create_dir(&d).await
                && !self.src.stat(&d).await.is_ok_and(|s| s.is_dir)
            {
                return Err(e);
            }
        }
        Ok(())
    }

    pub(crate) async fn write(
        &self,
        path: &VPath,
        data: huehuecoyotl_vfs::Chunks,
        expected: Option<Expected>,
    ) -> Result<u64, VfsError> {
        inside(&self.root, path)?;
        safe_write(&*self.src, path, data, expected).await
    }
}

/// Where the user wants the library.
fn parse_place(args: &Value) -> CResult<managed::Place> {
    let path = args["path"].as_str().unwrap_or("").trim();
    if path.is_empty() {
        return Err(CoreError::new(ErrorKind::Unsupported, "no folder chosen"));
    }
    let share = (args["where"] == "share")
        .then(|| args["share"].as_i64())
        .flatten();
    if args["where"] == "share" && share.is_none() {
        return Err(CoreError::new(ErrorKind::Unsupported, "no share chosen"));
    }
    let root = if share.is_some() {
        format!("/{}", path.trim_matches('/'))
    } else {
        path.trim_end_matches('/').to_string()
    };
    Ok(managed::Place { share, root })
}

async fn connection(inner: &Arc<Inner>, p: &managed::Place) -> CResult<Arc<dyn Source>> {
    Ok(match p.share {
        Some(s) => crate::shares::share_connection(inner, s).await?,
        None => Arc::new(LocalSource),
    })
}

pub(crate) async fn source_connection(inner: &Arc<Inner>, id: i64) -> CResult<Arc<dyn Source>> {
    let row = inner
        .lib
        .read(move |c| huehuecoyotl_library::catalog::source(c, id))
        .await?
        .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("source {id}")))?;
    Ok(if row.kind == "local" {
        Arc::new(LocalSource)
    } else {
        inner.remote_source(id).await?
    })
}

fn free_space(p: &managed::Place) -> Option<u64> {
    if p.share.is_some() {
        return None; // Not every protocol reports it.
    }
    crate::available_space(std::path::Path::new(&p.root)).ok()
}

/// The checks before a place becomes the library.
async fn check(inner: &Arc<Inner>, p: &managed::Place) -> CResult<Value> {
    let (current, (files, bytes), overlap) = {
        let q = p.clone();
        inner
            .lib
            .read(move |c| {
                Ok((
                    managed::place(c)?,
                    managed::copies(c)?,
                    managed::overlapping(c, &q)?,
                ))
            })
            .await?
    };
    let mut checks = Vec::new();
    let mut add = |key: &str, ok: bool, detail: String| {
        checks.push(json!({ "key": key, "ok": ok, "detail": detail }));
        ok
    };
    let dir = VPath::new(p.root.clone());
    let src = connection(inner, p).await;
    let reachable = match &src {
        Ok(s) => match s.stat(&dir).await {
            Ok(e) if e.is_dir => add("reachable", true, String::new()),
            Ok(_) => add("reachable", false, "notFolder".into()),
            Err(VfsError::NotFound(_)) => add("reachable", false, "notFound".into()),
            Err(e) => add("reachable", false, e.to_string()),
        },
        Err(e) => add("reachable", false, e.message.clone()),
    };
    let writable = if let (true, Ok(s)) = (reachable, &src) {
        match check_writable(&**s, &dir).await {
            Ok(()) => add("writable", true, String::new()),
            Err(VfsError::ReadOnly(_) | VfsError::Auth(_)) => {
                add("writable", false, "readOnly".into())
            }
            Err(e) => add("writable", false, e.to_string()),
        }
    } else {
        add("writable", false, "notChecked".into())
    };
    let free = free_space(p);
    let room = free.is_none_or(|f| f >= bytes as u64);
    add(
        "space",
        room,
        if free.is_some() {
            String::new()
        } else {
            "unknown".into()
        },
    );
    let same = current.as_ref() == Some(p);
    let clear = overlap.is_none() && !same;
    add(
        "alone",
        clear,
        if same {
            "current".into()
        } else {
            overlap.unwrap_or_default()
        },
    );
    Ok(json!({
        "ok": reachable && writable && room && clear,
        "checks": checks,
        "free": free,
        "needed": bytes,
        "copies": files,
        "current": current.map(|c| c.root),
        "path": p.root,
    }))
}

async fn status(inner: &Arc<Inner>) -> CResult<Value> {
    let (place, (files, bytes), (left, left_bytes), types, list, watchable) = inner
        .lib
        .read(|c| {
            Ok((
                managed::place(c)?,
                managed::copies(c)?,
                managed::move_left(c)?,
                managed::type_settings(c)?,
                share_db::list(c)?,
                managed::watchable(c)?,
            ))
        })
        .await?;
    let offline = inner.offline.lock().unwrap().clone();
    let shares: Vec<Value> = list
        .iter()
        .map(|s| {
            let away =
                !s.folders.is_empty() && s.folders.iter().all(|f| offline.contains(&f.source_id));
            json!({ "id": s.id, "name": s.name, "protocol": s.kind,
                    "state": if away { "offline" } else { "online" } })
        })
        .collect();
    let error = inner.library_move_error.lock().unwrap().clone();
    let share_name = place
        .as_ref()
        .and_then(|p| p.share)
        .and_then(|id| list.iter().find(|s| s.id == id))
        .map(|s| s.name.clone());
    Ok(json!({
        "configured": place.is_some(),
        "where": place.as_ref().map(|p| if p.share.is_some() { "share" } else { "local" }),
        "share": place.as_ref().and_then(|p| p.share),
        "shareName": share_name,
        "path": place.as_ref().map(|p| p.root.clone()),
        "free": place.as_ref().and_then(free_space),
        "copies": files,
        "size": bytes,
        "moving": left > 0,
        "moveLeft": left,
        "moveLeftBytes": left_bytes,
        "moveError": error,
        "types": types,
        "shares": shares,
        "watchable": watchable,
        "examples": {
            "movies": naming::example("movies"),
            "shows": naming::example("shows"),
            "music": naming::example("music"),
            "audiobooks": naming::example("audiobooks"),
        },
    }))
}

async fn apply(inner: &Arc<Inner>, args: &Value) -> CResult<Value> {
    let p = parse_place(args)?;
    let moving = inner.lib.read(|c| Ok(managed::move_left(c)?.0)).await?;
    if moving > 0 {
        return Err(CoreError::new(
            ErrorKind::Unsupported,
            "the library is still moving",
        ));
    }
    let result = check(inner, &p).await?;
    if result["ok"] != true {
        return Err(CoreError::new(
            ErrorKind::Unsupported,
            "this folder did not pass the checks",
        ));
    }
    let move_copies = args["mode"] != "empty";
    let scoped = Scoped::new(connection(inner, &p).await?, VPath::new(p.root.clone()));
    for (_, folder, _) in managed::TYPES {
        scoped
            .ensure_dir(&VPath::new(managed::join(&p.root, folder)))
            .await?;
    }
    let q = p.clone();
    let new = inner
        .lib
        .write(move |c| managed::install(c, &q, move_copies))
        .await?;
    let waiting = inner.lib.read(|c| Ok(managed::move_left(c)?.0)).await?;
    if waiting > 0 {
        enqueue_move(inner).await;
    } else {
        // Old folders without copies have nothing to wait for.
        finish(inner).await?;
        for id in new {
            enqueue_scan(inner, id, jobs::priority::USER).await;
        }
    }
    inner.mark_changed();
    status(inner).await
}

pub(crate) async fn enqueue_move(inner: &Arc<Inner>) {
    let _ = inner
        .lib
        .write(|c| {
            jobs::enqueue(
                c,
                "library_move",
                "{}",
                jobs::priority::NORMAL,
                Some("library_move"),
            )
        })
        .await;
    inner.wake.notify_waiters();
}

async fn finish(inner: &Arc<Inner>) -> CResult<()> {
    let gone = inner.lib.write(managed::finish_moves).await?;
    for id in gone {
        inner.forget_source(id);
    }
    Ok(())
}

/// Stream a file into a channel, chunk by chunk.
pub(crate) fn reader(src: Arc<dyn Source>, path: VPath, size: u64) -> huehuecoyotl_vfs::Chunks {
    let (tx, rx) = mpsc::channel::<huehuecoyotl_vfs::Result<Bytes>>(4);
    tokio::spawn(async move {
        let mut at = 0u64;
        while at < size {
            match src.read_at(&path, at, CHUNK).await {
                Ok(b) if b.is_empty() => {
                    let _ = tx
                        .send(Err(VfsError::Internal(format!("{path} ended early"))))
                        .await;
                    return;
                }
                Ok(b) => {
                    at += b.len() as u64;
                    if tx.send(Ok(b)).await.is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    return;
                }
            }
        }
    });
    rx
}

/// Byte for byte: the copy is what was read.
pub(crate) async fn same_content(
    a: &dyn Source,
    pa: &VPath,
    b: &dyn Source,
    pb: &VPath,
    size: u64,
) -> Result<bool, VfsError> {
    let mut at = 0u64;
    while at < size {
        let (x, y) = tokio::try_join!(a.read_at(pa, at, CHUNK), b.read_at(pb, at, CHUNK))?;
        if x.is_empty() || x != y {
            return Ok(false);
        }
        at += x.len() as u64;
    }
    Ok(true)
}

fn root_of(row: &huehuecoyotl_library::catalog::SourceRow) -> VPath {
    VPath::new(row.root.clone().unwrap_or_else(|| "/".into()))
}

/// Move one copy: written to the new place under a temporary name, compared, renamed; the row
/// follows; only then is the old file removed. Returns whether more wait.
async fn move_one(inner: &Arc<Inner>) -> CResult<bool> {
    let Some(m) = inner.lib.read(managed::next_move).await? else {
        finish(inner).await?;
        return Ok(false);
    };
    let (from_row, to_row) = {
        let (f, t) = (m.from, m.to);
        inner
            .lib
            .read(move |c| {
                Ok((
                    huehuecoyotl_library::catalog::source(c, f)?,
                    huehuecoyotl_library::catalog::source(c, t)?,
                ))
            })
            .await?
    };
    let (Some(from_row), Some(to_row)) = (from_row, to_row) else {
        return Err(CoreError::new(ErrorKind::NotFound, "library folder"));
    };
    let from = source_connection(inner, m.from).await?;
    let to = source_connection(inner, m.to).await?;
    let (from_root, to_root) = (root_of(&from_row), root_of(&to_row));
    let old = from_root.join(&m.rel);
    let new = to_root.join(&m.rel);
    let size = match from.stat(&old).await {
        Ok(e) => e.size,
        // Already gone from the old place: a scan of it would drop the row too.
        Err(VfsError::NotFound(_)) => {
            let id = m.file;
            inner
                .lib
                .write(move |c| {
                    c.execute("DELETE FROM file WHERE id = ?1", [id])?;
                    Ok(())
                })
                .await?;
            return Ok(true);
        }
        Err(e) => return Err(e.into()),
    };
    let scoped = Scoped::new(to.clone(), to_root.clone());
    if let Some(dir) = new.parent() {
        scoped.ensure_dir(&dir).await?;
    }
    // After an interruption the copy may already be there.
    let existing = to.stat(&new).await.ok();
    let complete = match &existing {
        Some(e) if e.size == size => same_content(&*from, &old, &*to, &new, size).await?,
        _ => false,
    };
    if !complete {
        let expected = existing.map(|e| Expected {
            size: e.size,
            mtime: e.mtime,
        });
        let written = scoped
            .write(&new, reader(from.clone(), old.clone(), size), expected)
            .await?;
        if written != size || !same_content(&*from, &old, &*to, &new, size).await? {
            return Err(CoreError::new(
                ErrorKind::Internal,
                format!("the copy of {} differs from the original", m.rel),
            ));
        }
    }
    let row = m.clone();
    inner
        .lib
        .write(move |c| managed::file_moved(c, &row))
        .await?;
    if let Err(e) = from.remove(&old).await {
        tracing::warn!("the old copy of a moved file stays: {e}");
    } else {
        // Folders the move left empty.
        let mut dir = old.parent();
        while let Some(d) = dir {
            if d == from_root || from.remove_dir(&d).await.is_err() {
                break;
            }
            dir = d.parent();
        }
    }
    inner.mark_changed();
    Ok(true)
}

/// The persisted job: one file per run, so scans and shutdown never wait for a whole library. A
/// problem pauses it (the job comes back later); nothing is removed without a checked copy.
/// Returns the seconds until the next run, `None` when it is finished.
pub(crate) async fn run_move(inner: &Arc<Inner>) -> Option<i64> {
    match move_one(inner).await {
        Ok(true) => {
            inner.library_move_error.lock().unwrap().take();
            Some(0)
        }
        Ok(false) => {
            inner.library_move_error.lock().unwrap().take();
            let ids = inner.lib.read(managed::sources).await.unwrap_or_default();
            for id in ids {
                enqueue_scan(inner, id, jobs::priority::USER).await;
            }
            inner.mark_changed();
            None
        }
        Err(e) => {
            tracing::warn!("moving the library paused: {}", e.message);
            *inner.library_move_error.lock().unwrap() = Some(e.message);
            inner.mark_changed();
            Some(RETRY_SECONDS)
        }
    }
}

impl Core {
    /// Issue #150: Settings → Library. `action`: status, check, apply, setType, removePreview,
    /// gone or itemOrigin. Answers with MusicDetail of kind `library…`.
    pub fn library_call(&self, request_id: u64, action: &str, args_json: &str) {
        let action = action.to_string();
        let args: Value = serde_json::from_str(args_json).unwrap_or_default();
        self.spawn(request_id, move |inner| async move {
            let (kind, v) = match action.as_str() {
                "status" => ("library", status(&inner).await?),
                "check" => ("libraryCheck", check(&inner, &parse_place(&args)?).await?),
                "apply" => ("library", apply(&inner, &args).await?),
                "setType" => {
                    let key = args["type"].as_str().unwrap_or("").to_string();
                    let changes = args["changes"].clone();
                    inner
                        .lib
                        .write(move |c| managed::set_type(c, &key, &changes))
                        .await?;
                    ("library", status(&inner).await?)
                }
                "createFolder" => {
                    let (Some(share), Some(dir), Some(name)) = (
                        args["share"].as_i64(),
                        args["path"].as_str(),
                        args["name"].as_str(),
                    ) else {
                        return Err(CoreError::new(ErrorKind::Unsupported, "no folder name"));
                    };
                    let name = name.trim();
                    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".."
                    {
                        return Err(CoreError::new(ErrorKind::Unsupported, "not a folder name"));
                    }
                    let path = VPath::new(if dir == "/" { "" } else { dir }).join(name);
                    let path = if path.as_str().starts_with('/') {
                        path
                    } else {
                        VPath::new(format!("/{path}"))
                    };
                    let src = crate::shares::share_connection(&inner, share).await?;
                    src.create_dir(&path).await?;
                    ("libraryFolder", json!({ "path": path.to_string() }))
                }
                "removePreview" => {
                    let s = args["source"].as_i64().unwrap_or(0);
                    let (stay, leave) = inner
                        .lib
                        .read(move |c| managed::remove_preview(c, s))
                        .await?;
                    (
                        "libraryRemovePreview",
                        json!({ "source": s, "stay": stay, "leave": leave }),
                    )
                }
                "gone" => {
                    let s = args["source"].as_i64().unwrap_or(0);
                    let (counts, items) = inner
                        .lib
                        .read(move |c| Ok((managed::gone_counts(c, s)?, managed::gone_list(c, s)?)))
                        .await?;
                    (
                        "libraryGone",
                        json!({ "source": s, "total": counts.0, "kept": counts.1,
                                "left": counts.2, "items": items }),
                    )
                }
                "itemOrigin" => {
                    let id = args["item"].as_i64().unwrap_or(0);
                    let mut v = inner.lib.read(move |c| managed::item_origin(c, id)).await?;
                    v["item"] = json!(id);
                    ("libraryOrigin", v)
                }
                other => match crate::imports::action(&inner, other, &args).await? {
                    Some(answer) => answer,
                    None => match crate::tag_edit::action(&inner, other, &args).await? {
                        Some(answer) => answer,
                        None => {
                            return Err(CoreError::new(
                                ErrorKind::Unsupported,
                                format!("unknown library action {other}"),
                            ));
                        }
                    },
                },
            };
            Ok(Event::MusicDetail {
                request_id,
                kind: kind.into(),
                json: v.to_string(),
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_stay_inside_the_folder() {
        let root = VPath::new("/lib");
        assert!(inside(&root, &VPath::new("/lib/Music/a.flac")).is_ok());
        assert!(inside(&root, &VPath::new("/lib")).is_ok());
        for p in ["/libx/a", "/other/a", "/lib/../etc/a", "/lib/./a", "/"] {
            assert!(
                matches!(inside(&root, &VPath::new(p)), Err(VfsError::ReadOnly(_))),
                "{p}"
            );
        }
    }
}
