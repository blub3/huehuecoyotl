//! What a chosen match changes before it is applied (2.7, issues #97 to #100): the item another
//! item with that TMDB id is merged into, the episodes a show's new match does not have, and, for
//! a movie that is really a show or the reverse, which files move and as what.

use std::sync::Arc;

use huehuecoyotl_library::kinds::{self, TreeFile};
use huehuecoyotl_library::{catalog, jobs, matching as m};
use huehuecoyotl_metadata::MediaType;
use huehuecoyotl_scanner::parse::{Kind, parse};
use serde_json::{Value, json};

use crate::{CResult, CoreError, ErrorKind, Event, Inner};

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn folder(path: &str) -> String {
    path.rsplit_once('/').map_or("", |(d, _)| d).to_string()
}

/// Season and episode from a file name: what the scanner reads, else "1x02", "E02", "Episode 2",
/// "Folge 2" or "Part 2" (season 1).
pub(crate) fn episode_numbers(path: &str) -> Option<(i64, i64)> {
    let p = parse(path);
    if p.kind == Kind::Episode
        && let Some(e) = p.episodes.first()
    {
        return Some((i64::from(p.season.unwrap_or(1)), i64::from(*e)));
    }
    let name = file_name(path);
    let re = regex::Regex::new(
        r"(?i)(?:\bs(\d{1,2})\s*e(\d{1,3}))|(?:\b(\d{1,2})x(\d{2,3})\b)|(?:\b(?:episode|ep|e|folge|part|teil)[ ._-]?(\d{1,3})\b)",
    )
    .ok()?;
    let c = re.captures(name)?;
    let n = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<i64>().ok());
    if let (Some(s), Some(e)) = (n(1), n(2)) {
        return Some((s, e));
    }
    if let (Some(s), Some(e)) = (n(3), n(4)) {
        return Some((s, e));
    }
    n(5).map(|e| (1, e))
}

fn tmdb_id(choice: &Value) -> CResult<i64> {
    choice["id"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| choice["id"].as_i64())
        .ok_or_else(|| CoreError::new(ErrorKind::NotFound, "no match chosen".to_string()))
}

fn file_json(f: &TreeFile, fixed: bool, on: bool) -> Value {
    let numbers = episode_numbers(&f.path);
    json!({
        "fileId": f.file_id,
        "name": file_name(&f.path),
        "path": f.path,
        "from": f.owner,
        "fixed": fixed,
        "on": on,
        "season": numbers.map(|n| n.0).or(f.season),
        "episode": numbers.map(|n| n.1).or(f.episode),
        "needsNumbers": numbers.is_none() && f.episode.is_none(),
    })
}

/// Issue #97 to #100: what choosing `choice` for `item` would do, as a `MusicDetail` of kind
/// "matchPreview": `change` (none, toShow, toMovie), `merge` (the title of an item with that id
/// that the item joins), `missing` (episodes on disk the show's new match does not have) and
/// `files` (for a kind change: each file, whether it is fixed or ticked, and as what it moves).
pub(crate) async fn preview(inner: &Arc<Inner>, item: i64, choice: &Value) -> CResult<Value> {
    let (kind, _) = inner
        .lib
        .read(move |c| m::kind_title(c, item))
        .await?
        .ok_or_else(|| CoreError::new(ErrorKind::NotFound, format!("item {item}")))?;
    let tmdb = tmdb_id(choice)?;
    let to = choice["kind"].as_str().unwrap_or(&kind).to_string();
    let current_kind = if kind == "movie" { "movie" } else { "show" };
    let other = {
        let to = to.clone();
        inner
            .lib
            .read(move |c| kinds::by_tmdb(c, &to, tmdb))
            .await?
            .filter(|(id, _)| *id != item)
            .map(|(_, t)| t)
    };
    let files = inner.lib.read(move |c| kinds::files_of(c, item)).await?;
    if to == current_kind {
        let mut missing = Vec::new();
        if to == "show" {
            let p = inner.provider().ok_or_else(|| {
                CoreError::new(ErrorKind::Auth, "No TMDB API key is set.".to_string())
            })?;
            let d = p
                .details(MediaType::Show, tmdb)
                .await
                .map_err(crate::books::meta_err)?;
            let show = if kind == "show" {
                item
            } else {
                inner
                    .lib
                    .read(move |c| {
                        Ok(c.query_row(
                            "SELECT se.parent_id FROM item e JOIN item se ON se.id = e.parent_id WHERE e.id = ?1",
                            [item],
                            |r| r.get::<_, i64>(0),
                        )?)
                    })
                    .await?
            };
            let on_disk = inner
                .lib
                .read(move |c| catalog::show_structure(c, show, true))
                .await?;
            for (_, season, eps) in on_disk {
                let known = d
                    .seasons
                    .iter()
                    .find(|s| i64::from(s.season_no) == season)
                    .map_or(0, |s| i64::from(s.episode_count));
                for (_, e) in eps {
                    if e > known {
                        missing.push(format!("S{season:02}E{e:02}"));
                    }
                }
            }
        }
        return Ok(json!({
            "itemId": item,
            "change": "none",
            "merge": other,
            "missing": missing,
            "files": files.iter().map(|f| file_json(f, true, true)).collect::<Vec<_>>(),
        }));
    }
    if to == "show" {
        // This movie's files, and other movies in the same folders: likely more episodes.
        let folders: Vec<String> = files.iter().map(|f| folder(&f.path)).collect();
        let more = inner
            .lib
            .read(move |c| kinds::movies_in_folders(c, &folders, item))
            .await?;
        let mut list: Vec<Value> = files.iter().map(|f| file_json(f, true, true)).collect();
        list.extend(more.iter().map(|f| {
            let on = episode_numbers(&f.path).is_some();
            file_json(f, false, on)
        }));
        return Ok(json!({ "itemId": item, "change": "toShow", "merge": other, "files": list }));
    }
    // To a movie: from an episode, its files (fixed) and the show's other files in the same
    // folder (versions, ticked); from a show, all its files, ticked, for the user to untick.
    let list: Vec<Value> = if kind == "episode" {
        let own = files.clone();
        let show = inner
            .lib
            .read(move |c| {
                Ok(c.query_row(
                    "SELECT se.parent_id FROM item e JOIN item se ON se.id = e.parent_id WHERE e.id = ?1",
                    [item],
                    |r| r.get::<_, i64>(0),
                )?)
            })
            .await?;
        let all = inner.lib.read(move |c| kinds::files_of(c, show)).await?;
        let folders: Vec<String> = own.iter().map(|f| folder(&f.path)).collect();
        own.iter()
            .map(|f| file_json(f, true, true))
            .chain(
                all.iter()
                    .filter(|f| f.item_id != item && folders.contains(&folder(&f.path)))
                    .map(|f| file_json(f, false, true)),
            )
            .collect()
    } else {
        files.iter().map(|f| file_json(f, false, true)).collect()
    };
    Ok(json!({ "itemId": item, "change": "toMovie", "merge": other, "files": list }))
}

