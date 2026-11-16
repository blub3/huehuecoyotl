//! Issue #12: TheAudioDB and Fanart.tv against a mock server. The fixtures have the structure of
//! recorded answers (field names checked against the live services) with neutral placeholder
//! text and no keys. Covered: where each key goes, parsing, the biography language with English
//! fallback, choosing among several images, unknown ids, rejected keys, and that no error
//! message carries the key.
use std::time::Duration;

use huehuecoyotl_metadata::MetaError;
use huehuecoyotl_metadata::fanarttv::FanartTv;
use huehuecoyotl_metadata::music::MusicProvider;
use huehuecoyotl_metadata::theaudiodb::TheAudioDb;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ARTIST: &str = "cc197bad-dc9c-440d-a5b5-d52ba2e14234";
const GROUP: &str = "1dc4c347-a1db-32aa-b14f-bc9cc507b843";
const KEY: &str = "test-key-7f3a";

fn fixture(name: &str, base: &str) -> String {
    let raw = std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    raw.replace("{BASE}", base)
}

fn tadb(server: &MockServer, key: &str) -> TheAudioDb {
    TheAudioDb::with_base(
        "2.0.0",
        key,
        &format!("{}/api/v1/json", server.uri()),
        Duration::from_millis(1),
    )
    .unwrap()
}

fn fanart(server: &MockServer, key: &str, lang: &str) -> FanartTv {
    FanartTv::with_base(
        "2.0.0",
        key,
        lang,
        &format!("{}/v3", server.uri()),
        Duration::from_millis(1),
    )
    .unwrap()
}

#[tokio::test]
async fn theaudiodb_artist_album_and_language() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/json/{KEY}/artist-mb.php")))
        .and(query_param("i", ARTIST))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(fixture("tadb_artist.json", &server.uri())),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/json/{KEY}/album-mb.php")))
        .and(query_param("i", GROUP))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(fixture("tadb_album.json", &server.uri())),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/json/{KEY}/artist-mb.php")))
        .and(query_param("i", "unknown"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"artists":null}"#))
        .mount(&server)
        .await;
    let p = tadb(&server, KEY);
    let a = p.artist_extras(ARTIST).await.unwrap().unwrap();
    let base = server.uri();
    assert_eq!(
        a.image.as_deref(),
        Some(format!("{base}/img/artist/thumb/example.jpg").as_str())
    );
    assert_eq!(
        a.background.as_deref(),
        Some(format!("{base}/img/artist/fanart/example.jpg").as_str())
    );
    assert_eq!(
        a.logo.as_deref(),
        Some(format!("{base}/img/artist/logo/example.png").as_str())
    );
    // In the UI language, else English; an empty text does not count.
    assert_eq!(
        a.biography("de-DE"),
        Some("Example Band ist eine Band aus den Testdaten.")
    );
    assert_eq!(
        a.biography("fr-FR"),
        Some("Example Band are a band from the test fixtures.")
    );
    assert_eq!(
        a.biography("ja"),
        Some("Example Band are a band from the test fixtures.")
    );
    let art = p.album_art(GROUP).await.unwrap().unwrap();
    assert_eq!(
        art.cover.as_deref(),
        Some(format!("{base}/img/album/thumb/example.jpg").as_str())
    );
    assert_eq!(
        art.disc.as_deref(),
        Some(format!("{base}/img/album/cdart/example.png").as_str())
    );
    assert_eq!(p.artist_extras("unknown").await.unwrap(), None);
}

#[tokio::test]
async fn theaudiodb_rejects_a_key_without_showing_it() {
    let server = MockServer::start().await;
    // TheAudioDB answers an unknown key with a 404 HTML page.
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_string("<html>404 - File or directory not found.</html>"),
        )
        .mount(&server)
        .await;
    let p = tadb(&server, KEY);
    let e = p.validate().await.unwrap_err();
    assert!(matches!(e, MetaError::Auth));
    assert!(!e.to_string().contains(KEY));
    // A server that is down: the error names no URL, so no key.
    let gone = TheAudioDb::with_base(
        "2.0.0",
        KEY,
        "http://127.0.0.1:9/api/v1/json",
        Duration::from_millis(1),
    )
    .unwrap();
    let e = gone.artist_extras(ARTIST).await.unwrap_err();
    assert!(matches!(e, MetaError::Network(_)));
    assert!(!e.to_string().contains(KEY), "{e}");
}

#[tokio::test]
async fn fanart_picks_the_best_images() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v3/music/{ARTIST}")))
        .and(header("api-key", KEY))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(fixture("fanart_artist.json", &server.uri())),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v3/music/unknown"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_string(r#"{"status":"error","error message":"Not found"}"#),
        )
        .mount(&server)
        .await;
    let base = server.uri();
    let a = fanart(&server, KEY, "de-DE")
        .artist_extras(ARTIST)
        .await
        .unwrap()
        .unwrap();
    // The most liked thumb and background; the logo in the UI language before a better-liked
    // one in another.
    assert_eq!(
        a.image.as_deref(),
        Some(format!("{base}/img/fa/thumb-best.jpg").as_str())
    );
    assert_eq!(
        a.background.as_deref(),
        Some(format!("{base}/img/fa/bg-best.jpg").as_str())
    );
    assert_eq!(
        a.logo.as_deref(),
        Some(format!("{base}/img/fa/logo-de.png").as_str())
    );
    // Without one in the UI language: English.
    let a2 = fanart(&server, KEY, "it-IT")
        .artist_extras(ARTIST)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        a2.logo.as_deref(),
        Some(format!("{base}/img/fa/logo-en.png").as_str())
    );
    assert!(a.biographies.is_empty());
    // The albums' art comes with the artist.
    let (g, art) = &a.albums[0];
    assert_eq!(g, GROUP);
    assert_eq!(
        art.cover.as_deref(),
        Some(format!("{base}/img/fa/cover.jpg").as_str())
    );
    assert_eq!(
        art.disc.as_deref(),
        Some(format!("{base}/img/fa/cdart.png").as_str())
    );
    assert_eq!(a.albums[1].1.cover, None);
    assert_eq!(
        fanart(&server, KEY, "en")
            .artist_extras("unknown")
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn fanart_rejects_a_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_string(r#"{"status":"error","error message":"Invalid API key"}"#),
        )
        .mount(&server)
        .await;
    let e = fanart(&server, KEY, "en").validate().await.unwrap_err();
    assert!(matches!(e, MetaError::Auth));
    assert!(!e.to_string().contains(KEY));
}

#[tokio::test]
async fn providers_fill_each_other_in() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v3/music/{ARTIST}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(fixture("fanart_artist.json", &server.uri())),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/json/{KEY}/artist-mb.php")))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(fixture("tadb_artist.json", &server.uri())),
        )
        .mount(&server)
        .await;
    let mut a = fanart(&server, KEY, "en")
        .artist_extras(ARTIST)
        .await
        .unwrap()
        .unwrap();
    a.merge(
        tadb(&server, KEY)
            .artist_extras(ARTIST)
            .await
            .unwrap()
            .unwrap(),
    );
    let base = server.uri();
    // Fanart.tv's images first, TheAudioDB's biography.
    assert_eq!(
        a.image.as_deref(),
        Some(format!("{base}/img/fa/thumb-best.jpg").as_str())
    );
    assert_eq!(
        a.biography("en"),
        Some("Example Band are a band from the test fixtures.")
    );
}
