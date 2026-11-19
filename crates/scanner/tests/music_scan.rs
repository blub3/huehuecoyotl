//! Issue #7: a music folder is scanned into artists, albums and tracks from generated, tagged
//! fixtures: multi-disc albums, a compilation, several artists per track, an untagged album, a
//! broken file, moves, a second scan with no changes, and a video library next to it.
mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use bytes::Bytes;
use huehuecoyotl_library::Library;
use huehuecoyotl_scanner::music::{VARIOUS_ARTISTS, scan_music_source};
use huehuecoyotl_vfs::{Caps, Entry, LocalSource, Source, VPath};

/// Counts the bytes read, to prove that scans read tags, not files.
struct Counting {
    inner: LocalSource,
    read: Arc<AtomicU64>,
}

#[async_trait]
impl Source for Counting {
    async fn list(&self, dir: &VPath) -> huehuecoyotl_vfs::Result<Vec<Entry>> {
        self.inner.list(dir).await
    }
    async fn stat(&self, path: &VPath) -> huehuecoyotl_vfs::Result<Entry> {
        self.inner.stat(path).await
    }
    async fn read_at(
        &self,
        path: &VPath,
        offset: u64,
        len: u32,
    ) -> huehuecoyotl_vfs::Result<Bytes> {
        let b = self.inner.read_at(path, offset, len).await?;
        self.read.fetch_add(b.len() as u64, Ordering::Relaxed);
        Ok(b)
    }
    fn capabilities(&self) -> Caps {
        self.inner.capabilities()
    }
}

fn flac(dir: &std::path::Path, rel: &str, tags: &[(&str, &str)]) {
    common::tone(dir, rel, &["-c:a", "flac"], 44100, 0.5, tags, None);
}

async fn q<T: rusqlite::types::FromSql + Send + 'static>(
    lib: &Library,
    sql: &'static str,
) -> Vec<T> {
    lib.read(move |c| {
        let mut st = c.prepare(sql)?;
        let rows = st.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<T>>>()?)
    })
    .await
    .unwrap()
}

use huehuecoyotl_library::rusqlite;

