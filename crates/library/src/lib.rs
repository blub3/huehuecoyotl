//! The library database (SPEC 7): SQLite in WAL mode, exactly one writer thread fed by a
//! channel, and a small pool of read-only connections for UI queries (SPEC 4, threading).

pub mod books;
pub mod catalog;
pub mod copies;
pub mod jobs;
pub mod matching;
pub mod merge;
mod migrations;
pub mod music;
pub mod playlists;
pub mod queries;
pub mod queue;
pub mod text;
pub mod versions;

use std::path::Path;
use std::thread;

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use rusqlite::{Connection, OpenFlags};
use tokio::sync::oneshot;

pub use migrations::{MIGRATIONS, Migration};
pub use rusqlite;

pub const LOCAL_SOURCE_ID: i64 = 1;
const READERS: usize = 3;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("database writer is gone")]
    Closed,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, DbError>;

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// Cheap to clone; all clones share the writer thread and the read pool.
#[derive(Clone)]
pub struct Library {
    writer: Sender<Job>,
    readers: (Sender<Connection>, Receiver<Connection>),
}

fn configure(c: &Connection) -> rusqlite::Result<()> {
    c.pragma_update(None, "foreign_keys", "ON")?;
    c.pragma_update(None, "synchronous", "NORMAL")?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

/// Apply all pending migrations in one transaction each. Returns the new schema version.
pub fn migrate(c: &mut Connection) -> Result<usize> {
    let mut v: usize = c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? as usize;
    if v > MIGRATIONS.len() {
        return Err(DbError::Other(format!(
            "database schema {v} is newer than this build ({}); refusing to open",
            MIGRATIONS.len()
        )));
    }
    while v < MIGRATIONS.len() {
        // Foreign keys can only be switched outside a transaction.
        let rebuild = matches!(MIGRATIONS[v], Migration::Rebuild(_));
        if rebuild {
            c.pragma_update(None, "foreign_keys", "OFF")?;
        }
        let done = (|| -> Result<()> {
            let tx = c.transaction()?;
            match &MIGRATIONS[v] {
                Migration::Sql(sql) => tx.execute_batch(sql)?,
                Migration::Rust(f) | Migration::Rebuild(f) | Migration::Merge(f) => f(&tx)?,
            }
            if rebuild {
                let broken: i64 =
                    tx.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                        r.get(0)
                    })?;
                if broken > 0 {
                    return Err(DbError::Other(format!(
                        "migration {} would break {broken} references; not applied",
                        v + 1
                    )));
                }
            }
            tx.pragma_update(None, "user_version", (v + 1) as i64)?;
            tx.commit()?;
            Ok(())
        })();
        if rebuild {
            c.pragma_update(None, "foreign_keys", "ON")?;
        }
        done?;
        v += 1;
        tracing::info!(version = v, "database migrated");
    }
    Ok(v)
}

