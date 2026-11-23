//! Edit tags… (3.0, issue #157, SPEC 7). A preview shows every file, field, before and after.
//! Writing goes file by file through a local copy and a safe replace, with the time and size the
//! file had when it was read; one failure never stops the others. Files are never renamed.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use huehuecoyotl_library::copy_meta::{self, CopyTags};
use huehuecoyotl_library::tag_edit::{self, Changes, EditFile};
use huehuecoyotl_library::{catalog, copies};
use huehuecoyotl_scanner::{copy_tags, tags};
use huehuecoyotl_vfs::{Expected, LocalSource, Source, VPath, VfsError, safe_write};
use serde_json::{Value, json};

use crate::managed::reader;
use crate::{CoreError, ErrorKind, Inner};

type CResult<T> = Result<T, CoreError>;

const CHUNK: u32 = 1 << 20;
const GRANTS: &str = "tag_write_grants";

fn list(v: &str) -> Vec<String> {
    v.split([';', ','])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_changes(args: &Value, scope: &str) -> CResult<Changes> {
    let f = &args["changes"];
    let allowed = tag_edit::allowed(scope);
    let get = |k: &str| {
        allowed
            .contains(&k)
            .then(|| f[k].as_str().map(|s| s.trim().to_string()))
            .flatten()
    };
    let disc = match get("disc") {
        Some(d) if !d.is_empty() => Some(
            d.parse::<u32>()
                .map_err(|_| CoreError::new(ErrorKind::Unsupported, "the disc must be a number"))?,
        ),
        _ => None,
    };
    let names = |k: &str| get(k).map(|v| list(&v)).filter(|l| !l.is_empty());
    Ok(Changes {
        title: get("title").filter(|t| !t.is_empty()),
        artist: names("artist"),
        album_artist: names("albumArtist"),
        album: get("album").filter(|t| !t.is_empty()),
        year: get("year").filter(|t| !t.is_empty()),
        genre: get("genre").filter(|t| !t.is_empty()),
        disc,
    })
}

/// (source kind, root, name) per source, and where the user granted write access.
async fn roots(
    inner: &Arc<Inner>,
    files: &[EditFile],
) -> CResult<HashMap<i64, (String, VPath, String)>> {
    let ids: HashSet<i64> = files.iter().map(|f| f.source).collect();
    let mut out = HashMap::new();
    for id in ids {
        let row = inner
            .lib
            .read(move |c| {
                let row = catalog::source(c, id)?;
                let name: Option<String> = c
                    .query_row("SELECT name FROM source WHERE id = ?1", [id], |r| r.get(0))
                    .ok();
                Ok((row, name.unwrap_or_default()))
            })
            .await?;
        if let (Some(r), name) = row {
            out.insert(
                id,
                (
                    r.kind,
                    VPath::new(r.root.unwrap_or_else(|| "/".into())),
                    name,
                ),
            );
        }
    }
    Ok(out)
}

async fn grants(inner: &Arc<Inner>) -> CResult<Value> {
    let s = inner.lib.read(|c| copies::setting(c, GRANTS)).await?;
    Ok(s.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({})))
}

/// The file through the connection that may write it (a folder the portal granted, if any).
async fn place(
    inner: &Arc<Inner>,
    f: &EditFile,
    root: &(String, VPath, String),
    granted: &Value,
) -> CResult<(Arc<dyn Source>, VPath)> {
    if root.0 == "local" {
        let base = granted[f.source.to_string()]
            .as_str()
            .map(VPath::new)
            .unwrap_or_else(|| root.1.clone());
        return Ok((Arc::new(LocalSource), base.join(&f.path)));
    }
    Ok((inner.remote_source(f.source).await?, root.1.join(&f.path)))
}

fn before_of(t: &tags::TrackTags) -> serde_json::Map<String, Value> {
    let names = |v: &[tags::Credit]| {
        v.iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>()
            .join("; ")
    };
    let mut m = serde_json::Map::new();
    m.insert("title".into(), json!(t.title.clone().unwrap_or_default()));
    m.insert("artist".into(), json!(names(&t.artists)));
    m.insert("albumArtist".into(), json!(names(&t.album_artists)));
    m.insert("album".into(), json!(t.album.clone().unwrap_or_default()));
    m.insert("year".into(), json!(t.date.clone().unwrap_or_default()));
    m.insert("genre".into(), json!(t.genre.clone().unwrap_or_default()));
    m.insert(
        "disc".into(),
        json!(t.disc_no.map(|d| d.to_string()).unwrap_or_default()),
    );
    m
}

