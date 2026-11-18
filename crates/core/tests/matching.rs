//! Issue #92: matching again through the public core API with a fake TMDB provider: library
//! rematches touch only the items they should (unmatched and uncertain ones, or all; the user's
//! choices only when included), never drop a match they cannot replace, count their results,
//! resume after a restart, and refuse to run without a key or with online information off.
//! Pasted TMDB links and IMDb ids find their record directly.
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event};
use huehuecoyotl_library::rusqlite::Connection;
use huehuecoyotl_metadata::{
    Candidate, Details, MediaType, MetadataProvider, Result, SeasonDetails,
};
use serde_json::Value;

/// A fake TMDB. After `remade` is set, "The Thing" finds only the 2011 film.
#[derive(Default)]
struct Fake {
    remade: AtomicBool,
    /// "Alien" finds another record, surely.
    moved: AtomicBool,
}

fn cand(id: i64, title: &str, year: u16) -> Candidate {
    Candidate {
        id,
        title: title.into(),
        original_title: title.into(),
        year: Some(year),
        poster_path: Some(format!("/p{id}.png")),
        popularity: 10.0,
        ..Default::default()
    }
}

#[async_trait]
impl MetadataProvider for Fake {
    async fn validate(&self) -> Result<()> {
        Ok(())
    }
    async fn search(&self, ty: MediaType, title: &str, _y: Option<u16>) -> Result<Vec<Candidate>> {
        Ok(match (ty, title.to_lowercase().as_str()) {
            (MediaType::Movie, "alien") if self.moved.load(Ordering::SeqCst) => {
                vec![cand(999, "Alien", 1979)]
            }
            (MediaType::Movie, "alien") => {
                vec![cand(348, "Alien", 1979), cand(679, "Aliens", 1986)]
            }
            (MediaType::Movie, "the thing") if self.remade.load(Ordering::SeqCst) => {
                vec![cand(60935, "The Thing", 2011)]
            }
            (MediaType::Movie, "the thing") => {
                vec![
                    cand(1091, "The Thing", 1982),
                    cand(60935, "The Thing", 2011),
                ]
            }
            _ => vec![],
        })
    }
    async fn details(&self, _ty: MediaType, id: i64) -> Result<Details> {
        let (title, year) = match id {
            348 | 999 => ("Alien", 1979),
            1091 => ("The Thing", 1982),
            60935 => ("The Thing", 2011),
            _ => ("Other", 2000),
        };
        Ok(Details {
            id,
            title: title.into(),
            original_title: title.into(),
            year: Some(year),
            poster_path: Some(format!("/p{id}.png")),
            ..Default::default()
        })
    }
    async fn season(&self, _show: i64, season_no: u16) -> Result<SeasonDetails> {
        Ok(SeasonDetails {
            season_no,
            ..Default::default()
        })
    }
    async fn image(&self, _path: &str, _size: &str) -> Result<Bytes> {
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::new(50, 75))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        Ok(Bytes::from(png))
    }
    async fn find_imdb(&self, imdb: &str) -> Result<Vec<(MediaType, Candidate)>> {
        Ok(if imdb == "tt0078748" {
            vec![(MediaType::Movie, cand(348, "Alien", 1979))]
        } else {
            vec![]
        })
    }
}

fn answer(rx: &Receiver<Event>, id: u64) -> Event {
    let deadline = Instant::now() + Duration::from_secs(20);
    while let Ok(e) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        match &e {
            Event::MusicDetail { request_id, .. } | Event::Error { request_id, .. }
                if *request_id == id =>
            {
                return e;
            }
            _ => {}
        }
    }
    panic!("no answer to request {id}");
}

fn json(e: &Event) -> Value {
    match e {
        Event::MusicDetail { json, .. } => serde_json::from_str(json).unwrap(),
        other => panic!("not a detail: {other:?}"),
    }
}

/// (title, tmdb id, match state, locked) of the movies.
fn movies(db: &Path) -> Vec<(String, Option<i64>, String, bool)> {
    let c = Connection::open(db).unwrap();
    c.prepare(
        "SELECT title, tmdb_id, coalesce(match_state, ''), locked FROM item WHERE kind = 'movie' ORDER BY title",
    )
    .unwrap()
    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0)))
    .unwrap()
    .collect::<std::result::Result<_, _>>()
    .unwrap()
}