/// Before a migration that rebuilds a table (8: music) or merges rows (9: album versions), keep a copy of the database next to it
/// (`library.db.before-8`), made with `VACUUM INTO`, so the user can go back to the previous
/// version. An existing copy is kept.
fn backup_before_rebuild(c: &Connection, path: &Path) -> Result<()> {
    let v: usize = c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? as usize;
    let Some(next) = (v..MIGRATIONS.len())
        .find(|i| matches!(MIGRATIONS[*i], Migration::Rebuild(_) | Migration::Merge(_)))
    else {
        return Ok(());
    };
    if v == 0 {
        return Ok(()); // a new database: nothing to keep
    }
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".before-{}", next + 1));
    let dest = std::path::PathBuf::from(name);
    if dest.exists() {
        return Ok(());
    }
    c.execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()])?;
    tracing::info!(backup = %dest.display(), "database copied before migration {}", next + 1);
    Ok(())
}

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| DbError::Other(e.to_string()))?;
        }
        let mut w = Connection::open(path)?;
        w.pragma_update(None, "journal_mode", "WAL")?;
        configure(&w)?;
        backup_before_rebuild(&w, path)?;
        migrate(&mut w)?;

        let (rtx, rrx) = bounded(READERS);
        for _ in 0..READERS {
            let r = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX
                    | OpenFlags::SQLITE_OPEN_URI,
            )?;
            configure(&r)?;
            rtx.send(r).expect("pool has room");
        }

        let (tx, rx) = unbounded::<Job>();
        thread::Builder::new()
            .name("coyotl-db-writer".into())
            .spawn(move || {
                let mut w = w;
                for job in rx {
                    job(&mut w);
                }
            })
            .map_err(|e| DbError::Other(e.to_string()))?;
        Ok(Self {
            writer: tx,
            readers: (rtx, rrx),
        })
    }

    /// Run `f` on the single writer connection.
    pub async fn write<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        self.submit(f).await
    }

    /// Queue `f` on the writer **now** (writes run in submission order) and return a future
    /// for its result that does not borrow `self`.
    pub fn submit<T, F>(
        &self,
        f: F,
    ) -> impl std::future::Future<Output = Result<T>> + Send + 'static
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let sent = self.writer.send(Box::new(move |c| {
            let _ = tx.send(f(c));
        }));
        async move {
            sent.map_err(|_| DbError::Closed)?;
            rx.await.map_err(|_| DbError::Closed)?
        }
    }

    /// Run `f` on a read-only connection from the pool, off the async runtime.
    pub async fn read<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let (put, take) = self.readers.clone();
        tokio::task::spawn_blocking(move || {
            let c = take.recv().map_err(|_| DbError::Closed)?;
            let r = f(&c);
            let _ = put.send(c);
            r
        })
        .await
        .map_err(|e| DbError::Other(e.to_string()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_are_idempotent() {
        let mut c = Connection::open_in_memory().unwrap();
        assert_eq!(migrate(&mut c).unwrap(), MIGRATIONS.len());
        assert_eq!(migrate(&mut c).unwrap(), MIGRATIONS.len());
        let n: i64 = c
            .query_row("SELECT count(*) FROM source WHERE kind='local'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(n, 1);
        // FTS5 is compiled in.
        c.execute("INSERT INTO item_fts(title) VALUES ('Alien')", [])
            .unwrap();
    }

    /// Every row of a table, as text, in a stable order.
    fn dump(c: &Connection, table: &str) -> Vec<String> {
        let mut st = c
            .prepare(&format!("SELECT * FROM {table} ORDER BY 1, 2"))
            .unwrap();
        let n = st.column_count();
        st.query_map([], |r| {
            Ok((0..n)
                .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
                .collect::<Vec<_>>()
                .join("|"))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }

    /// Issue #6/#7: migration 8 rebuilds `item` for the music kinds and keeps everything a
    /// version-7 library has: items with their ids, files, play state, artwork, saved copies,
    /// indexes. A cascade on the dropped table would have deleted the play state.
    #[test]
    fn music_migration_keeps_video_libraries() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("library.db");
        {
            let mut c = Connection::open(&path).unwrap();
            c.pragma_update(None, "foreign_keys", "ON").unwrap();
            for m in &MIGRATIONS[..7] {
                let tx = c.transaction().unwrap();
                match m {
                    Migration::Sql(sql) => tx.execute_batch(sql).unwrap(),
                    Migration::Rust(f) | Migration::Rebuild(f) | Migration::Merge(f) => {
                        f(&tx).unwrap()
                    }
                }
                tx.commit().unwrap();
            }
            c.pragma_update(None, "user_version", 7).unwrap();
            c.execute_batch(
                "INSERT INTO source(id, kind, name, root) VALUES (2, 'local', 'Media', '/m');
                 INSERT INTO item(id, kind, title, sort_title, year, tmdb_id, match_state, added_at) VALUES
                   (10, 'movie', 'Alien', 'alien', 1979, 348, 'matched', 1),
                   (20, 'show', 'Firefly', 'firefly', 2002, 1437, 'matched', 2);
                 INSERT INTO item(id, kind, title, parent_id, season_no) VALUES (21, 'season', 'Season 1', 20, 1);
                 INSERT INTO item(id, kind, title, parent_id, season_no, episode_no) VALUES (22, 'episode', 'Serenity', 21, 1, 1);
                 INSERT INTO item_fts(rowid, title) VALUES (10, 'Alien'), (20, 'Firefly');
                 INSERT INTO file(id, source_id, path, size, item_id) VALUES (100, 2, 'Alien.mkv', 5, 10), (101, 2, 'F/S01E01.mkv', 5, 22);
                 INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at, last_file_id)
                   VALUES (10, 1, 60000, 7000000, 0, 5, 100), (22, 1, 0, 2000000, 1, 6, 101);
                 INSERT INTO file_state(file_id, profile_id, position_ms, updated_at) VALUES (100, 1, 60000, 5);
                 INSERT INTO artwork(item_id, role, remote_path, local_path) VALUES (10, 'poster', '/a.jpg', '/c/a.jpg');
                 INSERT INTO track_pref(item_id, profile_id, audio_lang, sub_lang, sub_off) VALUES (20, 1, 'ger', 'eng', 0);
                 INSERT INTO local_copy(id, kind, file_id, item_id, source_id, source_path, path, size, state, created_at)
                   VALUES (900, 'saved', 101, 22, 2, '/m/F/S01E01.mkv', '/s/S01E01.mkv', 5, 'done', 1);",
            )
            .unwrap();
        }
        let before = {
            let c = Connection::open(&path).unwrap();
            let tables = [
                "item",
                "file",
                "playstate",
                "file_state",
                "artwork",
                "track_pref",
                "local_copy",
            ];
            let rows: Vec<Vec<String>> = tables.iter().map(|t| dump(&c, t)).collect();
            let indexes = dump_indexes(&c);
            (rows, indexes)
        };

        let lib = Library::open(&path).unwrap();
        drop(lib);
        let c = Connection::open(&path).unwrap();
        let v: i64 = c
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());
        let tables = [
            "item",
            "file",
            "playstate",
            "file_state",
            "artwork",
            "track_pref",
            "local_copy",
        ];
        for (i, t) in tables.iter().enumerate() {
            let now = dump(&c, t);
            if *t == "playstate" {
                // Two new columns (play count 0, never played) at the end.
                let trimmed: Vec<String> = now
                    .iter()
                    .map(|r| r.rsplitn(3, '|').nth(2).unwrap().to_string())
                    .collect();
                assert_eq!(trimmed, before.0[i], "{t}");
            } else if *t == "artwork" {
                // Migration 12 adds the Now Playing colour (none yet) at the end.
                let trimmed: Vec<String> = now
                    .iter()
                    .map(|r| r.rsplit_once('|').unwrap().0.to_string())
                    .collect();
                assert_eq!(trimmed, before.0[i], "{t}");
            } else if *t == "file" {
                // Migration 9 adds the version (none for video files) at the end.
                let trimmed: Vec<String> = now
                    .iter()
                    .map(|r| r.rsplit_once('|').unwrap().0.to_string())
                    .collect();
                assert_eq!(trimmed, before.0[i], "{t}");
            } else {
                assert_eq!(now, before.0[i], "{t}");
            }
        }
        // Every index on item came back; the music ones are new.
        let after = dump_indexes(&c);
        for ix in &before.1 {
            assert!(after.contains(ix), "{ix} missing");
        }
        // Existing folders are video folders; the music kinds are allowed now.
        let video: i64 = c
            .query_row(
                "SELECT count(*) FROM source WHERE content = 'video'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let all: i64 = c
            .query_row("SELECT count(*) FROM source", [], |r| r.get(0))
            .unwrap();
        assert_eq!((video, all), (2, 2));
        c.execute(
            "INSERT INTO item(kind, title) VALUES ('album', 'Abbey Road')",
            [],
        )
        .unwrap();
        assert!(
            c.execute("INSERT INTO item(kind, title) VALUES ('podcast', 'x')", [])
                .is_err()
        );
        let broken: i64 = c
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(broken, 0);
        // The copy made before the migration is the version-7 database.
        let copy = Connection::open(d.path().join("library.db.before-8")).unwrap();
        let v: i64 = copy
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 7);
        assert_eq!(dump(&copy, "playstate"), before.0[2]);
    }

    /// Issue #65: migration 10 rebuilds `item` and `source` for audiobooks and keeps a
    /// version-9 library with music: album versions, credits, the queue with its files, play
    /// counts, music and video folders. A copy is kept as `library.db.before-10`.
    #[test]
    fn audiobook_migration_keeps_music_libraries() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("library.db");
        {
            let mut c = Connection::open(&path).unwrap();
            c.pragma_update(None, "foreign_keys", "ON").unwrap();
            for m in &MIGRATIONS[..9] {
                let tx = c.transaction().unwrap();
                match m {
                    Migration::Sql(sql) => tx.execute_batch(sql).unwrap(),
                    Migration::Rust(f) | Migration::Rebuild(f) | Migration::Merge(f) => {
                        f(&tx).unwrap()
                    }
                }
                tx.commit().unwrap();
            }
            c.pragma_update(None, "user_version", 9).unwrap();
            c.execute_batch(
                "INSERT INTO source(id, kind, name, root, content) VALUES (2, 'local', 'Music', '/mu', 'music'), (3, 'local', 'Films', '/f', 'video');
                 INSERT INTO item(id, kind, title, sort_title) VALUES (10, 'artist', 'Band', 'band'), (11, 'album', 'Album', 'album'),
                   (12, 'track', 'Song', 'song'), (20, 'movie', 'Alien', 'alien');
                 UPDATE item SET parent_id = 11 WHERE id = 12;
                 INSERT INTO credit(item_id, artist_id, role, position) VALUES (11, 10, 'album_artist', 0), (12, 10, 'artist', 0);
                 INSERT INTO album_version(id, album_id, key, source_id, title) VALUES (5, 11, 'v:2:band|album', 2, '');
                 INSERT INTO file(id, source_id, path, size, item_id, version_id) VALUES (100, 2, 'Band/Album/01.flac', 5, 12, 5), (101, 3, 'Alien.mkv', 5, 20, NULL);
                 INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at, last_file_id, play_count, last_played_at)
                   VALUES (12, 1, 0, 200000, 0, 5, 100, 7, 5), (20, 1, 60000, 7000000, 0, 6, 101, 0, NULL);
                 INSERT INTO play_queue(position, item_id, file_id, shuffled_position) VALUES (0, 12, 100, NULL);",
            )
            .unwrap();
        }
        let tables = [
            "source",
            "item",
            "credit",
            "album_version",
            "file",
            "playstate",
            "play_queue",
        ];
        let (before, indexes) = {
            let c = Connection::open(&path).unwrap();
            let rows: Vec<Vec<String>> = tables.iter().map(|t| dump(&c, t)).collect();
            (rows, dump_indexes(&c))
        };

        drop(Library::open(&path).unwrap());
        let c = Connection::open(&path).unwrap();
        for (i, t) in tables.iter().enumerate() {
            assert_eq!(dump(&c, t), before[i], "{t}");
        }
        let after = dump_indexes(&c);
        for ix in &indexes {
            assert!(after.contains(ix), "{ix} missing");
        }
        c.execute_batch(
            "INSERT INTO source(id, kind, name, root, content) VALUES (4, 'local', 'Books', '/b', 'audiobooks');
             INSERT INTO item(id, kind, title) VALUES (30, 'audiobook', 'Book'), (31, 'person', 'Writer'), (32, 'book_series', 'Saga');
             INSERT INTO book(item_id, key) VALUES (30, 'b:4:Book');
             INSERT INTO book_credit(book_id, person_id, role, position) VALUES (30, 31, 'author', 0);",
        )
        .unwrap();
        let broken: i64 = c
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(broken, 0);
        let copy = Connection::open(d.path().join("library.db.before-10")).unwrap();
        let v: i64 = copy
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 9);
        assert_eq!(dump(&copy, "play_queue"), before[6]);
    }

    fn dump_indexes(c: &Connection) -> Vec<String> {
        let mut st = c
            .prepare("SELECT name || ': ' || sql FROM sqlite_master WHERE type = 'index' AND sql IS NOT NULL ORDER BY name")
            .unwrap();
        st.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn refuses_newer_schema() {
        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "user_version", 999).unwrap();
        assert!(migrate(&mut c).is_err());
    }

    #[tokio::test]
    async fn writer_and_readers_see_the_same_data() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::open(&d.path().join("library.db")).unwrap();
        lib.write(|c| Ok(c.execute("INSERT INTO item(kind, title) VALUES ('video','x')", [])?))
            .await
            .unwrap();
        let n: i64 = lib
            .read(|c| Ok(c.query_row("SELECT count(*) FROM item", [], |r| r.get(0))?))
            .await
            .unwrap();
        assert_eq!(n, 1);
        let mode: String = lib
            .read(|c| Ok(c.pragma_query_value(None, "journal_mode", |r| r.get(0))?))
            .await
            .unwrap();
        assert_eq!(mode, "wal");
    }
}