#[tokio::test(flavor = "multi_thread")]
async fn a_music_folder_becomes_artists_albums_and_tracks() {
    if !common::ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("music");
    // A double album in CD1/CD2 folders.
    for (disc, n, title) in [
        (1, 1, "Side A One"),
        (1, 2, "Side A Two"),
        (2, 1, "Side C One"),
    ] {
        flac(
            &m,
            &format!("Band/Double (1999)/CD{disc}/0{n} - {title}.flac"),
            &[
                ("title", title),
                ("artist", "Band"),
                ("album_artist", "Band"),
                ("album", "Double"),
                ("disc", &format!("{disc}/2")),
                ("track", &n.to_string()),
                ("date", "1999"),
            ],
        );
    }
    // A compilation: no album artist, different track artists, one with two artists.
    flac(
        &m,
        "Compilations/Hits 2020/01.flac",
        &[
            ("title", "First Hit"),
            ("artist", "Singer A"),
            ("album", "Hits 2020"),
            ("track", "1"),
            ("compilation", "1"),
        ],
    );
    flac(
        &m,
        "Compilations/Hits 2020/02.flac",
        &[
            ("title", "Second Hit"),
            ("artist", "Singer B feat. Rapper C"),
            ("album", "Hits 2020"),
            ("track", "2"),
            ("compilation", "1"),
        ],
    );
    // An album identified by its MusicBrainz release id, split over two folders.
    for (folder, n) in [("Release/Part 1", 1), ("Release/Part 2", 2)] {
        flac(
            &m,
            &format!("{folder}/{n}.flac"),
            &[
                ("title", &format!("Part {n}")),
                ("artist", "Orchestra"),
                ("album", "Symphony"),
                ("track", &n.to_string()),
                (
                    "MUSICBRAINZ_ALBUMID",
                    "11111111-2222-3333-4444-555555555555",
                ),
            ],
        );
    }
    // Untagged files, named by their folders.
    common::tone(
        &m,
        "Quiet Artist/Silent Album (2003)/01 - Hush.wav",
        &["-c:a", "pcm_s16le"],
        44100,
        0.5,
        &[],
        None,
    );
    // A broken file never stops the scan.
    std::fs::create_dir_all(m.join("Broken")).unwrap();
    std::fs::write(m.join("Broken/03 - Damaged.mp3"), vec![0x42u8; 50_000]).unwrap();
    // A sidecar cover.
    common::png(&m.join("Band/Double (1999)/cover.jpg.png"), [10, 20, 30]);
    std::fs::rename(
        m.join("Band/Double (1999)/cover.jpg.png"),
        m.join("Band/Double (1999)/cover.png"),
    )
    .unwrap();
    // A video library in the same database.
    let v = d.path().join("video/Alien (1979)");
    std::fs::create_dir_all(&v).unwrap();
    std::fs::write(v.join("Alien.1979.mkv"), vec![1u8; 4096]).unwrap();

    let lib = Library::open(&d.path().join("library.db")).unwrap();
    let (mroot, vroot) = (
        m.to_string_lossy().to_string(),
        d.path().join("video").to_string_lossy().to_string(),
    );
    lib.write(move |c| {
        c.execute("INSERT INTO source(id, kind, name, root, content) VALUES (2, 'local', 'Music', ?1, 'music'), (3, 'local', 'Video', ?2, 'video')", [mroot, vroot])?;
        Ok(())
    })
    .await
    .unwrap();
    let video = huehuecoyotl_scanner::scan::scan_source(
        &lib,
        &LocalSource,
        3,
        &VPath::new(d.path().join("video").to_string_lossy()),
    )
    .await
    .unwrap();
    assert_eq!(video.added, 1);
    let movies_before: Vec<String> = q(
        &lib,
        "SELECT id || title || kind FROM item WHERE kind = 'movie'",
    )
    .await;

    let read = Arc::new(AtomicU64::new(0));
    let src = Counting {
        inner: LocalSource,
        read: read.clone(),
    };
    let root = VPath::new(m.to_string_lossy());
    let r = scan_music_source(&lib, &src, 2, &root).await.unwrap();
    assert_eq!(r.files_seen, 9);
    assert_eq!(r.added, 9);
    assert_eq!(r.unreadable, 1);

    // Albums and their artists.
    let albums: Vec<String> = q(&lib, "SELECT a.title || ' / ' || p.title FROM item a JOIN item p ON p.id = a.parent_id WHERE a.kind = 'album' ORDER BY a.title").await;
    assert_eq!(
        albums,
        [
            // The broken file's album is named after its folder; it has no artist.
            "Broken / Unknown artist".to_string(),
            "Double / Band".into(),
            format!("Hits 2020 / {VARIOUS_ARTISTS}"),
            "Silent Album / Quiet Artist".into(),
            "Symphony / Orchestra".into(),
        ]
    );
    // The double album: three tracks on two discs, in order.
    let double: Vec<String> = q(&lib, "SELECT m.disc_no || '-' || m.track_no || ' ' || t.title FROM item t JOIN music m ON m.item_id = t.id JOIN item a ON a.id = t.parent_id WHERE a.title = 'Double' ORDER BY m.disc_no, m.track_no").await;
    assert_eq!(
        double,
        ["1-1 Side A One", "1-2 Side A Two", "2-1 Side C One"]
    );
    // Two artists on one track; the second has its own artist item that lists the track.
    let credits: Vec<String> = q(&lib, "SELECT a.title || '|' || c.join_phrase FROM item t JOIN credit c ON c.item_id = t.id AND c.role = 'artist' JOIN item a ON a.id = c.artist_id WHERE t.title = 'Second Hit' ORDER BY c.position").await;
    assert_eq!(credits, ["Singer B| feat. ", "Rapper C|"]);
    let compilation: Vec<i64> = q(&lib, "SELECT m.compilation FROM item a JOIN music m ON m.item_id = a.id WHERE a.title = 'Hits 2020'").await;
    assert_eq!(compilation, [1]);
    // One album over two folders by its release id, with the id kept.
    let sym: Vec<i64> = q(
        &lib,
        "SELECT count(*) FROM item t JOIN item a ON a.id = t.parent_id WHERE a.title = 'Symphony'",
    )
    .await;
    assert_eq!(sym, [2]);
    let ids: Vec<String> = q(&lib, "SELECT value FROM external_id WHERE kind = 'release'").await;
    assert_eq!(ids, ["11111111-2222-3333-4444-555555555555"]);
    // The untagged file and the broken one have readable names.
    let titles: Vec<String> = q(&lib, "SELECT title FROM item WHERE kind = 'track' AND title IN ('Hush', 'Damaged')  ORDER BY title").await;
    assert_eq!(titles, ["Damaged", "Hush"]);
    // Audio properties.
    let audio: Vec<String> = q(&lib, "SELECT codec || ' ' || sample_rate || ' ' || lossless FROM file_audio a JOIN file f ON f.id = a.file_id WHERE f.path LIKE '%Hush.wav'").await;
    assert_eq!(audio, ["pcm 44100 1"]);
    // Covers: the sidecar for the double album.
    assert!(r.covers.iter().any(|(_, c)| matches!(c, huehuecoyotl_scanner::music::Cover::Sidecar(p) if p.as_str().ends_with("cover.png"))));
    // The video library is untouched.
    let movies_after: Vec<String> = q(
        &lib,
        "SELECT id || title || kind FROM item WHERE kind = 'movie'",
    )
    .await;
    assert_eq!(movies_before, movies_after);

    // A second scan with no changes reads nothing.
    read.store(0, Ordering::Relaxed);
    let again = scan_music_source(&lib, &src, 2, &root).await.unwrap();
    assert_eq!((again.added, again.changed, again.removed), (0, 0, 0));
    assert_eq!(read.load(Ordering::Relaxed), 0);

    // A moved file keeps its track and play count.
    let track: Vec<i64> = q(&lib, "SELECT id FROM item WHERE title = 'Side C One'").await;
    let t = track[0];
    lib.write(move |c| Ok(c.execute("INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at, play_count) VALUES (?1, 1, 0, 0, 0, 1, 5)", [t])?)).await.unwrap();
    std::fs::rename(
        m.join("Band/Double (1999)/CD2/01 - Side C One.flac"),
        m.join("Band/Double (1999)/CD2/01 Side C One (renamed).flac"),
    )
    .unwrap();
    let moved = scan_music_source(&lib, &src, 2, &root).await.unwrap();
    assert_eq!((moved.moved, moved.removed), (1, 0));
    let counts: Vec<i64> = q(&lib, "SELECT p.play_count FROM playstate p JOIN item i ON i.id = p.item_id WHERE i.title = 'Side C One'").await;
    assert_eq!(counts, [5]);

    // Removed files take their tracks, emptied albums and artists along.
    std::fs::remove_dir_all(m.join("Release")).unwrap();
    let gone = scan_music_source(&lib, &src, 2, &root).await.unwrap();
    assert_eq!(gone.removed, 2);
    let left: Vec<i64> = q(
        &lib,
        "SELECT count(*) FROM item WHERE title IN ('Symphony', 'Orchestra')",
    )
    .await;
    assert_eq!(left, [0]);
    assert_eq!(
        movies_before,
        q::<String>(
            &lib,
            "SELECT id || title || kind FROM item WHERE kind = 'movie'"
        )
        .await
    );
}

