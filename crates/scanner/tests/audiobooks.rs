//! Issue #65: audiobook folders scanned into books from generated fixtures (tones and silence
//! with tags; no recordings committed): an M4B with chapters, a 12-file MP3 book, a two-disc book,
//! a series of three books with OPF files, an untagged book in `Author/Title`, a file with broken
//! tags, a DRM file, a 40-file book read without reading whole files, and a rescan.
mod common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use bytes::Bytes;
use huehuecoyotl_library::{Library, rusqlite};
use huehuecoyotl_scanner::audiobooks::scan_audiobook_source;
use huehuecoyotl_vfs::{Caps, Entry, LocalSource, Source, VPath};

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

fn mp3(dir: &Path, rel: &str, seconds: f32, tags: &[(&str, &str)]) {
    common::tone(
        dir,
        rel,
        &["-c:a", "libmp3lame", "-b:a", "64k"],
        22_050,
        seconds,
        tags,
        None,
    );
}

/// An M4B with three chapters (FFmpeg's metadata format).
fn m4b(dir: &Path, rel: &str) {
    let out = dir.join(rel);
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    let meta = dir.join("chapters.txt");
    std::fs::write(
        &meta,
        ";FFMETADATA1\ntitle=Single Book\nartist=Mono Author\ncomposer=Mono Reader\n\
         [CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1000\ntitle=Opening\n\
         [CHAPTER]\nTIMEBASE=1/1000\nSTART=1000\nEND=2000\ntitle=Middle\n\
         [CHAPTER]\nTIMEBASE=1/1000\nSTART=2000\nEND=3000\ntitle=Ending\n",
    )
    .unwrap();
    let ok = Command::new("ffmpeg")
        .args([
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=duration=3",
        ])
        .arg("-i")
        .arg(&meta)
        .args([
            "-map",
            "0:a",
            "-map_metadata",
            "1",
            "-map_chapters",
            "1",
            "-c:a",
            "aac",
            "-f",
            "mp4",
        ])
        .arg(&out)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    std::fs::remove_file(meta).unwrap();
}

fn opf(dir: &Path, rel: &str, title: &str, index: &str) {
    let out = dir.join(rel);
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    std::fs::write(
        out,
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
<dc:title>{title}</dc:title><dc:creator opf:role="aut">Saga Writer</dc:creator><dc:creator opf:role="nrt">Saga Voice</dc:creator>
<dc:identifier opf:scheme="ISBN">978000000000{index}</dc:identifier>
<meta name="calibre:series" content="The Saga"/><meta name="calibre:series_index" content="{index}"/>
</metadata></package>"#
        ),
    )
    .unwrap();
}

