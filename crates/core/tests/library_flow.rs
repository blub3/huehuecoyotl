//! M2 end to end through the public core API with a fake metadata provider:
//! add library -> scan -> match -> details -> artwork -> browse/search/detail -> review.
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event};
use huehuecoyotl_metadata::{
    Candidate, Details, EpisodeDetails, MediaType, MetadataProvider, Result, SeasonDetails,
    SeasonRef,
};

struct Fake;

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
    async fn search(
        &self,
        ty: MediaType,
        title: &str,
        _year: Option<u16>,
    ) -> Result<Vec<Candidate>> {
        Ok(match (ty, title.to_lowercase().as_str()) {
            (MediaType::Movie, "alien") => {
                vec![cand(348, "Alien", 1979), cand(679, "Aliens", 1986)]
            }
            (MediaType::Movie, "the thing") => vec![
                cand(1091, "The Thing", 1982),
                cand(60935, "The Thing", 2011),
            ],
            (MediaType::Show, "firefly") => vec![cand(1437, "Firefly", 2002)],
            _ => vec![],
        })
    }
    async fn details(&self, _ty: MediaType, id: i64) -> Result<Details> {
        let (title, year) = match id {
            348 => ("Alien", 1979),
            1091 => ("The Thing", 1982),
            1437 => ("Firefly", 2002),
            _ => ("Other", 2000),
        };
        Ok(Details {
            id,
            title: title.into(),
            original_title: title.into(),
            year: Some(year),
            overview: format!("Overview of {title}"),
            rating: Some(8.0),
            genres: vec!["Science Fiction".into()],
            poster_path: Some(format!("/p{id}.png")),
            backdrop_path: Some(format!("/b{id}.png")),
            seasons: vec![SeasonRef {
                season_no: 1,
                name: "Season 1".into(),
                poster_path: None,
                episode_count: 2,
            }],
            ..Default::default()
        })
    }
    async fn season(&self, _show: i64, season_no: u16) -> Result<SeasonDetails> {
        Ok(SeasonDetails {
            season_no,
            name: "Season 1".into(),
            episodes: vec![
                EpisodeDetails {
                    episode_no: 1,
                    title: "Serenity".into(),
                    overview: "Pilot".into(),
                    still_path: Some("/e1.png".into()),
                    ..Default::default()
                },
                EpisodeDetails {
                    episode_no: 2,
                    title: "The Train Job".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        })
    }
    async fn image(&self, _path: &str, _size: &str) -> Result<Bytes> {
        let img = image::RgbImage::from_pixel(500, 750, image::Rgb([200, 120, 40]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        Ok(Bytes::from(png))
    }
}

fn wait_for(rx: &Receiver<Event>, what: &str, mut f: impl FnMut(&Event) -> bool) -> Event {
    let deadline = Instant::now() + Duration::from_secs(20);
    while let Ok(e) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        if f(&e) {
            return e;
        }
    }
    panic!("timed out waiting for {what}");
}

/// Poll a query until `ok` holds (metadata jobs run in the background).
fn until(
    core: &Core,
    rx: &Receiver<Event>,
    what: &str,
    mut ask: impl FnMut(&Core, u64),
    mut ok: impl FnMut(&Event) -> bool,
) -> Event {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut id = 1000;
    loop {
        id += 1;
        ask(core, id);
        let e = wait_for(
            rx,
            what,
            |e| matches!(e, Event::Page { request_id, .. } | Event::Detail { request_id, .. } | Event::Review { request_id, .. } if *request_id == id),
        );
        if ok(&e) {
            return e;
        }
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn library_end_to_end() {
    // Never touch the developer's real keystore (it may hold a real TMDB key).
    // SAFETY: set before any thread of this test process reads the environment.
    unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
    let d = tempfile::tempdir().unwrap();
    let media = d.path().join("media");
    for f in [
        "Alien (1979)/Alien.1979.1080p.mkv",
        "The Thing.mkv",
        "Firefly/Season 1/Firefly.S01E01.mkv",
        "Firefly/Season 1/Firefly.S01E02.mkv",
    ] {
        let p = media.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, f.as_bytes()).unwrap();
    }
    let (tx, rx) = unbounded();
    let core = Core::open(&d.path().join("data"), "en-US", move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    core.set_provider(Arc::new(Fake));

    core.add_library(1, &media.to_string_lossy());
    let Event::Libraries { libraries, .. } = wait_for(&rx, "libraries", |e| {
        matches!(e, Event::Libraries { request_id: 1, .. })
    }) else {
        unreachable!()
    };
    assert_eq!(libraries.len(), 1);
    wait_for(&rx, "scan end", |e| {
        matches!(e, Event::ScanState { running: false, .. })
    });

    // Alien: confident match, details and a downscaled grid poster on disk.
    let Event::Page { items, .. } = until(
        &core,
        &rx,
        "movies with posters",
        |c, id| c.browse_items(id, "movie", 0, "title", "", 0, 50),
        |e| matches!(e, Event::Page { items, .. } if items.iter().any(|i| i.title == "Alien" && !i.poster.is_empty())),
    ) else {
        unreachable!()
    };
    let alien = items.iter().find(|i| i.title == "Alien").unwrap();
    assert_eq!(alien.year, Some(1979));
    let poster = image::open(&alien.poster).unwrap();
    assert_eq!(poster.width(), 360, "grid thumbnail is pre-scaled");
    // The larger tier comes from the same download (never upscaled: the fake image is 500 px).
    let large = image::open(&alien.poster_large).unwrap();
    assert_eq!(large.width(), 500, "large grid tier made with the download");

    // The Thing has no year: two remakes -> review list, not a guess.
    let Event::Review { items: review, .. } = until(
        &core,
        &rx,
        "review list",
        |c, id| c.review(id),
        |e| matches!(e, Event::Review { items, .. } if !items.is_empty()),
    ) else {
        unreachable!()
    };
    assert_eq!(review[0].item.title, "The Thing");
    assert_eq!(review[0].candidates.len(), 2);
    core.choose_match(review[0].item.id, 1091);
    let thing = review[0].item.id;
    let Event::Detail { detail, .. } = until(
        &core,
        &rx,
        "chosen match applied",
        move |c, id| c.item_detail(id, thing),
        |e| matches!(e, Event::Detail { detail, .. } if detail.item.year == Some(1982) && detail.match_state == "matched"),
    ) else {
        unreachable!()
    };
    assert_eq!(detail.tmdb_id, Some(1091));

    // Firefly: show with episode titles from the season call.
    let Event::Page { items: shows, .. } = until(
        &core,
        &rx,
        "show matched",
        |c, id| c.browse_items(id, "show", 0, "title", "", 0, 50),
        |e| matches!(e, Event::Page { items, .. } if items.first().is_some_and(|s| !s.overview.is_empty())),
    ) else {
        unreachable!()
    };
    let show = shows[0].id;
    assert_eq!(shows[0].unwatched, 2);
    let Event::Detail { detail, .. } = until(
        &core,
        &rx,
        "episodes titled",
        move |c, id| c.item_detail(id, show),
        |e| matches!(e, Event::Detail { detail, .. } if detail.next_up.as_ref().is_some_and(|n| n.title == "Serenity")),
    ) else {
        unreachable!()
    };
    let season = detail.children[0].id;

    // Search finds by title prefix and by overview words.
    core.search(50, "fire");
    let Event::Page { items, .. } = wait_for(&rx, "search", |e| {
        matches!(e, Event::Page { request_id: 50, .. })
    }) else {
        unreachable!()
    };
    assert_eq!(items[0].id, show);

    // Watching a season counts down the show.
    core.set_watched(season, true);
    until(
        &core,
        &rx,
        "season watched",
        |c, id| c.browse_items(id, "show", 0, "title", "", 0, 50),
        |e| matches!(e, Event::Page { items, .. } if items.first().is_some_and(|s| s.unwatched == 0)),
    );
}
