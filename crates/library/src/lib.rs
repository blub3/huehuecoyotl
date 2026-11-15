//! The library database (SPEC 7): SQLite in WAL mode, exactly one writer thread fed by a
//! channel, and a small pool of read-only connections for UI queries (SPEC 4, threading).

pub mod catalog;
pub mod copies;
pub mod jobs;
pub mod merge;
mod migrations;
pub mod queries;

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
        let tx = c.transaction()?;
        match &MIGRATIONS[v] {
            Migration::Sql(sql) => tx.execute_batch(sql)?,
            Migration::Rust(f) => f(&tx)?,
        }
        tx.pragma_update(None, "user_version", (v + 1) as i64)?;
        tx.commit()?;
        v += 1;
        tracing::info!(version = v, "database migrated");
    }
    Ok(v)
}

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| DbError::Other(e.to_string()))?;
        }
        let mut w = Connection::open(path)?;
        w.pragma_update(None, "journal_mode", "WAL")?;
        configure(&w)?;
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
