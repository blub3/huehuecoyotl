//! Issue #11: the MusicBrainz client against a mock server, with responses recorded from the
//! live service (core data, CC0): parsing, the User-Agent on every request, the rate limiter under
//! concurrent jobs, backoff on 503, and the Cover Art Archive's fallback to the release group.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use huehuecoyotl_metadata::music::{Decision, MusicProvider, decide};
use huehuecoyotl_metadata::musicbrainz::{MusicBrainz, user_agent};
use wiremock::matchers::{header, method, path, path_regex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const RELEASE: &str = include_str!("fixtures/mb_release.json");
const SEARCH: &str = include_str!("fixtures/mb_search.json");
const KIND_OF_BLUE: &str = "e32a3f0b-1c19-3170-bb1c-650893774744";
const VERSION: &str = "2.0.0";

fn client(server: &MockServer, interval: Duration) -> MusicBrainz {
    MusicBrainz::with_bases(
        VERSION,
        &format!("{}/ws/2", server.uri()),
        &format!("{}/art", server.uri()),
        interval,
    )
    .unwrap()
}

#[tokio::test]
async fn a_tagged_album_takes_one_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/ws/2/release/{KIND_OF_BLUE}")))
        .and(header("user-agent", user_agent(VERSION).as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_string(RELEASE))
        .expect(1)
        .mount(&server)
        .await;
    let r = client(&server, Duration::from_millis(1))
        .release(KIND_OF_BLUE)
        .await
        .unwrap();
    assert_eq!(r.title, "Kind of Blue");
    assert_eq!(r.release_type.as_deref(), Some("album"));
    assert_eq!(
        r.group_id.as_deref(),
        Some("8e8a594f-2175-38c7-a871-abb68ec363e7")
    );
    assert_eq!(r.artists[0].name, "Miles Davis");
    assert_eq!(r.media.len(), 1);
    let titles: Vec<&str> = r.media[0].tracks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "So What",
            "Freddie Freeloader",
            "Blue in Green",
            "All Blues",
            "Flamenco Sketches"
        ]
    );
    assert_eq!(
        r.media[0].tracks[0].recording_id,
        "60f750f4-d222-46b7-ad5c-905334c9a48a"
    );
    // The User-Agent names the app, its real version and the project.
    assert_eq!(
        user_agent(VERSION),
        "Huehuecoyotl/2.0.0 ( https://github.com/blub3/huehuecoyotl )"
    );
}

#[tokio::test]
async fn a_search_decides_without_guessing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ws/2/release/"))
        .and(header("user-agent", user_agent(VERSION).as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_string(SEARCH))
        .mount(&server)
        .await;
    let mb = client(&server, Duration::from_millis(1));
    let hits = mb
        .search_releases("Miles Davis", "Kind of Blue", Some(5))
        .await
        .unwrap();
    assert_eq!(hits.len(), 5);
    assert!(hits.iter().all(|h| h.score == 100));
    // Five editions of one release group: matched, the first one.
    assert_eq!(
        decide(&hits, "Miles Davis", "Kind of Blue", Some(5)),
        Decision::Matched(KIND_OF_BLUE.into())
    );
    // The same results for an album with nine tracks are not a match.
    assert!(matches!(
        decide(&hits, "Miles Davis", "Kind of Blue", Some(9)),
        Decision::Review(_)
    ));
    let q = &server.received_requests().await.unwrap()[0];
    let query: String = q
        .url
        .query_pairs()
        .find(|(k, _)| k == "query")
        .unwrap()
        .1
        .into();
    assert_eq!(
        query,
        "release:\"Kind of Blue\" AND artist:\"Miles Davis\" AND tracks:5"
    );
}

/// Records when each request arrived.
struct Clock(Arc<Mutex<Vec<Instant>>>);
impl Respond for Clock {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        self.0.lock().unwrap().push(Instant::now());
        ResponseTemplate::new(200).set_body_string(RELEASE)
    }
}

/// Jobs running at the same time share one pace: never two requests closer than the interval.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_limiter_holds_under_concurrent_jobs() {
    let server = MockServer::start().await;
    let times = Arc::new(Mutex::new(Vec::new()));
    Mock::given(method("GET"))
        .and(path_regex("^/ws/2/release/.+"))
        .respond_with(Clock(times.clone()))
        .mount(&server)
        .await;
    let interval = Duration::from_millis(150);
    let mb = Arc::new(client(&server, interval));
    let mut tasks = Vec::new();
    for _ in 0..4 {
        let mb = mb.clone();
        tasks.push(tokio::spawn(async move {
            for _ in 0..3 {
                mb.release(KIND_OF_BLUE).await.unwrap();
            }
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    let mut t = times.lock().unwrap().clone();
    t.sort();
    assert_eq!(t.len(), 12);
    for w in t.windows(2) {
        let gap = w[1] - w[0];
        // A little slack for the time between the limiter and the server's clock.
        assert!(
            gap >= interval - Duration::from_millis(15),
            "requests {gap:?} apart"
        );
    }
}

#[tokio::test]
async fn busy_answers_back_off_and_retry() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/ws/2/release/{KIND_OF_BLUE}")))
        .respond_with(ResponseTemplate::new(503).insert_header("retry-after", "0"))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/ws/2/release/{KIND_OF_BLUE}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(RELEASE))
        .mount(&server)
        .await;
    let r = client(&server, Duration::from_millis(1))
        .release(KIND_OF_BLUE)
        .await
        .unwrap();
    assert_eq!(r.title, "Kind of Blue");
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn covers_fall_back_to_the_release_group() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/art/release/r1/front-1200"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/art/release-group/g1/front-1200"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"\xFF\xD8jpeg".to_vec()))
        .mount(&server)
        .await;
    let mb = client(&server, Duration::from_millis(1));
    let cover = mb.front_cover("r1", Some("g1")).await.unwrap().unwrap();
    assert!(cover.starts_with(b"\xFF\xD8"));
    assert_eq!(mb.front_cover("r1", None).await.unwrap(), None);
}

/// Against the live service: one search and one lookup, at its pace. Not run in CI:
///   cargo test -p huehuecoyotl-metadata --features live-musicbrainz --test musicbrainz
#[cfg(feature = "live-musicbrainz")]
#[tokio::test]
async fn live_lookup() {
    let mb = MusicBrainz::new(VERSION).unwrap();
    let hits = mb
        .search_releases("Miles Davis", "Kind of Blue", Some(5))
        .await
        .unwrap();
    let Decision::Matched(id) = decide(&hits, "Miles Davis", "Kind of Blue", Some(5)) else {
        panic!("no match: {hits:?}");
    };
    let r = mb.release(&id).await.unwrap();
    assert_eq!(r.media[0].tracks.len(), 5);
}