async fn rows(lib: &Library, sql: &'static str) -> Vec<String> {
    lib.read(move |c| {
        let mut st = c.prepare(sql)?;
        let r = st.query_map([], |r| r.get(0))?;
        Ok(r.collect::<rusqlite::Result<Vec<String>>>()?)
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn audiobook_folders_become_books() {
    if !common::ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let b = d.path().join("books");
    m4b(&b, "Mono Author/Single Book.m4b");
    for n in 1..=12 {
        mp3(
            &b,
            &format!("Twelve Author/Twelve Parts/{n:02}.mp3"),
            0.4,
            &[
                ("album", "Twelve Parts"),
                ("artist", "Twelve Author"),
                ("composer", "Twelve Reader"),
                ("title", &format!("Part {n}")),
                ("track", &n.to_string()),
            ],
        );
    }
    for (disc, n) in [(1, 1), (1, 2), (2, 1), (2, 2)] {
        mp3(
            &b,
            &format!("Disc Author/Two Discs/CD{disc}/{n:02}.mp3"),
            0.4,
            &[
                ("album", "Two Discs"),
                ("artist", "Disc Author"),
                ("title", &format!("Disc {disc} part {n}")),
                ("track", &n.to_string()),
            ],
        );
    }
    for (i, t) in ["First", "Second", "Third"].iter().enumerate() {
        let i = i + 1;
        mp3(
            &b,
            &format!("Saga Writer/The Saga/Book {i} - {t}/01.mp3"),
            0.4,
            &[],
        );
        opf(
            &b,
            &format!("Saga Writer/The Saga/Book {i} - {t}/metadata.opf"),
            t,
            &i.to_string(),
        );
    }
    // Untagged: everything from the names.
    common::tone(
        &b,
        "Plain Author/Plain Title/01 Beginning.mp3",
        &["-c:a", "libmp3lame", "-map_metadata", "-1"],
        22_050,
        0.4,
        &[],
        None,
    );
    // Broken: not audio at all, under a readable name.
    std::fs::create_dir_all(b.join("Broken Author/Broken Book")).unwrap();
    std::fs::write(
        b.join("Broken Author/Broken Book/01.mp3"),
        b"this is not an mp3 file at all",
    )
    .unwrap();
    // DRM: listed, not played.
    std::fs::write(b.join("Mono Author/Locked Book.aax"), vec![0u8; 4096]).unwrap();
    std::fs::write(b.join("Mono Author/Other Locked.aax"), vec![0u8; 4096]).unwrap();
    // 40 files of noise (incompressible, 1.4 MB each): the scan must not read them whole.
    for n in 1..=40 {
        common::noise(
            &b,
            &format!("Long Author/Long Book/{n:02}.mp3"),
            &["-c:a", "libmp3lame", "-b:a", "128k"],
            22_050,
            90.0,
            &[
                ("album", "Long Book"),
                ("artist", "Long Author"),
                ("track", &n.to_string()),
            ],
            None,
        );
    }

    let lib = Library::open(&d.path().join("library.db")).unwrap();
    let root = b.to_string_lossy().to_string();
    lib.write(move |c| {
        c.execute("INSERT INTO source(id, kind, name, root, content) VALUES (2, 'local', 'Books', ?1, 'audiobooks')", [root])?;
        Ok(())
    })
    .await
    .unwrap();
    let read = Arc::new(AtomicU64::new(0));
    let src = Counting {
        inner: LocalSource,
        read: read.clone(),
    };
    let r = scan_audiobook_source(&lib, &src, 2, &VPath::new(b.to_string_lossy()))
        .await
        .unwrap();
    assert_eq!(r.books, 11);
    assert_eq!(r.read, 11);

    // One book each, with its author, narrator, file count and DRM mark.
    let listed = rows(
        &lib,
        "SELECT i.title || ' | ' || coalesce((SELECT group_concat(p.title, ', ') FROM book_credit c JOIN item p ON p.id = c.person_id WHERE c.book_id = i.id AND c.role = 'author'), '')
                || ' | ' || coalesce((SELECT group_concat(p.title, ', ') FROM book_credit c JOIN item p ON p.id = c.person_id WHERE c.book_id = i.id AND c.role = 'narrator'), '')
                || ' | ' || (SELECT count(*) FROM book_file f WHERE f.book_id = i.id) || ' | ' || b.drm
         FROM item i JOIN book b ON b.item_id = i.id ORDER BY i.title",
    )
    .await;
    assert_eq!(
        listed,
        [
            "Broken Book | Broken Author |  | 1 | 0",
            "First | Saga Writer | Saga Voice | 1 | 0",
            "Locked Book | Mono Author |  | 1 | 1",
            "Long Book | Long Author |  | 40 | 0",
            "Other Locked | Mono Author |  | 1 | 1",
            "Plain Title | Plain Author |  | 1 | 0",
            "Second | Saga Writer | Saga Voice | 1 | 0",
            "Single Book | Mono Author | Mono Reader | 1 | 0",
            "Third | Saga Writer | Saga Voice | 1 | 0",
            "Twelve Parts | Twelve Author | Twelve Reader | 12 | 0",
            "Two Discs | Disc Author |  | 4 | 0",
        ]
    );

    // Files on one timeline, in disc and track order.
    let discs = rows(
        &lib,
        "SELECT f.path || ' @' || bf.start_ms FROM book_file bf JOIN file f ON f.id = bf.file_id
          JOIN item i ON i.id = bf.book_id WHERE i.title = 'Two Discs' ORDER BY bf.idx",
    )
    .await;
    let names: Vec<&str> = discs
        .iter()
        .map(|l| l.split(" @").next().unwrap())
        .collect();
    assert!(names[0].ends_with("CD1/01.mp3") && names[1].ends_with("CD1/02.mp3"));
    assert!(names[2].ends_with("CD2/01.mp3") && names[3].ends_with("CD2/02.mp3"));
    let starts: Vec<i64> = discs
        .iter()
        .map(|l| l.rsplit('@').next().unwrap().parse().unwrap())
        .collect();
    assert_eq!(starts[0], 0);
    assert!(starts.windows(2).all(|w| w[1] > w[0]), "{starts:?}");

    // The series, in order, with positions from the OPF files.
    let saga = rows(
        &lib,
        "SELECT s.title || ' ' || CAST(b.series_position AS INTEGER) || ' ' || i.title FROM book b JOIN item i ON i.id = b.item_id
          JOIN item s ON s.id = i.parent_id ORDER BY b.series_position",
    )
    .await;
    assert_eq!(
        saga,
        ["The Saga 1 First", "The Saga 2 Second", "The Saga 3 Third"]
    );
    let isbn = rows(
        &lib,
        "SELECT b.isbn FROM book b JOIN item i ON i.id = b.item_id WHERE i.title = 'Second'",
    )
    .await;
    assert_eq!(isbn, ["9780000000002"]);

    // The 40-file book was read by its headers, not whole.
    let total: u64 = std::fs::read_dir(b.join("Long Author/Long Book"))
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    let read_all = read.load(Ordering::Relaxed);
    // Fingerprint chunks and tag blocks only: at most 256 KiB a file over the whole scan.
    assert!(
        read_all < 40 * 256 * 1024 && read_all < total / 4,
        "read {read_all} of {total} bytes"
    );

    // A rescan with nothing changed reads nothing; a changed book is read again.
    read.store(0, Ordering::Relaxed);
    let r = scan_audiobook_source(&lib, &src, 2, &VPath::new(b.to_string_lossy()))
        .await
        .unwrap();
    assert_eq!((r.books, r.read, r.removed), (11, 0, 0));
    assert_eq!(read.load(Ordering::Relaxed), 0);
    std::fs::remove_file(b.join("Twelve Author/Twelve Parts/12.mp3")).unwrap();
    std::fs::remove_dir_all(b.join("Broken Author")).unwrap();
    let r = scan_audiobook_source(&lib, &src, 2, &VPath::new(b.to_string_lossy()))
        .await
        .unwrap();
    assert_eq!((r.books, r.read), (10, 1));
    let left = rows(&lib, "SELECT i.title || ' ' || (SELECT count(*) FROM book_file f WHERE f.book_id = i.id) FROM item i JOIN book b ON b.item_id = i.id WHERE i.title IN ('Twelve Parts', 'Broken Book')").await;
    assert_eq!(left, ["Twelve Parts 11"]);
    let persons = rows(
        &lib,
        "SELECT title FROM item WHERE kind = 'person' AND title = 'Broken Author'",
    )
    .await;
    assert!(persons.is_empty(), "the author of a removed book is pruned");
}
