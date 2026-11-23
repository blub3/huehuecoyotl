//! Issue #154: Add to Library through the public core API, on real files made with ffmpeg: an
//! album lands under its name with tags written and the original untouched; importing it again
//! copies nothing; an unmatched album waits for review; a foreign file at the destination is not
//! overwritten; a movie gets its `.nfo`; a book stays one book. With the test servers
//! (`COYOTL_TEST_SERVERS=1`): an import from SMB continues after a dropped connection.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};
use huehuecoyotl_core::{Core, Event};
use huehuecoyotl_library::rusqlite::Connection;
use huehuecoyotl_vfs::{LocalSource, VPath};
use serde_json::{Value, json};

/// One release per album title: one release ID is one album (issue #147).
fn release_of(album: &str) -> String {
    let n = album
        .bytes()
        .fold(7u64, |h, b| h.wrapping_mul(131).wrapping_add(u64::from(b)));
    format!("aaaaaaaa-0000-4000-8000-{:012x}", n & 0xffff_ffff_ffff)
}

fn ffmpeg(args: &[&str]) -> bool {
    Command::new("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-y"])
        .args(args)
        .status()
        .is_ok_and(|s| s.success())
}

fn track(dir: &Path, n: u32, album: &str, ids: bool) {
    std::fs::create_dir_all(dir).unwrap();
    let (t, tr, al) = (
        format!("title=Song {n}"),
        format!("track={n}"),
        format!("album={album}"),
    );
    let rel = format!("MUSICBRAINZ_ALBUMID={}", release_of(album));
    let p = dir.join(format!("0{n}.flac"));
    let src = format!("sine=frequency={}:duration=1", 200 + n * 40);
    let mut a = vec![
        "-f",
        "lavfi",
        "-i",
        &src,
        "-c:a",
        "flac",
        "-metadata",
        &t,
        "-metadata",
        &tr,
        "-metadata",
        &al,
        "-metadata",
        "artist=Test Band",
        "-metadata",
        "album_artist=Test Band",
        "-metadata",
        "date=2001",
    ];
    if ids {
        a.extend(["-metadata", &rel]);
    }
    let ps = p.to_string_lossy().to_string();
    a.push(&ps);
    assert!(ffmpeg(&a), "ffmpeg");
}

struct App {
    core: Core,
    rx: Receiver<Event>,
    data: PathBuf,
    next: std::cell::Cell<u64>,
}

