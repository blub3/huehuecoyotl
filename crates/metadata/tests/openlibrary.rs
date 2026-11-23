//! Issue #67: the Open Library client against a mock server, with responses recorded from the
//! live service on 2026-11-16 (bibliographic data; the work's description is replaced by fixture
//! text): an ISBN match in one request, a title search that decides without guessing, the
//! User-Agent on every request, the limiter under concurrent jobs, backoff on 403 and 429, covers
//! by cover id only.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use huehuecoyotl_metadata::books::{BookDecision, BookProvider, BookQuery, decide};
use huehuecoyotl_metadata::musicbrainz::user_agent;
use huehuecoyotl_metadata::openlibrary::OpenLibrary;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const ISBN: &str = include_str!("fixtures/ol_search_isbn.json");
const TITLE: &str = include_str!("fixtures/ol_search_title.json");
const WORK: &str = include_str!("fixtures/ol_work.json");
const VERSION: &str = "2.5.0";

fn client(server: &MockServer, interval: Duration) -> OpenLibrary {
    OpenLibrary::with_bases(
        VERSION,
        &server.uri(),
        &format!("{}/covers", server.uri()),
        interval,
    )
    .unwrap()
}

/// Every request the server saw carried the app's User-Agent.
async fn all_identified(server: &MockServer) {
    for r in server.received_requests().await.unwrap() {
        assert_eq!(
            r.headers.get("user-agent").unwrap().to_str().unwrap(),
            "Huehuecoyotl/2.5.0 ( https://github.com/blub3/huehuecoyotl )",
            "{}",
            r.url
        );
    }
}

#[tokio::test]
async fn an_isbn_matches_with_one_search() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .and(query_param("q", "isbn:9780141439518"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ISBN))
        .expect(1)
        .mount(&server)
        .await;
    let ol = client(&server, Duration::from_millis(1));
    let hits = ol
        .search(BookQuery::Isbn("978-0-14-143951-8"))
        .await
        .unwrap();
    let BookDecision::Matched(h) = decide(&hits, "Pride and Prejudice", &[], true) else {
        panic!("no match: {hits:?}");
    };
    assert_eq!(h.id, "OL66554W");
    assert_eq!(h.authors, ["Jane Austen"]);
    assert_eq!(h.year, Some(1813));
    assert_eq!(h.cover_id, Some(14348537));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    all_identified(&server).await;
    assert_eq!(
        user_agent(VERSION),
        "Huehuecoyotl/2.5.0 ( https://github.com/blub3/huehuecoyotl )"
    );
}

#[tokio::test]
async fn a_title_search_decides_without_guessing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .and(query_param("title", "Pride and Prejudice"))
        .and(query_param("author", "Jane Austen"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TITLE))
        .mount(&server)
        .await;
    let ol = client(&server, Duration::from_millis(1));
    let hits = ol
        .search(BookQuery::TitleAuthor {
            title: "Pride and Prejudice",
            author: "Jane Austen",
        })
        .await
        .unwrap();
    assert_eq!(hits.len(), 5);
    // The catalogue lists the novel twice; the first is the match, the collections are not.
    let d = decide(&hits, "Pride and Prejudice", &["Jane Austen".into()], false);
    assert!(matches!(d, BookDecision::Matched(ref h) if h.id == "OL66554W"));
    // Another author's book of that name is not matched.
    let d = decide(
        &hits,
        "Pride and Prejudice",
        &["Seth Grahame-Smith".into()],
        false,
    );
    assert!(matches!(d, BookDecision::Review(_)));
    all_identified(&server).await;
}

#[tokio::test]
async fn a_work_and_its_cover() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/works/OL66554W.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(WORK))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/covers/b/id/14348537-L.jpg"))
        .and(query_param("default", "false"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"\xFF\xD8jpeg".to_vec()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/covers/b/id/1-L.jpg"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let ol = client(&server, Duration::from_millis(1));
    let w = ol.work("/works/OL66554W").await.unwrap();
    assert_eq!(w.id, "OL66554W");
    assert!(w.description.unwrap().starts_with("A novel of manners"));
    assert_eq!(w.year, Some(1853));
    assert_eq!(w.cover_ids[0], 14348537);
    assert_eq!(w.subjects.len(), 12);
    let cover = ol.cover(14348537).await.unwrap().unwrap();
    assert!(cover.starts_with(b"\xFF\xD8"));
    assert_eq!(ol.cover(1).await.unwrap(), None);
    // Ids that are not Open Library's never become a path.
    assert!(ol.work("../../etc").await.is_err());
    all_identified(&server).await;
}

/// Records when each request arrived.
struct Clock(Arc<Mutex<Vec<Instant>>>);
impl Respond for Clock {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        self.0.lock().unwrap().push(Instant::now());
        ResponseTemplate::new(200).set_body_string(WORK)
    }
}

/// Jobs running at the same time share one pace: never two requests closer than the interval,
/// covers included.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_limiter_holds_under_concurrent_jobs() {
    let server = MockServer::start().await;
    let times = Arc::new(Mutex::new(Vec::new()));
    Mock::given(method("GET"))
        .respond_with(Clock(times.clone()))
        .mount(&server)
        .await;
    let interval = Duration::from_millis(150);
    let ol = Arc::new(client(&server, interval));
    let mut tasks = Vec::new();
    for i in 0..4 {
        let ol = ol.clone();
        tasks.push(tokio::spawn(async move {
            for _ in 0..3 {
                if i % 2 == 0 {
                    ol.work("OL66554W").await.unwrap();
                } else {
                    ol.cover(14348537).await.unwrap();
                }
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
        assert!(
            gap >= interval - Duration::from_millis(15),
            "requests {gap:?} apart"
        );
    }
}

#[tokio::test]
async fn slow_down_answers_back_off_and_retry() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/works/OL66554W.json"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/works/OL66554W.json"))
        .respond_with(ResponseTemplate::new(403).insert_header("retry-after", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/works/OL66554W.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(WORK))
        .mount(&server)
        .await;
    let w = client(&server, Duration::from_millis(1))
        .work("OL66554W")
        .await
        .unwrap();
    assert_eq!(w.id, "OL66554W");
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

/// Against the live service, at its pace: one ISBN search and one work for a public-domain
/// novel. Not run in CI:
///   cargo test -p huehuecoyotl-metadata --features live-openlibrary --test openlibrary
#[cfg(feature = "live-openlibrary")]
#[tokio::test]
async fn live_lookup() {
    let ol = OpenLibrary::new(VERSION).unwrap();
    let hits = ol.search(BookQuery::Isbn("9780141439518")).await.unwrap();
    let BookDecision::Matched(h) = decide(&hits, "Pride and Prejudice", &[], true) else {
        panic!("no match: {hits:?}");
    };
    let w = ol.work(&h.id).await.unwrap();
    assert!(w.description.is_some());
}