/// Issues #99 and #100: make a movie (and the ticked files of other movies) a show, or episodes
/// of a show a movie. `choice`: the candidate (`id`, `kind`), with `files`: for a show
/// `[{fileId, season, episode}]`, for a movie `[{fileId}]`. Returns the new show or movie.
pub(crate) async fn change_kind(inner: &Arc<Inner>, item: i64, choice: &Value) -> CResult<i64> {
    let tmdb = tmdb_id(choice)?;
    let to = choice["kind"].as_str().unwrap_or_default().to_string();
    let p = inner
        .provider()
        .ok_or_else(|| CoreError::new(ErrorKind::Auth, "No TMDB API key is set.".to_string()))?;
    let ty = if to == "show" {
        MediaType::Show
    } else {
        MediaType::Movie
    };
    let d = p.details(ty, tmdb).await.map_err(crate::books::meta_err)?;
    let year = d.year.map(i64::from);
    let files: Vec<&Value> = choice["files"]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    let own = inner.lib.read(move |c| kinds::files_of(c, item)).await?;
    if files.is_empty() && own.is_empty() {
        return Err(CoreError::new(
            ErrorKind::NotFound,
            "no files to move".to_string(),
        ));
    }
    let new = if to == "show" {
        let mut moves = Vec::new();
        // The item's own files always move.
        for f in &own {
            let given = files
                .iter()
                .find(|v| v["fileId"].as_i64() == Some(f.file_id));
            let numbers = given
                .and_then(|v| Some((v["season"].as_i64()?, v["episode"].as_i64()?)))
                .or_else(|| episode_numbers(&f.path));
            let Some((s, e)) = numbers else {
                return Err(CoreError::new(
                    ErrorKind::Unsupported,
                    format!(
                        "{} needs a season and an episode number",
                        file_name(&f.path)
                    ),
                ));
            };
            moves.push((f.file_id, s, e));
        }
        for v in &files {
            let Some(id) = v["fileId"].as_i64() else {
                continue;
            };
            if moves.iter().any(|(f, _, _)| *f == id) {
                continue;
            }
            if let (Some(s), Some(e)) = (v["season"].as_i64(), v["episode"].as_i64()) {
                moves.push((id, s, e));
            }
        }
        let first = own.first().map(|f| f.path.clone()).unwrap_or_default();
        let parsed = parse(&first);
        let key =
            huehuecoyotl_scanner::scan::show_key(parsed.show_title.as_deref().unwrap_or(&d.title));
        let title = d.title.clone();
        inner
            .lib
            .write(move |c| kinds::movies_to_show(c, &moves, tmdb, &title, year, &key))
            .await?
    } else {
        let mut ids: Vec<i64> = files.iter().filter_map(|v| v["fileId"].as_i64()).collect();
        let kind = inner
            .lib
            .read(move |c| m::kind_title(c, item))
            .await?
            .map(|(k, _)| k)
            .unwrap_or_default();
        if kind == "episode" {
            for f in &own {
                if !ids.contains(&f.file_id) {
                    ids.push(f.file_id);
                }
            }
        }
        if ids.is_empty() {
            return Err(CoreError::new(
                ErrorKind::NotFound,
                "no files chosen".to_string(),
            ));
        }
        let title = d.title.clone();
        let key = format!(
            "{}|{}",
            huehuecoyotl_library::text::normalize(&d.title),
            year.map(|y| y.to_string()).unwrap_or_default()
        );
        inner
            .lib
            .write(move |c| kinds::episodes_to_movie(c, &ids, tmdb, &title, year, &key))
            .await?
    };
    crate::worker::enqueue_details(inner, new, tmdb, jobs::priority::USER).await;
    inner.sink.post(Event::ItemChanged { item_id: new });
    inner.mark_changed();
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_from_names() {
        assert_eq!(
            episode_numbers("Show/The.Long.Harbour.S01E02.1080p.mkv"),
            Some((1, 2))
        );
        assert_eq!(episode_numbers("x/Long Harbour 2x05.mkv"), Some((2, 5)));
        assert_eq!(
            episode_numbers("x/Long Harbour - Folge 7.mkv"),
            Some((1, 7))
        );
        assert_eq!(episode_numbers("x/Long Harbour Part 3.mkv"), Some((1, 3)));
        assert_eq!(
            episode_numbers("Night Shift (2019)/Night.Shift.2019.mkv"),
            None
        );
    }
}