/// Issue #146: an album tagged "Rosalia" without an artist ID and one tagged "Rosalía" with
/// Rosalía's ID become one artist; another "Nirvana" with a different ID stays apart; a rescan
/// keeps it so.
#[tokio::test(flavor = "multi_thread")]
async fn artists_join_by_id_and_name() {
    if !common::ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("music");
    let rosalia = "25f3abd9-63b5-471a-bd25-feb9672dfa11";
    flac(
        &m,
        "Rosalia/LUX/01.flac",
        &[
            ("title", "One"),
            ("artist", "Rosalia"),
            ("album", "LUX"),
            ("track", "1"),
        ],
    );
    flac(
        &m,
        "Rosalía/Lux/01.flac",
        &[
            ("title", "Two"),
            ("artist", "Rosalía"),
            ("album", "Lux"),
            ("track", "1"),
            ("MUSICBRAINZ_ARTISTID", rosalia),
        ],
    );
    flac(
        &m,
        "Nirvana A/Album/01.flac",
        &[
            ("title", "Three"),
            ("artist", "Nirvana"),
            ("album", "A"),
            ("track", "1"),
            (
                "MUSICBRAINZ_ARTISTID",
                "5b11f4ce-a62d-471e-81fc-a69a8278c7da",
            ),
        ],
    );
    flac(
        &m,
        "Nirvana B/Album/01.flac",
        &[
            ("title", "Four"),
            ("artist", "Nirvana"),
            ("album", "B"),
            ("track", "1"),
            (
                "MUSICBRAINZ_ARTISTID",
                "c7b7e0e5-0000-4000-8000-000000000001",
            ),
        ],
    );
    let lib = Library::open(&d.path().join("library.db")).unwrap();
    let root_s = m.to_string_lossy().to_string();
    lib.write(move |c| {
        c.execute("INSERT INTO source(id, kind, name, root, content) VALUES (2, 'local', 'Music', ?1, 'music')", [root_s])?;
        Ok(())
    })
    .await
    .unwrap();
    let root = VPath::new(m.to_string_lossy());
    scan_music_source(&lib, &LocalSource, 2, &root)
        .await
        .unwrap();
    lib.write(|c| huehuecoyotl_library::artists::reconcile(c).map(|_| ()))
        .await
        .unwrap();
    let names: Vec<String> = q(
        &lib,
        "SELECT title FROM item WHERE kind = 'artist' ORDER BY title",
    )
    .await;
    assert_eq!(
        names.iter().filter(|n| n.starts_with("Ros")).count(),
        1,
        "{names:?}"
    );
    assert_eq!(
        names.iter().filter(|n| *n == "Nirvana").count(),
        2,
        "{names:?}"
    );
    // A rescan finds the joined artist again.
    scan_music_source(&lib, &LocalSource, 2, &root)
        .await
        .unwrap();
    let again: Vec<String> = q(
        &lib,
        "SELECT title FROM item WHERE kind = 'artist' ORDER BY title",
    )
    .await;
    assert_eq!(again, names);
}