const FIELDS: [&str; 7] = [
    "albumArtist",
    "artist",
    "album",
    "year",
    "genre",
    "disc",
    "title",
];

/// Tags of the scope's files, read from the files; a file that cannot be read is "offline".
async fn read_all(
    inner: &Arc<Inner>,
    files: &[EditFile],
) -> CResult<Vec<(EditFile, Option<serde_json::Map<String, Value>>)>> {
    let rs = roots(inner, files).await?;
    let granted = grants(inner).await?;
    let mut out = Vec::with_capacity(files.len());
    for f in files {
        let m = match rs.get(&f.source) {
            Some(r) => match place(inner, f, r, &granted).await {
                Ok((src, p)) => tags::read(&*src, &p, f.size as u64)
                    .await
                    .ok()
                    .map(|x| before_of(&x.tags)),
                Err(_) => None,
            },
            None => None,
        };
        out.push((f.clone(), m));
    }
    Ok(out)
}

async fn plan(inner: &Arc<Inner>, item: i64) -> CResult<Value> {
    let s = inner.lib.read(move |c| tag_edit::scopes(c, item)).await?;
    Ok(json!({ "item": item, "scopes": s }))
}

async fn scope_values(inner: &Arc<Inner>, args: &Value) -> CResult<Value> {
    let scope = args["scope"].as_str().unwrap_or("album").to_string();
    let id = args["id"].as_i64().unwrap_or(0);
    let sc = scope.clone();
    let files = inner.lib.read(move |c| tag_edit::files(c, &sc, id)).await?;
    let rows = read_all(inner, &files).await?;
    let mut fields = serde_json::Map::new();
    for k in FIELDS {
        let vals: HashSet<String> = rows
            .iter()
            .filter_map(|(_, m)| m.as_ref())
            .map(|m| m[k].as_str().unwrap_or("").to_string())
            .collect();
        let mut sample: Vec<String> = vals.iter().filter(|v| !v.is_empty()).cloned().collect();
        sample.sort();
        fields.insert(
            k.to_string(),
            json!({
                "value": if vals.len() == 1 { vals.iter().next().cloned().unwrap_or_default() } else { String::new() },
                "several": vals.len() > 1,
                "sample": sample.into_iter().take(3).collect::<Vec<_>>(),
                "allowed": tag_edit::allowed(&scope).contains(&k),
            }),
        );
    }
    let first = files.first().map(|f| f.track);
    let canonical = match first {
        Some(t) => {
            let c = inner.lib.read(move |c| copy_meta::track_tags(c, t)).await?;
            // Only a matched release has values to fill in.
            c.filter(|t| !t.mb_release.is_empty()).map(|t| {
                json!({
                    "albumArtist": t.album_artists.join("; "), "artist": t.artists.join("; "),
                    "album": t.album, "year": t.date, "genre": t.genre,
                    "disc": t.disc.map(|d| d.to_string()).unwrap_or_default(), "title": t.title,
                })
            })
        }
        None => None,
    };
    let unread = rows.iter().filter(|(_, m)| m.is_none()).count();
    Ok(
        json!({ "scope": scope, "id": id, "files": files.len(), "unreadable": unread,
               "fields": fields, "canonical": canonical }),
    )
}

fn after(ch: &Changes, k: &str) -> Option<String> {
    match k {
        "title" => ch.title.clone(),
        "artist" => ch.artist.as_ref().map(|v| v.join("; ")),
        "albumArtist" => ch.album_artist.as_ref().map(|v| v.join("; ")),
        "album" => ch.album.clone(),
        "year" => ch.year.clone(),
        "genre" => ch.genre.clone(),
        "disc" => ch.disc.map(|d| d.to_string()),
        _ => None,
    }
}

