//! Issues #97 to #100 through the public core API with a fake TMDB: the preview of a change (the
//! files that move and as what, episodes a show's match lacks), movies that become a show with
//! their watch state, and an episode that becomes a movie.
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event};
use huehuecoyotl_library::rusqlite::Connection;
use huehuecoyotl_metadata::{
    Candidate, Details, MediaType, MetadataProvider, Result, SeasonDetails, SeasonRef,
};
use serde_json::{Value, json};

struct Fake;

#[async_trait]
impl MetadataProvider for Fake {
    async fn validate(&self) -> Result<()> {
        Ok(())
    }
    async fn search(&self, _ty: MediaType, _t: &str, _y: Option<u16>) -> Result<Vec<Candidate>> {
        Ok(vec![])
    }
    async fn details(&self, ty: MediaType, id: i64) -> Result<Details> {
        Ok(match ty {
            MediaType::Show => Details {
                id,
                title: "The Long Harbour".into(),
                year: Some(2019),
                // TMDB knows one episode of season 1.
                seasons: vec![SeasonRef {
                    season_no: 1,
                    name: "Season 1".into(),
                    episode_count: 1,
                    ..Default::default()
                }],
                ..Default::default()
            },
            MediaType::Movie => Details {
                id,
                title: "Night Shift".into(),
                year: Some(2019),
                ..Default::default()
            },
        })
    }
    async fn season(&self, _show: i64, season_no: u16) -> Result<SeasonDetails> {
        Ok(SeasonDetails {
            season_no,
            ..Default::default()
        })
    }
    async fn image(&self, _path: &str, _size: &str) -> Result<Bytes> {
        Err(huehuecoyotl_metadata::MetaError::NotFound)
    }
}

fn answer(rx: &Receiver<Event>, id: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    while let Ok(e) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        match e {
            Event::MusicDetail {
                request_id, json, ..
            } if request_id == id => {
                return serde_json::from_str(&json).unwrap();
            }
            Event::Error {
                request_id,
                message,
                ..
            } if request_id == id => panic!("error: {message}"),
            _ => {}
        }
    }
    panic!("no answer to request {id}");
}

fn count(db: &Path, sql: &str) -> i64 {
    Connection::open(db)
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}

#[test]
fn movies_become_a_show_and_an_episode_a_movie() {
    // SAFETY: set before any thread of this test process reads the environment.
    unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
    let d = tempfile::tempdir().unwrap();
    let media = d.path().join("media");
    // Named so the scanner takes them for films; their names still carry the episode numbers.
    for f in [
        "Long Harbour/Long Harbour Folge 1.mkv",
        "Long Harbour/Long Harbour Folge 2.mkv",
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
    core.set_provider(Arc::new(Fake));
    core.add_library(1, &media.to_string_lossy(), "video");
    let deadline = Instant::now() + Duration::from_secs(20);
    while count(&db, "SELECT count(*) FROM item WHERE kind = 'movie'") < 2 {
        assert!(Instant::now() < deadline, "scan");
        std::thread::sleep(Duration::from_millis(100));
    }
    let first: i64 = Connection::open(&db)
        .unwrap()
        .query_row(
            "SELECT i.id FROM item i JOIN file f ON f.item_id = i.id WHERE f.path LIKE '%Folge 1%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    // The first one was watched.
    Connection::open(&db)
        .unwrap()
        .execute(
            "INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at) VALUES (?1, 1, 0, 9, 1, 1)",
            [first],
        )
        .unwrap();

    // Preview: its own file is fixed, the other movie in the folder is offered, with numbers.
    core.match_preview(1, first, r#"{"id":"77","kind":"show"}"#);
    let p = answer(&rx, 1);
    assert_eq!(p["change"], "toShow");
    let files = p["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(
        (files[0]["fixed"].as_bool(), files[0]["episode"].as_i64()),
        (Some(true), Some(1))
    );
    assert_eq!(
        (files[1]["on"].as_bool(), files[1]["episode"].as_i64()),
        (Some(true), Some(2))
    );

    // Choose it with both files.
    let choice = json!({ "id": "77", "kind": "show", "files": files.iter().map(|f| json!({
        "fileId": f["fileId"], "season": f["season"], "episode": f["episode"] })).collect::<Vec<_>>() });
    core.match_choose(2, first, &choice.to_string());
    let show = answer(&rx, 2)["newItemId"].as_i64().unwrap();
    assert_eq!(
        count(&db, "SELECT count(*) FROM item WHERE kind = 'movie'"),
        0
    );
    assert_eq!(
        count(
            &db,
            &format!(
                "SELECT count(*) FROM item WHERE id = {show} AND kind = 'show' AND tmdb_id = 77 AND locked = 1"
            )
        ),
        1
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM file f JOIN item e ON e.id = f.item_id WHERE e.kind = 'episode' AND e.virtual = 0"
        ),
        2
    );
    assert_eq!(
        count(
            &db,
            "SELECT watched FROM playstate p JOIN item e ON e.id = p.item_id WHERE e.kind = 'episode' AND e.episode_no = 1"
        ),
        1,
        "the watch state moved with the file"
    );

    // The show's own preview: TMDB knows only one episode of season 1.
    core.match_preview(3, show, r#"{"id":"77","kind":"show"}"#);
    let p = answer(&rx, 3);
    assert_eq!(p["change"], "none");
    assert_eq!(p["missing"], json!(["S01E02"]));

    // Episode 2 is really a film.
    let ep2: i64 = Connection::open(&db)
        .unwrap()
        .query_row(
            "SELECT id FROM item WHERE kind = 'episode' AND episode_no = 2",
            [],
            |r| r.get(0),
        )
        .unwrap();
    core.match_preview(4, ep2, r#"{"id":"9","kind":"movie"}"#);
    let p = answer(&rx, 4);
    assert_eq!(p["change"], "toMovie");
    let own = p["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["fixed"] == true)
        .unwrap()
        .clone();
    // Only its own file: untick the show's other file in the folder.
    let choice = json!({ "id": "9", "kind": "movie", "files": [{ "fileId": own["fileId"] }] });
    core.match_choose(5, ep2, &choice.to_string());
    let movie = answer(&rx, 5)["newItemId"].as_i64().unwrap();
    assert_eq!(
        count(
            &db,
            &format!("SELECT count(*) FROM file WHERE item_id = {movie}")
        ),
        1
    );
    assert_eq!(
        count(&db, "SELECT count(*) FROM item WHERE kind = 'show'"),
        1,
        "the show keeps episode 1"
    );
}