fn poll<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The library's run once it is no longer waiting or running.
fn finished_run(core: &Core, rx: &Receiver<Event>, library: &str, next: &mut u64) -> Value {
    poll("run finished", || {
        *next += 1;
        core.matching_status(*next);
        let v = json(&answer(rx, *next));
        let l = v["libraries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["library"] == library)
            .unwrap()
            .clone();
        let state = l["run"]["state"].as_str().unwrap_or_default().to_string();
        (state == "done" || state == "stopped").then_some(l)
    })
}

fn open(data: &Path, provider: bool) -> (Core, Receiver<Event>) {
    let (tx, rx) = unbounded();
    let core = Core::open(data, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    if provider {
        core.set_provider(Arc::new(Fake::default()));
    }
    (core, rx)
}

#[test]
fn library_rematch_and_manual_search() {
    // SAFETY: set before any thread of this test process reads the environment.
    unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
    let d = tempfile::tempdir().unwrap();
    let media = d.path().join("media");
    for f in [
        "Alien (1979)/Alien.1979.mkv",
        "The Thing.mkv",
        "Nothing Here (2003).mkv",
    ] {
        let p = media.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, f.as_bytes()).unwrap();
    }
    let data = d.path().join("data");
    let db = data.join("library.db");
    let (tx, rx) = unbounded();
    let core = Core::open(&data, "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    let fake = Arc::new(Fake::default());
    core.set_provider(fake.clone());
    core.add_library(1, &media.to_string_lossy(), "video");
    poll("first matching", || {
        let m = movies(&db);
        (m.len() == 3
            && m.iter().any(|x| x.0 == "Alien" && x.2 == "matched")
            && m.iter()
                .any(|x| x.0 == "The Thing" && x.2 == "needs_review")
            && m.iter()
                .any(|x| x.0 == "Nothing Here" && x.2 == "unmatched"))
        .then_some(())
    });
    let mut next = 100;

    // A pasted TMDB link and an IMDb id find their record; the current match is marked.
    let alien = Connection::open(&db)
        .unwrap()
        .query_row(
            "SELECT id FROM item WHERE title = 'Alien' AND kind = 'movie'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();
    next += 1;
    core.match_search(
        next,
        alien,
        r#"{"text":"https://www.themoviedb.org/movie/348-alien"}"#,
    );
    let v = json(&answer(&rx, next));
    assert_eq!(v["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(v["candidates"][0]["id"], "348");
    assert_eq!(v["candidates"][0]["current"], true);
    assert_eq!(
        v["candidates"][0]["image"],
        "https://image.tmdb.org/t/p/w185/p348.png"
    );
    next += 1;
    core.match_search(next, alien, r#"{"text":"tt0078748"}"#);
    assert_eq!(json(&answer(&rx, next))["candidates"][0]["kind"], "movie");
    // A title search scores the candidates.
    next += 1;
    core.match_search(next, alien, r#"{"text":"Alien","year":1979}"#);
    let v = json(&answer(&rx, next));
    assert_eq!(v["candidates"].as_array().unwrap().len(), 2);
    assert!(v["candidates"][0]["score"].as_f64().unwrap() > 0.5);

    // The user chooses for The Thing: locked.
    let thing = Connection::open(&db)
        .unwrap()
        .query_row("SELECT id FROM item WHERE title = 'The Thing'", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap();
    next += 1;
    core.match_choose(next, thing, r#"{"id":"1091","kind":"movie"}"#);
    assert_eq!(json(&answer(&rx, next))["newItemId"], thing);
    poll("choice applied", || {
        movies(&db)
            .into_iter()
            .find(|m| m.0 == "The Thing" && m.1 == Some(1091) && m.3)
    });

    // "Unmatched and uncertain only": just Nothing Here, which is still not found.
    fake.remade.store(true, Ordering::SeqCst);
    next += 1;
    core.rematch_library(next, "movies", false, false);
    assert!(matches!(answer(&rx, next), Event::MusicDetail { .. }));
    let l = finished_run(&core, &rx, "movies", &mut next);
    assert_eq!(
        (l["run"]["total"].as_i64(), l["run"]["notFound"].as_i64()),
        (Some(1), Some(1))
    );
    assert_eq!(l["matched"], 2);
    assert_eq!(l["locked"], 1);

    // Everything, without the user's choices: Alien stays, Nothing Here still not found, The
    // Thing untouched though the provider now says otherwise.
    next += 1;
    core.rematch_library(next, "movies", true, false);
    answer(&rx, next);
    let l = finished_run(&core, &rx, "movies", &mut next);
    assert_eq!(l["run"]["total"], 2);
    assert_eq!(l["run"]["changed"], 0);
    assert!(
        movies(&db)
            .iter()
            .any(|m| m.0 == "The Thing" && m.1 == Some(1091) && m.3)
    );

    // With the user's choices: The Thing (no year) now finds only the 2011 film, which is not
    // sure enough to replace the choice: it waits for review, keeping its data, unlocked.
    next += 1;
    core.rematch_library(next, "movies", true, true);
    answer(&rx, next);
    let l = finished_run(&core, &rx, "movies", &mut next);
    assert_eq!(
        (l["run"]["total"].as_i64(), l["run"]["review"].as_i64()),
        (Some(3), Some(1))
    );
    let m = movies(&db);
    assert!(
        m.iter()
            .any(|m| m.0 == "The Thing" && m.1 == Some(1091) && m.2 == "needs_review" && !m.3),
        "{m:?}"
    );
    // Alien is still matched.
    assert!(
        m.iter()
            .any(|m| m.0 == "Alien" && m.1 == Some(348) && m.2 == "matched")
    );

    // One item again: a locked one says so first.
    Connection::open(&db)
        .unwrap()
        .execute("UPDATE item SET locked = 1 WHERE id = ?1", [alien])
        .unwrap();
    next += 1;
    core.match_again(next, alien, false);
    assert_eq!(json(&answer(&rx, next))["result"], "locked");
    next += 1;
    core.match_again(next, alien, true);
    let v = json(&answer(&rx, next));
    assert_eq!(v["result"], "unchanged");
    // Still the user's choice: kept until it changes. A sure other match replaces it.
    fake.moved.store(true, Ordering::SeqCst);
    next += 1;
    core.match_again(next, alien, true);
    assert_eq!(json(&answer(&rx, next))["result"], "changed");
    assert!(
        movies(&db)
            .iter()
            .any(|m| m.0 == "Alien" && m.1 == Some(999) && !m.3)
    );
    drop(core);

    // A run waiting when the app closed starts with the next start.
    let c = Connection::open(&db).unwrap();
    c.execute(
        "UPDATE setting SET value = '[{\"library\":\"movies\",\"all\":true,\"includeLocked\":false,\"state\":\"waiting\"}]' WHERE key = 'rematch_runs'",
        [],
    )
    .unwrap();
    drop(c);
    let (core, rx) = open(&data, true);
    let l = finished_run(&core, &rx, "movies", &mut next);
    assert_eq!(l["run"]["state"], "done");
    assert!(l["run"]["total"].as_i64().unwrap() >= 1);
    drop(core);

    // No key: movies cannot be matched again; nothing is queued.
    let (core, rx) = open(&data, false);
    next += 1;
    core.rematch_library(next, "movies", true, false);
    assert!(matches!(answer(&rx, next), Event::Error { .. }));
    next += 1;
    core.matching_status(next);
    let v = json(&answer(&rx, next));
    assert_eq!(v["libraries"][0]["blocked"], "noKey");
    // Online information off: music and books cannot either.
    next += 1;
    core.set_music_online(next, false);
    std::thread::sleep(Duration::from_millis(300));
    next += 1;
    core.rematch_library(next, "books", false, false);
    assert!(matches!(answer(&rx, next), Event::Error { .. }));
    next += 1;
    core.matching_status(next);
    let v = json(&answer(&rx, next));
    assert_eq!(v["libraries"][3]["blocked"], "offline");
}