async fn preview(inner: &Arc<Inner>, args: &Value) -> CResult<Value> {
    let scope = args["scope"].as_str().unwrap_or("album").to_string();
    let id = args["id"].as_i64().unwrap_or(0);
    let ch = parse_changes(args, &scope)?;
    let sc = scope.clone();
    let files = inner.lib.read(move |c| tag_edit::files(c, &sc, id)).await?;
    let rs = roots(inner, &files).await?;
    let rows = read_all(inner, &files).await?;
    let known_ro = inner.read_only_sources.lock().unwrap().clone();
    let mut changes = Vec::new();
    let (mut will, mut same, mut ro, mut off) = (0, 0, 0, 0);
    let mut ro_names = HashSet::new();
    let mut off_names = HashSet::new();
    for (f, before) in &rows {
        let name = rs.get(&f.source).map(|r| r.2.clone()).unwrap_or_default();
        let Some(b) = before else {
            off += 1;
            off_names.insert(name);
            continue;
        };
        let diffs: Vec<(&str, String, String)> = FIELDS
            .iter()
            .filter_map(|k| {
                let a = after(&ch, k)?;
                let was = b[*k].as_str().unwrap_or("").to_string();
                (a != was).then_some((*k, was, a))
            })
            .collect();
        if known_ro.contains(&f.source) {
            ro += 1;
            ro_names.insert(name);
            continue;
        }
        if diffs.is_empty() && args["writeIds"] != true {
            same += 1;
            continue;
        }
        will += 1;
        for (k, was, now) in diffs {
            if changes.len() < 200 {
                changes.push(json!({ "file": f.path, "field": k, "before": was, "after": now }));
            }
        }
    }
    Ok(json!({
        "scope": scope, "id": id, "files": rows.len(), "willChange": will, "same": same,
        "readOnly": ro, "readOnlyNames": ro_names.into_iter().collect::<Vec<_>>(),
        "offline": off, "offlineNames": off_names.into_iter().collect::<Vec<_>>(),
        "rows": changes,
    }))
}

fn copy_tags_for(ch: &Changes, ids: Option<CopyTags>) -> CopyTags {
    let mut t = CopyTags::default();
    if let Some(i) = ids {
        t.mb_artists = i.mb_artists;
        t.mb_album_artists = i.mb_album_artists;
        t.mb_release_group = i.mb_release_group;
        t.mb_release = i.mb_release;
        t.mb_recording = i.mb_recording;
        t.mb_track = i.mb_track;
        t.isrc = i.isrc;
        t.barcode = i.barcode;
    }
    t.title = ch.title.clone().unwrap_or_default();
    t.artists = ch.artist.clone().unwrap_or_default();
    t.album_artists = ch.album_artist.clone().unwrap_or_default();
    t.album = ch.album.clone().unwrap_or_default();
    t.date = ch.year.clone().unwrap_or_default();
    t.genre = ch.genre.clone().unwrap_or_default();
    t.disc = ch.disc;
    t
}

enum Outcome {
    Written(i64, i64),
    ReadOnly,
    Offline,
    Failed(String),
}