impl App {
    fn new(data: &Path) -> Self {
        // SAFETY: set before any thread of this test process reads the environment.
        unsafe { std::env::set_var("COYOTL_DISABLE_KEYSTORE", "1") };
        let (tx, rx) = unbounded();
        let core = Core::open(data, "en-US", move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        core.set_music_online(1, false);
        App {
            core,
            rx,
            data: data.to_path_buf(),
            next: std::cell::Cell::new(100),
        }
    }
    fn id(&self) -> u64 {
        let i = self.next.get();
        self.next.set(i + 1);
        i
    }
    fn answer(&self, id: u64) -> Result<Value, String> {
        let deadline = Instant::now() + Duration::from_secs(60);
        while let Ok(e) = self
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            match e {
                Event::MusicDetail {
                    request_id, json, ..
                } if request_id == id => {
                    return Ok(serde_json::from_str(&json).unwrap());
                }
                Event::Error {
                    request_id,
                    message,
                    ..
                } if request_id == id => return Err(message),
                _ => {}
            }
        }
        panic!("no answer");
    }
    fn call(&self, action: &str, args: Value) -> Result<Value, String> {
        let id = self.id();
        self.core.library_call(id, action, &args.to_string());
        self.answer(id)
    }
    fn db(&self) -> Connection {
        Connection::open(self.data.join("library.db")).unwrap()
    }
    fn library(&self, path: &Path) {
        self.call(
            "apply",
            json!({ "where": "local", "path": path.to_string_lossy(), "mode": "move" }),
        )
        .unwrap();
    }
    fn add(&self, path: &Path, content: &str, files: i64) -> i64 {
        self.core.add_library(90, &path.to_string_lossy(), content);
        let root = path.to_string_lossy().to_string();
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            let c = self.db();
            let got: Option<(i64, i64)> = c
                .query_row(
                    "SELECT s.id, (SELECT count(*) FROM file f WHERE f.source_id = s.id AND f.item_id IS NOT NULL) FROM source s WHERE s.root = ?1",
                    [&root],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok();
            if let Some((s, n)) = got
                && n >= files
            {
                return s;
            }
            assert!(Instant::now() < deadline, "scan of {root}");
            std::thread::sleep(Duration::from_millis(80));
        }
    }
    fn item(&self, kind: &str, title: &str) -> i64 {
        self.db()
            .query_row(
                "SELECT id FROM item WHERE kind = ?1 AND title = ?2",
                [kind, title],
                |r| r.get(0),
            )
            .unwrap()
    }
    fn finish(&self, secs: u64) -> Value {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let l = self.call("imports", json!({})).unwrap();
            if l["active"] == 0 {
                return l;
            }
            assert!(Instant::now() < deadline, "{l}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    fn job(&self, l: &Value, title: &str) -> Value {
        l["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["title"] == title)
            .cloned()
            .unwrap_or_else(|| panic!("{l}"))
    }
}

async fn tags(path: &Path) -> huehuecoyotl_scanner::tags::TrackTags {
    let size = std::fs::metadata(path).unwrap().len();
    huehuecoyotl_scanner::tags::read(&LocalSource, &VPath::new(path.to_string_lossy()), size)
        .await
        .unwrap()
        .tags
}

#[test]
fn an_album_is_copied_named_and_tagged() {
    let d = tempfile::tempdir().unwrap();
    let (music, lib) = (d.path().join("music"), d.path().join("lib"));
    std::fs::create_dir_all(&lib).unwrap();
    track(&music.join("in"), 1, "Sample", true);
    track(&music.join("in"), 2, "Sample", true);
    let before = std::fs::read(music.join("in/01.flac")).unwrap();
    let a = App::new(&d.path().join("data"));
    a.library(&lib);
    a.add(&music, "music", 2);
    let album = a.item("album", "Sample");

    let sheet = a.call("importPlan", json!({ "item": album })).unwrap();
    assert_eq!(sheet["files"], 2, "{sheet}");
    assert_eq!(sheet["review"], 0);
    assert_eq!(sheet["plans"][0]["names"][0], "01 - Song 1.flac");
    assert!(sheet["from"].as_str().unwrap().ends_with("/in"), "{sheet}");
    assert!(
        sheet["tags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["field"] == "album")
    );

    a.call("importStart", json!({ "item": album })).unwrap();
    let l = a.finish(30);
    assert_eq!(a.job(&l, "Sample")["state"], "done", "{l}");
    let copy = lib.join("Music/Test Band/Sample (2001)/01 - Song 1.flac");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let t = rt.block_on(tags(&copy));
    assert_eq!(
        (t.title.as_deref(), t.album.as_deref()),
        (Some("Song 1"), Some("Sample"))
    );
    assert_eq!(t.mb_release, Some(release_of("Sample")));
    assert_eq!(
        std::fs::read(music.join("in/01.flac")).unwrap(),
        before,
        "the original stays"
    );
    assert!(
        walk(&lib)
            .iter()
            .all(|p| !p.file_name().unwrap().to_string_lossy().starts_with('.'))
    );
    assert_eq!(
        std::fs::read_dir(a.data.join("imports")).unwrap().count(),
        0,
        "no leftovers"
    );

    // One album, with the copy as a second version of each track.
    let c = a.db();
    let n: i64 = c
        .query_row("SELECT count(*) FROM item WHERE kind = 'album'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(n, 1);
    let v: i64 = c
        .query_row(
            "SELECT count(*) FROM album_version WHERE album_id = ?1",
            [album],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(v, 2);

    // Again: nothing is copied, also after a rescan of the library.
    let sheet = a.call("importPlan", json!({ "item": album })).unwrap();
    assert_eq!(sheet["files"], 0, "{sheet}");
    let files: i64 = c
        .query_row("SELECT count(*) FROM file", [], |r| r.get(0))
        .unwrap();
    a.call("importStart", json!({ "item": album })).unwrap();
    let l = a.finish(10);
    assert_eq!(l["jobs"][0]["note"], "already", "{l}");
    a.core.rescan(0);
    std::thread::sleep(Duration::from_secs(2));
    let again: i64 = c
        .query_row("SELECT count(*) FROM file", [], |r| r.get(0))
        .unwrap();
    assert_eq!(again, files);
    let n: i64 = c
        .query_row("SELECT count(*) FROM item WHERE kind = 'album'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(n, 1, "the rescan makes no second album");

    // The original goes; the album stays and plays from the copy.
    std::fs::remove_dir_all(music.join("in")).unwrap();
    track(&music.join("other"), 1, "Other", false);
    let s: i64 = c
        .query_row(
            "SELECT id FROM source WHERE root = ?1",
            [music.to_string_lossy()],
            |r| r.get(0),
        )
        .unwrap();
    a.core.rescan(s);
    let deadline = Instant::now() + Duration::from_secs(20);
    while a.call("itemOrigin", json!({ "item": album })).unwrap()["libraryOnly"] != true {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn unsure_and_clashing_imports_wait() {
    let d = tempfile::tempdir().unwrap();
    let (music, lib) = (d.path().join("music"), d.path().join("lib"));
    std::fs::create_dir_all(&lib).unwrap();
    track(&music.join("u"), 1, "Unsure", false);
    track(&music.join("c"), 1, "Clash", true);
    let a = App::new(&d.path().join("data"));
    a.library(&lib);
    a.add(&music, "music", 2);

    // No match and no release ID: it waits.
    let unsure = a.item("album", "Unsure");
    a.call("importStart", json!({ "item": unsure })).unwrap();
    let l = a.call("imports", json!({})).unwrap();
    let j = a.job(&l, "Unsure");
    assert_eq!(
        (j["state"].as_str(), j["review"].as_str()),
        (Some("review"), Some("match"))
    );
    assert_eq!(l["waiting"], 1);
    assert!(!lib.join("Music/Test Band/Unsure (2001)").exists());

    // Someone else's file at the destination is not overwritten.
    let dir = lib.join("Music/Test Band/Clash (2001)");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("01 - Song 1.flac"), b"mine").unwrap();
    let clash = a.item("album", "Clash");
    a.call("importStart", json!({ "item": clash })).unwrap();
    let l = a.finish(20);
    let j = a.job(&l, "Clash");
    assert_eq!(
        (j["state"].as_str(), j["review"].as_str()),
        (Some("review"), Some("clash")),
        "{l}"
    );
    assert_eq!(
        std::fs::read(dir.join("01 - Song 1.flac")).unwrap(),
        b"mine"
    );
}

#[test]
fn a_movie_gets_its_nfo_and_a_book_stays_one_book() {
    let d = tempfile::tempdir().unwrap();
    let (films, books, lib) = (
        d.path().join("films"),
        d.path().join("books"),
        d.path().join("lib"),
    );
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::create_dir_all(films.join("Clip (2020)")).unwrap();
    let movie = films.join("Clip (2020)/Clip (2020).mkv");
    assert!(ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x180:rate=10:duration=1",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        movie.to_str().unwrap()
    ]));
    let dir = books.join("Jane Austen/Emma");
    std::fs::create_dir_all(&dir).unwrap();
    let meta = d.path().join("ch.txt");
    std::fs::write(&meta, ";FFMETADATA1\ntitle=Emma\nartist=Jane Austen\nalbum=Emma\ndate=1815\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1000\ntitle=One\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=1000\nEND=2000\ntitle=Two\n").unwrap();
    let book = dir.join("Emma.m4b");
    assert!(ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "sine=duration=2",
        "-i",
        meta.to_str().unwrap(),
        "-map_metadata",
        "1",
        "-map_chapters",
        "1",
        "-c:a",
        "aac",
        "-f",
        "ipod",
        book.to_str().unwrap()
    ]));

    let a = App::new(&d.path().join("data"));
    a.library(&lib);
    a.add(&films, "video", 1);
    let m = a.item("movie", "Clip");
    // Without TMDB it is not matched: the user says "as it is" (issue #155); here, the same.
    a.db()
        .execute(
            "UPDATE item SET match_state = 'matched', tmdb_id = 42, imdb_id = 'tt42' WHERE id = ?1",
            [m],
        )
        .unwrap();
    a.call("importStart", json!({ "item": m })).unwrap();
    let l = a.finish(30);
    assert_eq!(a.job(&l, "Clip")["state"], "done", "{l}");
    let folder = lib.join("Movies/Clip (2020) {imdb-tt42}");
    let names: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let nfo = names.iter().find(|n| n.ends_with(".nfo")).expect("an .nfo");
    let text = std::fs::read_to_string(folder.join(nfo)).unwrap();
    assert!(text.contains(">42<") && text.contains(">tt42<"), "{text}");
    assert!(names.iter().any(|n| n.ends_with(".mkv")));

    a.add(&books, "audiobooks", 1);
    let b = a.item("audiobook", "Emma");
    a.db()
        .execute("UPDATE item SET match_state = 'matched' WHERE id = ?1", [b])
        .unwrap();
    a.call("importStart", json!({ "item": b })).unwrap();
    let l = a.finish(30);
    assert_eq!(a.job(&l, "Emma")["state"], "done", "{l}");
    let copy = lib.join("Audiobooks/Jane Austen/Emma (1815)/Emma.m4b");
    assert!(copy.is_file());
    let o = Command::new("ffprobe")
        .args(["-v", "error", "-show_chapters", "-of", "csv=p=0"])
        .arg(&copy)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&o.stdout).lines().count(),
        2,
        "chapters"
    );
    a.core.rescan(0);
    std::thread::sleep(Duration::from_secs(3));
    let n: i64 = a
        .db()
        .query_row(
            "SELECT count(*) FROM item WHERE kind = 'audiobook'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1, "one book after scanning both places");
    let on: i64 = a.db().query_row(
        "SELECT s.managed IS NOT NULL FROM book_file b JOIN file f ON f.id = b.file_id JOIN source s ON s.id = f.source_id WHERE b.book_id = ?1",
        [b],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(on, 1, "the book plays from the library");
}

/// A second copy of one release in another quality, for the review tests.
fn quality_copy(dir: &Path, album: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let (tr, al) = ("track=1".to_string(), format!("album={album}"));
    let rel = format!("MUSICBRAINZ_ALBUMID={}", release_of(album));
    let p = dir.join("01.flac");
    assert!(ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=240:duration=1:sample_rate=96000",
        "-c:a",
        "flac",
        "-sample_fmt",
        "s32",
        "-bits_per_raw_sample",
        "24",
        "-metadata",
        "title=Song 1",
        "-metadata",
        &tr,
        "-metadata",
        &al,
        "-metadata",
        "artist=Test Band",
        "-metadata",
        "album_artist=Test Band",
        "-metadata",
        "date=2001",
        "-metadata",
        &rel,
        p.to_str().unwrap(),
    ]));
}

#[test]
fn another_quality_is_the_users_call() {
    let d = tempfile::tempdir().unwrap();
    let (music, lib) = (d.path().join("music"), d.path().join("lib"));
    std::fs::create_dir_all(&lib).unwrap();
    track(&music.join("cd"), 1, "Same", true);
    let a = App::new(&d.path().join("data"));
    a.library(&lib);
    let source = a.add(&music, "music", 1);
    let album = a.item("album", "Same");
    a.call("importStart", json!({ "item": album })).unwrap();
    a.finish(20);
    let plain = lib.join("Music/Test Band/Same (2001)/01 - Song 1.flac");
    assert!(plain.is_file());
    let first = std::fs::read(&plain).unwrap();
    let track_id: i64 = a
        .db()
        .query_row("SELECT id FROM item WHERE kind = 'track'", [], |r| r.get(0))
        .unwrap();
    a.db().execute("INSERT INTO playstate(item_id, profile_id, updated_at, play_count) VALUES (?1, 1, 1, 7)", [track_id]).unwrap();

    // The same release in 24/96 appears; the CD files leave the source.
    quality_copy(&music.join("hires"), "Same");
    std::fs::remove_dir_all(music.join("cd")).unwrap();
    a.core.rescan(source);
    let deadline = Instant::now() + Duration::from_secs(20);
    while a.db().query_row("SELECT count(*) FROM file f JOIN file_audio x ON x.file_id = f.id WHERE x.sample_rate = 96000", [], |r| r.get::<_, i64>(0)).unwrap() == 0 {
        assert!(Instant::now() < deadline, "scan");
        std::thread::sleep(Duration::from_millis(80));
    }
    a.call("importStart", json!({ "item": album })).unwrap();
    let l = a.call("imports", json!({})).unwrap();
    let j = l["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["state"] == "review")
        .cloned()
        .unwrap_or_else(|| panic!("{l}"));
    assert_eq!(j["review"], "clash", "{l}");
    let job = j["id"].as_i64().unwrap();
    let r = a.call("importReview", json!({ "job": job })).unwrap();
    assert_eq!(r["library"]["files"], 1, "{r}");
    assert!(r["format"].as_str().unwrap().contains("96"), "{r}");
    assert_eq!(r["label"], "24-96", "{r}");

    // Another version: its own folder, the first copy untouched.
    a.call("importResolve", json!({ "job": job, "choice": "version" }))
        .unwrap();
    let l = a.finish(20);
    assert_eq!(
        l["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == job)
            .unwrap()["state"],
        "done",
        "{l}"
    );
    let second = lib.join("Music/Test Band/Same (2001) [24-96]/01 - Song 1.flac");
    assert!(second.is_file());
    assert_eq!(std::fs::read(&plain).unwrap(), first);
    let versions: i64 = a.db().query_row("SELECT count(*) FROM album_version v WHERE album_id = ?1 AND EXISTS (SELECT 1 FROM file f WHERE f.version_id = v.id)", [album], |r| r.get(0)).unwrap();
    assert!(versions >= 2, "{versions}");

    // Replace: the old copies move aside, play counts stay.
    let job2 = {
        a.db()
            .execute(
                "UPDATE import_job SET state = 'review', review = 'clash' WHERE id = ?1",
                [job],
            )
            .unwrap();
        a.db().execute("UPDATE import_file SET state = 'pending', dest = 'Test Band/Same (2001)/01 - Song 1.flac' WHERE job_id = ?1", [job]).unwrap();
        job
    };
    a.call("importResolve", json!({ "job": job2, "choice": "replace" }))
        .unwrap();
    a.finish(20);
    assert!(
        walk(&lib.join("Music/.trash")).len() >= 2,
        "the old files are kept in the trash"
    );
    let new = std::fs::read(&plain).unwrap();
    assert_ne!(new, first);
    let plays: i64 = a
        .db()
        .query_row(
            "SELECT play_count FROM playstate WHERE item_id = ?1",
            [track_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(plays, 7);
}

#[test]
fn keep_and_as_is_answers() {
    let d = tempfile::tempdir().unwrap();
    let (music, lib) = (d.path().join("music"), d.path().join("lib"));
    std::fs::create_dir_all(&lib).unwrap();
    track(&music.join("u"), 1, "Mine", false);
    track(&music.join("k"), 1, "Kept", true);
    let a = App::new(&d.path().join("data"));
    a.library(&lib);
    a.add(&music, "music", 2);
    let mine = a.item("album", "Mine");
    a.call("importStart", json!({ "item": mine })).unwrap();
    let l = a.call("imports", json!({})).unwrap();
    let job = a.job(&l, "Mine")["id"].as_i64().unwrap();
    assert_eq!(
        a.call("importReview", json!({ "job": job })).unwrap()["review"],
        "match"
    );
    a.call("importResolve", json!({ "job": job, "choice": "asIs" }))
        .unwrap();
    let l = a.finish(20);
    assert_eq!(a.job(&l, "Mine")["state"], "done");
    assert!(
        lib.join("Music/Test Band/Mine (2001)/01 - Song 1.flac")
            .is_file()
    );

    // A foreign folder: keeping changes nothing; another name imports next to it.
    let dir = lib.join("Music/Test Band/Kept (2001)");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("01 - Song 1.flac"), b"mine").unwrap();
    let kept = a.item("album", "Kept");
    a.call("importStart", json!({ "item": kept })).unwrap();
    let l = a.finish(20);
    let job = a.job(&l, "Kept")["id"].as_i64().unwrap();
    assert_eq!(a.job(&l, "Kept")["review"], "clash");
    assert!(a.call("importReview", json!({ "job": job })).unwrap()["library"].is_null());
    a.call(
        "importResolve",
        json!({ "job": job, "choice": "rename", "label": "Second" }),
    )
    .unwrap();
    a.finish(20);
    assert_eq!(
        std::fs::read(dir.join("01 - Song 1.flac")).unwrap(),
        b"mine"
    );
    assert!(
        lib.join("Music/Test Band/Kept (2001) [Second]/01 - Song 1.flac")
            .is_file()
    );
}

fn relay(rt: &tokio::runtime::Runtime, target: &'static str, down: Arc<AtomicBool>) -> u16 {
    let l = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let port = l.local_addr().unwrap().port();
    rt.spawn(async move {
        loop {
            let Ok((mut a, _)) = l.accept().await else {
                return;
            };
            if down.load(Ordering::SeqCst) {
                continue;
            }
            let down = down.clone();
            tokio::spawn(async move {
                let Ok(mut b) = tokio::net::TcpStream::connect(target).await else {
                    return;
                };
                let copy = tokio::io::copy_bidirectional(&mut a, &mut b);
                let cut = async {
                    while !down.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                };
                tokio::select! { _ = copy => {} _ = cut => {} }
            });
        }
    });
    port
}

#[test]
fn an_import_from_smb_survives_a_dropped_connection() {
    if std::env::var_os("COYOTL_TEST_SERVERS").is_none() {
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let down = Arc::new(AtomicBool::new(false));
    let port = relay(&rt, "172.29.0.10:445", down.clone());
    let writable = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/servers/writable");
    let name = format!("t-imp-{}", std::process::id());
    let root = writable.join(&name);
    for n in 1..=4 {
        track(&root.join("Band"), n, "Remote", true);
    }
    {
        use std::os::unix::fs::PermissionsExt;
        for p in [root.clone(), root.join("Band")] {
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o777)).unwrap();
        }
    }
    let d = tempfile::tempdir().unwrap();
    let lib = d.path().join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    let a = App::new(&d.path().join("data"));
    a.library(&lib);
    let spec = format!(
        r#"{{"protocol":"smb","host":"127.0.0.1","port":{port},"share":"write","user":"coyotl"}}"#
    );
    let id = a.id();
    a.core.share_create(
        id,
        &spec,
        "coyotl-test",
        &json!([{ "path": format!("/{name}"), "content": "music" }]).to_string(),
    );
    a.answer(id).unwrap();
    let deadline = Instant::now() + Duration::from_secs(40);
    while a
        .db()
        .query_row("SELECT count(*) FROM item WHERE kind = 'track'", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
        < 4
    {
        assert!(Instant::now() < deadline, "scan");
        std::thread::sleep(Duration::from_millis(100));
    }
    let album = a.item("album", "Remote");
    a.call("importStart", json!({ "item": album })).unwrap();
    // Cut as soon as the first file is in.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let l = a.call("imports", json!({})).unwrap();
        if a.job(&l, "Remote")["filesDone"].as_i64().unwrap() >= 1 || Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    down.store(true, Ordering::SeqCst);
    std::thread::sleep(Duration::from_secs(2));
    down.store(false, Ordering::SeqCst);
    let l = a.finish(90);
    assert_eq!(a.job(&l, "Remote")["state"], "done", "{l}");
    for n in 1..=4 {
        assert!(
            lib.join(format!(
                "Music/Test Band/Remote (2001)/0{n} - Song {n}.flac"
            ))
            .is_file(),
            "{n}"
        );
    }
    drop(a);
    let _ = std::fs::remove_dir_all(&root);
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn watched_folders_import_what_is_new() {
    let d = tempfile::tempdir().unwrap();
    let (watched, other, lib) = (
        d.path().join("watched"),
        d.path().join("other"),
        d.path().join("lib"),
    );
    std::fs::create_dir_all(&lib).unwrap();
    track(&watched.join("old"), 1, "Before", true);
    track(&other.join("x"), 1, "Elsewhere", true);
    let a = App::new(&d.path().join("data"));
    a.library(&lib);
    let w = a.add(&watched, "music", 1);
    let o = a.add(&other, "music", 1);
    a.call(
        "setType",
        json!({ "type": "music", "changes": { "import": "auto", "watch": [w] } }),
    )
    .unwrap();
    let files = |n: &str| lib.join(format!("Music/Test Band/{n} (2001)/01 - Song 1.flac"));
    std::thread::sleep(Duration::from_millis(20));

    // A new album in the watched folder is imported after the scan; one elsewhere is not.
    track(&watched.join("new"), 1, "Fresh", true);
    track(&other.join("y"), 1, "Outside", true);
    a.core.rescan(o);
    a.core.rescan(w);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !files("Fresh").is_file() {
        assert!(
            Instant::now() < deadline,
            "{}",
            a.call("imports", json!({})).unwrap()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let l = a.finish(20);
    assert_eq!(a.job(&l, "Fresh")["automatic"], true);
    assert!(!files("Before").exists(), "what was there before stays");
    assert!(!files("Outside").exists(), "another folder is not watched");

    // Without a match it waits for the user.
    track(&watched.join("unsure"), 1, "Unsure", false);
    a.core.rescan(w);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let l = a.call("imports", json!({})).unwrap();
        if l["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|j| j["title"] == "Unsure")
        {
            assert_eq!(a.job(&l, "Unsure")["state"], "review", "{l}");
            break;
        }
        assert!(Instant::now() < deadline, "{l}");
        std::thread::sleep(Duration::from_millis(100));
    }

    // Turned off: nothing more.
    a.call(
        "setType",
        json!({ "type": "music", "changes": { "import": "manual" } }),
    )
    .unwrap();
    track(&watched.join("later"), 1, "Later", true);
    a.core.rescan(w);
    let deadline = Instant::now() + Duration::from_secs(20);
    while a
        .db()
        .query_row("SELECT count(*) FROM item WHERE title = 'Later'", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
        == 0
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_secs(1));
    assert!(!files("Later").exists());
    assert!(
        watched.join("new/01.flac").is_file(),
        "nothing leaves the source"
    );
}
