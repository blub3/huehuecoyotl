//! Issue #32: show matching against TMDB-shaped responses served by a mock server, through the
//! real TMDB client. The titles are made up; the cases are the ones seen in a real library:
//! punctuation, a country suffix, and a title used by two different shows.
use huehuecoyotl_core::find_match;
use huehuecoyotl_metadata::matching::Decision;
use huehuecoyotl_metadata::tmdb::Tmdb;
use huehuecoyotl_metadata::{MediaType, MetadataProvider};
use serde_json::Value;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> Value {
    let p = format!("{}/tests/fixtures/tmdb/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn empty() -> Value {
    serde_json::json!({"page": 1, "total_pages": 0, "total_results": 0, "results": []})
}

async fn tmdb() -> (MockServer, Tmdb) {
    let srv = MockServer::start().await;
    let ok = |v: Value| ResponseTemplate::new(200).set_body_json(v);
    for (q, body) in [
        // The full titles find nothing; the shorter query finds the show with the colon.
        ("Example Show The Return", empty()),
        ("example show the", empty()),
        ("example show", fixture("search-example-show.json")),
        ("Example Show", fixture("search-example-show.json")),
        ("Repeat Show", fixture("search-repeat-show.json")),
    ] {
        Mock::given(method("GET"))
            .and(path("/search/tv"))
            .and(query_param("query", q))
            .respond_with(ok(body))
            .mount(&srv)
            .await;
    }
    for id in [300, 301] {
        Mock::given(method("GET"))
            .and(path(format!("/tv/{id}")))
            .respond_with(ok(fixture(&format!("tv-{id}.json"))))
            .mount(&srv)
            .await;
    }
    let t = Tmdb::with_base(
        "0123456789abcdef0123456789abcdef",
        "en-US",
        &srv.uri(),
        &srv.uri(),
    )
    .unwrap();
    (srv, t)
}

async fn best(p: &dyn MetadataProvider, title: &str, on_disk: &[(u16, u16)]) -> (Decision, i64) {
    let (d, cands, _) = find_match(p, MediaType::Show, title, None, on_disk)
        .await
        .unwrap();
    (d, cands.first().map_or(0, |c| c.id))
}

#[tokio::test]
async fn punctuation_country_and_ambiguous_titles() {
    let (_srv, t) = tmdb().await;
    // Folder "Example Show The Return", TMDB "Example Show: The Return".
    assert_eq!(
        best(&t, "Example Show The Return", &[(1, 8)]).await,
        (Decision::Matched, 101)
    );
    // Folder "Example Show US": the American show, not the British original.
    assert_eq!(
        best(&t, "Example Show US", &[(1, 6)]).await,
        (Decision::Matched, 201)
    );
    // A title used by a 1970s and a 2000s show: season 3 on disk only exists in the remake.
    assert_eq!(
        best(&t, "Repeat Show", &[(0, 1), (1, 13), (3, 18)]).await,
        (Decision::Matched, 301)
    );
    // If both could hold the episodes on disk, the user decides: review, not a guess.
    assert_eq!(
        best(&t, "Repeat Show", &[(1, 10)]).await.0,
        Decision::NeedsReview
    );
}

/// The issue's acceptance test against the real TMDB API: "Pioneer One (2010)/Season 01/"
/// (a Creative Commons web series) matches TMDB show 33050.
///   COYOTL_TMDB_KEY=… cargo test -p huehuecoyotl-core --features live-tmdb --test show_matching
#[cfg(feature = "live-tmdb")]
#[tokio::test]
async fn live_pioneer_one() {
    let key = std::env::var("COYOTL_TMDB_KEY").expect("COYOTL_TMDB_KEY");
    let p =
        huehuecoyotl_scanner::parse::parse("Pioneer One (2010)/Season 01/Pioneer.One.S01E01.mkv");
    let title = p.show_title.clone().unwrap();
    assert_eq!((title.as_str(), p.show_year), ("Pioneer One", Some(2010)));
    let t = Tmdb::new(key, "en-US").unwrap();
    let (d, cands, _) = find_match(&t, MediaType::Show, &title, p.show_year, &[(1, 1)])
        .await
        .unwrap();
    assert_eq!((d, cands[0].id), (Decision::Matched, 33050));
}