/// One file: read to a local copy, tag it, and replace the original only if it is still what
/// was read.
async fn write_one(
    inner: &Arc<Inner>,
    f: &EditFile,
    root: &(String, VPath, String),
    granted: &Value,
    t: CopyTags,
    temp: &std::path::Path,
) -> Outcome {
    let fail = |e: VfsError| match e {
        VfsError::ReadOnly(_) | VfsError::Auth(_) => Outcome::ReadOnly,
        VfsError::Unreachable(_) => Outcome::Offline,
        VfsError::Changed(_) => Outcome::Failed("changed".into()),
        VfsError::NotFound(_) => Outcome::Failed("gone".into()),
        e => Outcome::Failed(e.to_string()),
    };
    let (src, path) = match place(inner, f, root, granted).await {
        Ok(p) => p,
        Err(e) if e.kind == ErrorKind::Unreachable => return Outcome::Offline,
        Err(e) => return Outcome::Failed(e.message),
    };
    let st = match src.stat(&path).await {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let expected = Expected {
        size: st.size,
        mtime: st.mtime,
    };
    // A file changed since the library last saw it is not the one previewed.
    if st.size != f.size as u64 || (f.mtime != 0 && st.mtime != f.mtime) {
        return Outcome::Failed("changed".into());
    }
    let _ = tokio::fs::remove_file(temp).await;
    let body = async {
        let mut out = tokio::fs::File::create(temp).await?;
        let mut at = 0;
        while at < st.size {
            let b = src.read_at(&path, at, CHUNK).await?;
            if b.is_empty() {
                return Err(VfsError::Internal("the file ended early".into()));
            }
            tokio::io::AsyncWriteExt::write_all(&mut out, &b).await?;
            at += b.len() as u64;
        }
        tokio::io::AsyncWriteExt::flush(&mut out).await?;
        Ok::<(), VfsError>(())
    }
    .await;
    if let Err(e) = body {
        return fail(e);
    }
    let p = temp.to_path_buf();
    let tagged = tokio::task::spawn_blocking(move || copy_tags::write_tags(&p, &t, None)).await;
    match tagged {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Outcome::Failed(e.to_string()),
        Err(e) => return Outcome::Failed(e.to_string()),
    }
    let new_size = match tokio::fs::metadata(temp).await {
        Ok(m) => m.len(),
        Err(e) => return fail(e.into()),
    };
    let chunks = reader(
        Arc::new(LocalSource),
        VPath::new(temp.to_string_lossy()),
        new_size,
    );
    if let Err(e) = safe_write(&*src, &path, chunks, Some(expected)).await {
        return fail(e);
    }
    match src.stat(&path).await {
        Ok(s) => Outcome::Written(s.size as i64, s.mtime),
        Err(e) => fail(e),
    }
}

async fn write(inner: &Arc<Inner>, args: &Value) -> CResult<Value> {
    let scope = args["scope"].as_str().unwrap_or("album").to_string();
    let id = args["id"].as_i64().unwrap_or(0);
    let ch = parse_changes(args, &scope)?;
    let write_ids = args["writeIds"] == true;
    let only: Option<HashSet<i64>> = args["only"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_i64).collect());
    let sc = scope.clone();
    let mut files = inner.lib.read(move |c| tag_edit::files(c, &sc, id)).await?;
    if let Some(o) = &only {
        files.retain(|f| o.contains(&f.file));
    }
    let rs = roots(inner, &files).await?;
    let granted = grants(inner).await?;
    let dir = inner.data_dir.join("tag-edit");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(VfsError::from)?;
    let (mut written, mut failed, mut read_only, mut offline) = (vec![], vec![], vec![], vec![]);
    let mut same = 0;
    for f in &files {
        let Some(root) = rs.get(&f.source) else {
            offline.push(json!({ "file": f.path, "source": "" }));
            continue;
        };
        // Nothing to do for a file that already has these values.
        let current = match place(inner, f, root, &granted).await {
            Ok((src, p)) => tags::read(&*src, &p, f.size as u64)
                .await
                .ok()
                .map(|x| before_of(&x.tags)),
            Err(_) => None,
        };
        if !write_ids
            && let Some(b) = &current
            && FIELDS
                .iter()
                .all(|k| after(&ch, k).is_none_or(|a| b[*k].as_str() == Some(a.as_str())))
        {
            same += 1;
            continue;
        }
        let track = f.track;
        let ids = if write_ids {
            inner
                .lib
                .read(move |c| copy_meta::track_tags(c, track))
                .await?
        } else {
            None
        };
        let t = copy_tags_for(&ch, ids);
        let ext = f.path.rsplit('.').next().unwrap_or("bin");
        let temp = dir.join(format!("{}.{ext}", f.file));
        let result = write_one(inner, f, root, &granted, t, &temp).await;
        let _ = tokio::fs::remove_file(&temp).await;
        match result {
            Outcome::Written(size, mtime) => written.push((f.clone(), size, mtime)),
            Outcome::ReadOnly => {
                inner.read_only_sources.lock().unwrap().insert(f.source);
                read_only.push(
                    json!({ "file": f.path, "source": root.2, "sourceId": f.source,
                                       "local": root.0 == "local" }),
                );
            }
            Outcome::Offline => offline.push(json!({ "file": f.path, "source": root.2 })),
            Outcome::Failed(reason) => {
                failed.push(json!({ "fileId": f.file, "file": f.path, "reason": reason }));
            }
        }
    }
    let done = written.len();
    if done > 0 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        let c2 = ch.clone();
        inner
            .lib
            .write(move |c| tag_edit::apply(c, &written, &c2, now))
            .await?;
        inner.mark_changed();
    }
    Ok(json!({
        "scope": scope, "id": id, "updated": done, "same": same,
        "attempted": done + failed.len(), "failed": failed, "readOnly": read_only, "offline": offline,
        "flatpak": std::path::Path::new("/.flatpak-info").exists(),
    }))
}

pub(crate) async fn action(
    inner: &Arc<Inner>,
    name: &str,
    args: &Value,
) -> CResult<Option<(&'static str, Value)>> {
    Ok(Some(match name {
        "tagPlan" => (
            "tagPlan",
            plan(inner, args["item"].as_i64().unwrap_or(0)).await?,
        ),
        "tagValues" => ("tagValues", scope_values(inner, args).await?),
        "tagPreview" => ("tagPreview", preview(inner, args).await?),
        "tagWrite" => ("tagResult", write(inner, args).await?),
        // Under Flatpak: the folder the portal gave write access to, for one local source.
        "tagGrant" => {
            let (source, path) = (
                args["source"].as_i64().unwrap_or(0),
                args["path"].as_str().unwrap_or("").to_string(),
            );
            let mut g = grants(inner).await?;
            g[source.to_string()] = json!(path);
            let text = g.to_string();
            inner
                .lib
                .write(move |c| copies::set_setting(c, GRANTS, Some(&text)))
                .await?;
            inner.read_only_sources.lock().unwrap().remove(&source);
            ("tagGranted", json!({ "source": source }))
        }
        _ => return Ok(None),
    }))
}
