//! Migrate a **copy** of a library database to the current schema and report what it holds, to
//! check a migration against a real library before releasing it (issue #7: migration 8).
//!
//!   cargo run -p huehuecoyotl-library --example migrate -- /path/to/copy/library.db
//!
//! The database is changed in place, and the pre-migration copy (`library.db.before-8`) is
//! written next to it. Never point this at the library an app instance uses.
use huehuecoyotl_library::{Library, rusqlite::Connection};

fn counts(path: &std::path::Path) -> Vec<(String, i64)> {
    let c = Connection::open(path).unwrap();
    let mut out = vec![(
        "user_version".to_string(),
        c.pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap(),
    )];
    for sql in [
        "SELECT 'items ' || kind, count(*) FROM item GROUP BY kind ORDER BY kind",
        "SELECT 'files', count(*) FROM file",
        "SELECT 'play state', count(*) FROM playstate",
        "SELECT 'watched', count(*) FROM playstate WHERE watched = 1",
        "SELECT 'artwork', count(*) FROM artwork",
        "SELECT 'offline copies', count(*) FROM local_copy",
        "SELECT 'broken references', count(*) FROM pragma_foreign_key_check",
    ] {
        let mut st = c.prepare(sql).unwrap();
        let rows = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .unwrap();
        out.extend(rows.map(Result::unwrap));
    }
    out
}

fn main() {
    let path = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("path to a copy of library.db"),
    );
    let before = counts(&path);
    drop(Library::open(&path).expect("migrate"));
    let after = counts(&path);
    let integrity: String = Connection::open(&path)
        .unwrap()
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    println!("{:<24} {:>8} {:>8}", "", "before", "after");
    for (k, v) in &after {
        let b = before
            .iter()
            .find(|(bk, _)| bk == k)
            .map(|(_, v)| v.to_string())
            .unwrap_or("-".into());
        println!("{k:<24} {b:>8} {v:>8}");
    }
    println!("integrity: {integrity}");
}
