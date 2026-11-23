//! Issue #63: copies of one album become versions of it, across folders and sources, from
//! generated fixtures: one album as FLAC 24/96 and as MP3 in two folders and as FLAC 16/44.1 on a
//! second source (three versions); a remaster with one extra track in the same release group (an
//! edition); an album with a replaced track (two files of one track in one version); and an
//! album of the same name and year with other songs (not a copy: an album of its own).
mod common;

use huehuecoyotl_library::versions::{self, Choice};
use huehuecoyotl_library::{Library, rusqlite};
use huehuecoyotl_scanner::music::scan_music_source;
use huehuecoyotl_vfs::{LocalSource, VPath};

fn tagged(
    album: &str,
    n: u32,
    title: &str,
    extra: &[(&'static str, &'static str)],
) -> Vec<(String, String)> {
    let mut t = vec![
        ("title".to_string(), title.to_string()),
        ("artist".into(), "Band".into()),
        ("album_artist".into(), "Band".into()),
        ("album".into(), album.into()),
        ("date".into(), "2000".into()),
        ("track".into(), n.to_string()),
    ];
    t.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    t
}

fn make(dir: &std::path::Path, rel: &str, codec: &[&str], rate: u32, tags: &[(String, String)]) {
    let pairs: Vec<(&str, &str)> = tags.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    common::tone(dir, rel, codec, rate, 0.4, &pairs, None);
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
async fn copies_become_versions() {
    if !common::ffmpeg() {
        eprintln!("skipped: ffmpeg is not installed");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let (a, b) = (d.path().join("nas"), d.path().join("laptop"));
    let flac24 = ["-c:a", "flac", "-sample_fmt", "s32"];
    let flac16 = ["-c:a", "flac"];
    let mp3 = ["-c:a", "libmp3lame", "-b:a", "320k"];
    for n in 1..=3 {
        let t = tagged("Album", n, &format!("Song {n}"), &[]);
        make(
            &a,
            &format!("Band/Album (2000)/0{n}.flac"),
            &flac24,
            96_000,
            &t,
        );
        make(
            &a,
            &format!("Band/Album (2000) [MP3]/0{n}.mp3"),
            &mp3,
            44_100,
            &t,
        );
        make(&b, &format!("Album/0{n}.flac"), &flac16, 44_100, &t);
        // Same name and year, other songs: another album.
        make(
            &a,
            &format!("Band/Album live/0{n}.flac"),
            &flac16,
            44_100,
            &tagged("Album", n, &format!("Live {n}"), &[]),
        );
    }
    // A remaster with one more track, in the same release group.
    for n in 1..=2 {
        let t = tagged(
            "Other",
            n,
            &format!("Other {n}"),
            &[
                ("MUSICBRAINZ_RELEASEGROUPID", "rg-other"),
                ("MUSICBRAINZ_ALBUMID", "rel-orig"),
            ],
        );
        make(
            &a,
            &format!("Band/Other (2001)/0{n}.flac"),
            &flac16,
            44_100,
            &t,
        );
    }
    for n in 1..=3 {
        let t = tagged(
            "Other (2011 Remaster)",
            n,
            &format!("Other {n}"),
            &[
                ("MUSICBRAINZ_RELEASEGROUPID", "rg-other"),
                ("MUSICBRAINZ_ALBUMID", "rel-remaster"),
            ],
        );
        make(
            &a,
            &format!("Band/Other (2011 Remaster)/0{n}.flac"),
            &flac24,
            96_000,
            &t,
        );
    }
    // A replaced track: two files of "Fix 2" in one folder.
    make(
        &a,
        "Band/Fixed (2003)/01.flac",
        &flac16,
        44_100,
        &tagged("Fixed", 1, "Fix 1", &[]),
    );
    make(
        &a,
        "Band/Fixed (2003)/02.flac",
        &flac16,
        44_100,
        &tagged("Fixed", 2, "Fix 2", &[]),
    );
    make(
        &a,
        "Band/Fixed (2003)/02 (new master).flac",
        &flac24,
        96_000,
        &tagged("Fixed", 2, "Fix 2", &[]),
    );

    // One folder with the album in FLAC and in MP3 (found in a real library): two versions.
    for n in 1..=2 {
        let t = tagged("Twin", n, &format!("Twin {n}"), &[]);
        make(
            &a,
            &format!("Band/Twin (2004)/0{n}.flac"),
            &flac16,
            44_100,
            &t,
        );
        make(&a, &format!("Band/Twin (2004)/0{n}.mp3"), &mp3, 44_100, &t);
    }
    // One title twice on a deluxe album (an acoustic take as track 3): two songs.
    make(
        &a,
        "Band/Deluxe (2005)/01.flac",
        &flac16,
        44_100,
        &tagged("Deluxe", 1, "Shake", &[]),
    );
    make(
        &a,
        "Band/Deluxe (2005)/02.flac",
        &flac16,
        44_100,
        &tagged("Deluxe", 2, "Other", &[]),
    );
    make(
        &a,
        "Band/Deluxe (2005)/03.flac",
        &flac16,
        44_100,
        &tagged("Deluxe", 3, "Shake", &[]),
    );

    let lib = Library::open(&d.path().join("library.db")).unwrap();
    let (ra, rb) = (
        a.to_string_lossy().to_string(),
        b.to_string_lossy().to_string(),
    );
    lib.write(move |c| {
        c.execute(
            "INSERT INTO source(id, kind, name, root, content) VALUES (2, 'local', 'NAS', ?1, 'music'), (3, 'local', 'Laptop', ?2, 'music')",
            [ra, rb],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    scan_music_source(&lib, &LocalSource, 2, &VPath::new(a.to_string_lossy()))
        .await
        .unwrap();
    scan_music_source(&lib, &LocalSource, 3, &VPath::new(b.to_string_lossy()))
        .await
        .unwrap();

    // One album per album, its versions counted.
    let albums = rows(
        &lib,
        "SELECT a.title || ' ' || (SELECT count(*) FROM album_version v WHERE v.album_id = a.id)
         FROM item a WHERE a.kind = 'album' ORDER BY a.title, (SELECT count(*) FROM album_version v WHERE v.album_id = a.id) DESC, a.locked",
    )
    .await;
    assert_eq!(
        albums,
        [
            "Album 3",
            "Album 1",
            "Deluxe 1",
            "Fixed 1",
            "Other 1",
            "Other (2011 Remaster) 1",
            "Twin 2"
        ]
    );
    let deluxe = rows(
        &lib,
        "SELECT t.title || ' ' || t.episode_no FROM item t JOIN item a ON a.id = t.parent_id WHERE a.title = 'Deluxe' ORDER BY t.episode_no",
    )
    .await;
    assert_eq!(deluxe, ["Shake 1", "Other 2", "Shake 3"]);
    let twin = rows(
        &lib,
        "SELECT group_concat(DISTINCT fa.codec) FROM album_version v JOIN item a ON a.id = v.album_id JOIN file f ON f.version_id = v.id
         JOIN file_audio fa ON fa.file_id = f.id WHERE a.title = 'Twin' GROUP BY v.id ORDER BY 1",
    )
    .await;
    assert_eq!(twin, ["flac", "mp3"]);

    let (album, other, fixed) = lib
        .read(|c| {
            let id = |sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0));
            Ok((
                id("SELECT a.id FROM item a WHERE a.kind = 'album' AND a.title = 'Album' AND (SELECT count(*) FROM album_version v WHERE v.album_id = a.id) = 3")?,
                id("SELECT id FROM item WHERE kind = 'album' AND title = 'Other'")?,
                id("SELECT id FROM item WHERE kind = 'album' AND title = 'Fixed'")?,
            ))
        })
        .await
        .unwrap();

    // Three versions, labelled from their files and sources; preselection picks the 24/96 copy.
    let vs = lib
        .read(move |c| versions::versions(c, album))
        .await
        .unwrap();
    let labels: Vec<String> = vs
        .iter()
        .map(|v| {
            format!(
                "{} {}/{} lossless={} {} tracks={}",
                v.codec, v.bit_depth, v.sample_rate, v.lossless, v.source_name, v.tracks
            )
        })
        .collect();
    assert_eq!(
        labels,
        [
            "flac 24/96000 lossless=true NAS tracks=3",
            "flac 16/44100 lossless=true Laptop tracks=3",
            "mp3 0/44100 lossless=false NAS tracks=3",
        ]
    );
    assert!(vs[0].hires && !vs[1].hires);
    let choices: Vec<Choice> = vs.iter().map(Choice::from).collect();
    assert_eq!(
        versions::preselect(&choices, None, "quality"),
        Some(vs[0].id)
    );
    // (A sine tone in FLAC can be smaller than in MP3 at 320 kbit/s: compare with the sizes.)
    let smallest = vs.iter().min_by_key(|v| v.size).unwrap().id;
    assert_eq!(
        versions::preselect(&choices, None, "smallest"),
        Some(smallest)
    );
    // Every version has the same three tracks: one track item each, with three files.
    let files_per_track = rows(
        &lib,
        "SELECT t.title || ' ' || count(f.id) FROM item t JOIN item a ON a.id = t.parent_id JOIN file f ON f.item_id = t.id
         WHERE a.title = 'Album' AND t.title LIKE 'Song %' GROUP BY t.id ORDER BY t.title",
    )
    .await;
    assert_eq!(files_per_track, ["Song 1 3", "Song 2 3", "Song 3 3"]);

    // Issue #147: the remaster is another release, so an album of its own, linked as an edition
    // through the release group; its extra track is only there.
    let (remaster_album, linked) = lib
        .read(move |c| {
            let id: i64 = c.query_row(
                "SELECT id FROM item WHERE kind = 'album' AND title = 'Other (2011 Remaster)'",
                [],
                |r| r.get(0),
            )?;
            Ok((id, versions::other_editions(c, other)?))
        })
        .await
        .unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].id, remaster_album);
    let orig = lib
        .read(move |c| versions::versions(c, other))
        .await
        .unwrap()[0]
        .id;
    let remaster = lib
        .read(move |c| versions::versions(c, remaster_album))
        .await
        .unwrap()[0]
        .id;
    let tracks = |v: i64| {
        let lib = &lib;
        async move {
            let ids = lib
                .read(move |c| versions::version_tracks(c, v, "quality"))
                .await
                .unwrap();
            lib.read(move |c| {
                ids.iter()
                    .map(|(t, _)| {
                        c.query_row("SELECT title FROM item WHERE id = ?1", [t], |r| r.get(0))
                            .map_err(Into::into)
                    })
                    .collect::<huehuecoyotl_library::Result<Vec<String>>>()
            })
            .await
            .unwrap()
        }
    };
    assert_eq!(tracks(orig).await, ["Other 1", "Other 2"]);
    assert_eq!(tracks(remaster).await, ["Other 1", "Other 2", "Other 3"]);

    // The replaced track: two files in one version; the better one plays unless the other was
    // played last.
    let fv = lib
        .read(move |c| versions::versions(c, fixed))
        .await
        .unwrap();
    assert_eq!(fv.len(), 1);
    let v = fv[0].id;
    let fix2: i64 = lib
        .read(|c| {
            Ok(
                c.query_row("SELECT id FROM item WHERE title = 'Fix 2'", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .await
        .unwrap();
    let files = lib
        .read(move |c| versions::track_files(c, fix2, v))
        .await
        .unwrap();
    assert_eq!(files.len(), 2);
    let best = files.iter().find(|f| f.bit_depth == 24).unwrap().id;
    let other_file = files.iter().find(|f| f.bit_depth != 24).unwrap().id;
    assert_eq!(
        lib.read(move |c| versions::track_file(c, fix2, v, "quality"))
            .await
            .unwrap(),
        Some(best)
    );
    lib.write(move |c| versions::remember_played(c, fix2, other_file, 1))
        .await
        .unwrap();
    assert_eq!(
        lib.read(move |c| versions::track_file(c, fix2, v, "quality"))
            .await
            .unwrap(),
        Some(other_file)
    );

    // A rescan changes nothing.
    scan_music_source(&lib, &LocalSource, 2, &VPath::new(a.to_string_lossy()))
        .await
        .unwrap();
    scan_music_source(&lib, &LocalSource, 3, &VPath::new(b.to_string_lossy()))
        .await
        .unwrap();
    let again = rows(
        &lib,
        "SELECT a.title || ' ' || (SELECT count(*) FROM album_version v WHERE v.album_id = a.id)
         FROM item a WHERE a.kind = 'album' ORDER BY a.title, (SELECT count(*) FROM album_version v WHERE v.album_id = a.id) DESC, a.locked",
    )
    .await;
    assert_eq!(again, albums);

    // "This is a different album": the MP3 copy becomes an album of its own and stays one.
    let mp3v = vs[2].id;
    let split = lib
        .write(move |c| versions::split_version(c, mp3v, 2))
        .await
        .unwrap();
    scan_music_source(&lib, &LocalSource, 2, &VPath::new(a.to_string_lossy()))
        .await
        .unwrap();
    let after = rows(
        &lib,
        "SELECT a.title || ' ' || (SELECT count(*) FROM album_version v WHERE v.album_id = a.id) || ' ' || a.locked
         FROM item a WHERE a.kind = 'album' ORDER BY a.title, (SELECT count(*) FROM album_version v WHERE v.album_id = a.id) DESC, a.locked",
    )
    .await;
    assert_eq!(
        after,
        [
            "Album 2 0",
            "Album 1 0",
            "Album 1 1",
            "Deluxe 1 0",
            "Fixed 1 0",
            "Other 1 0",
            "Other (2011 Remaster) 1 0",
            "Twin 2 0"
        ]
    );
    let split_tracks = lib
        .read(move |c| versions::version_tracks(c, mp3v, "quality"))
        .await
        .unwrap();
    assert_eq!(split_tracks.len(), 3);
    let parents: Vec<i64> = lib
        .read(move |c| {
            split_tracks
                .iter()
                .map(|(t, _)| {
                    c.query_row("SELECT parent_id FROM item WHERE id = ?1", [t], |r| {
                        r.get(0)
                    })
                    .map_err(Into::into)
                })
                .collect::<huehuecoyotl_library::Result<Vec<i64>>>()
        })
        .await
        .unwrap();
    assert!(parents.iter().all(|p| *p == split));
}
