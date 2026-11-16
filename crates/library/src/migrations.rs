//! Numbered schema migrations (SPEC 7). Index `i` brings `user_version` from `i` to `i + 1`.
//! Never edit a migration that has been released; add a new one. A migration is SQL, or a Rust
//! function for repairs that need logic; each runs in its own transaction.

use rusqlite::Transaction;

pub enum Migration {
    Sql(&'static str),
    Rust(fn(&Transaction) -> crate::Result<()>),
    /// A table rebuild (SQLite's twelve-step procedure): runs with foreign keys off, so dropping
    /// the old table cascades nothing, and commits only when `PRAGMA foreign_key_check` is clean.
    Rebuild(fn(&Transaction) -> crate::Result<()>),
    /// Rust that merges rows (9: album versions): the database is copied first, as before a
    /// rebuild, so the user can go back.
    Merge(fn(&Transaction) -> crate::Result<()>),
}

pub const MIGRATIONS: &[Migration] = &[
    // 1: initial schema
    Migration::Sql(
        r#"
CREATE TABLE source   (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL,
                       config_json TEXT NOT NULL DEFAULT '{}', credential_ref TEXT);

CREATE TABLE item     (id INTEGER PRIMARY KEY,
                       kind TEXT NOT NULL CHECK (kind IN ('movie','show','season','episode','video')),
                       parent_id INTEGER REFERENCES item(id) ON DELETE CASCADE,
                       title TEXT, sort_title TEXT, year INTEGER,
                       season_no INTEGER, episode_no INTEGER,
                       tmdb_id INTEGER, imdb_id TEXT,
                       match_state TEXT,
                       locked INTEGER NOT NULL DEFAULT 0,
                       meta_json TEXT, meta_fetched_at INTEGER);
CREATE INDEX item_parent ON item(parent_id);

CREATE TABLE file     (id INTEGER PRIMARY KEY,
                       source_id INTEGER NOT NULL REFERENCES source(id) ON DELETE CASCADE,
                       path TEXT NOT NULL, size INTEGER, mtime INTEGER,
                       fingerprint BLOB,
                       probe_json TEXT,
                       item_id INTEGER REFERENCES item(id) ON DELETE SET NULL,
                       last_seen_scan INTEGER,
                       UNIQUE (source_id, path));
CREATE INDEX file_item ON file(item_id);

CREATE TABLE artwork  (item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                       role TEXT NOT NULL, remote_path TEXT,
                       local_path TEXT, width INTEGER, height INTEGER);

CREATE TABLE playstate(item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                       profile_id INTEGER NOT NULL DEFAULT 1,
                       position_ms INTEGER NOT NULL DEFAULT 0, duration_ms INTEGER NOT NULL DEFAULT 0,
                       watched INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL,
                       PRIMARY KEY (item_id, profile_id));
CREATE INDEX playstate_recent ON playstate(profile_id, updated_at);

-- Audio/subtitle language choice, remembered per series (or per item for loose videos).
CREATE TABLE track_pref(item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                       profile_id INTEGER NOT NULL DEFAULT 1,
                       audio_lang TEXT, sub_lang TEXT, sub_off INTEGER NOT NULL DEFAULT 0,
                       PRIMARY KEY (item_id, profile_id));

CREATE TABLE job      (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, payload_json TEXT NOT NULL,
                       priority INTEGER NOT NULL DEFAULT 0, state TEXT NOT NULL DEFAULT 'queued',
                       attempts INTEGER NOT NULL DEFAULT 0, run_after INTEGER NOT NULL DEFAULT 0);
CREATE INDEX job_next ON job(state, priority DESC, run_after);

CREATE VIRTUAL TABLE item_fts USING fts5(title, original_title, overview);

-- Source 1 is the local file system; paths are absolute OS paths with '/' separators.
INSERT INTO source(id, kind, name) VALUES (1, 'local', 'This computer');
"#,
    ),
    // 2: library folders, item metadata columns, grouping keys, match candidates, job dedupe
    Migration::Sql(
        r#"
-- Library folders: local sources with a root. Source 1 stays the ad-hoc "this computer" source.
ALTER TABLE source ADD COLUMN root TEXT;
ALTER TABLE source ADD COLUMN last_scan INTEGER;

ALTER TABLE item ADD COLUMN original_title TEXT;
ALTER TABLE item ADD COLUMN overview TEXT;
ALTER TABLE item ADD COLUMN rating REAL;
ALTER TABLE item ADD COLUMN runtime_min INTEGER;
ALTER TABLE item ADD COLUMN added_at INTEGER;
-- Grouping key: normalised title(+year) for movies and shows, so rescans find the same item.
ALTER TABLE item ADD COLUMN group_key TEXT;
CREATE INDEX item_group ON item(kind, group_key);
CREATE INDEX item_sort ON item(kind, sort_title);
CREATE INDEX item_added ON item(kind, added_at);
CREATE INDEX file_source ON file(source_id, last_seen_scan);

-- Candidates offered for "needs review" items.
CREATE TABLE match_candidate(item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                             provider_id INTEGER NOT NULL, title TEXT, year INTEGER,
                             score REAL, poster_path TEXT, overview TEXT);
CREATE INDEX match_candidate_item ON match_candidate(item_id);

ALTER TABLE job ADD COLUMN dedupe TEXT;
ALTER TABLE job ADD COLUMN last_error TEXT;
CREATE UNIQUE INDEX job_dedupe ON job(dedupe) WHERE state IN ('queued', 'running');
"#,
    ),
    // 3: every poster lookup in the summary queries filters artwork by item; without this index
    // "recently added" scanned the whole artwork table once per item (1.2 s at 5,000 movies).
    Migration::Sql(
        r#"
CREATE INDEX artwork_item ON artwork(item_id, role);
"#,
    ),
    // 4 (issue #32): one item per TMDB show or movie. Merges the duplicates that 1.0 and 1.1
    // created, then makes the TMDB id unique and queues the offline regroup of shows.
    Migration::Rust(crate::merge::migration_4),
    // 5 (issue #32, show page): seasons and episodes TMDB knows that are not on disk. They are
    // shown dimmed and cannot be played; a scan that finds their file turns them into normal
    // items, and pruning keeps them while the show has anything on disk.
    Migration::Sql(
        r#"
ALTER TABLE item ADD COLUMN virtual INTEGER NOT NULL DEFAULT 0;
"#,
    ),
    // 6 (issue #34): several versions of one movie or episode. The version played last, and a
    // resume position per version for editions of a different length (a director's cut).
    Migration::Sql(
        r#"
ALTER TABLE playstate ADD COLUMN last_file_id INTEGER;
CREATE TABLE file_state(file_id INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
                        profile_id INTEGER NOT NULL DEFAULT 1,
                        position_ms INTEGER NOT NULL DEFAULT 0, duration_ms INTEGER NOT NULL DEFAULT 0,
                        updated_at INTEGER NOT NULL,
                        PRIMARY KEY (file_id, profile_id));
"#,
    ),
    // 7 (issue #3): offline copies. A row per copied file: the main file of a "Save" or a
    // "Save as…", plus its sidecar subtitles and the other parts of a multi-part file, which
    // point at it through `main_id`. Small key/value settings owned by the core.
    Migration::Sql(
        r#"
CREATE TABLE local_copy(id INTEGER PRIMARY KEY,
                        kind TEXT NOT NULL DEFAULT 'saved' CHECK (kind IN ('saved','export')),
                        main_id INTEGER REFERENCES local_copy(id) ON DELETE CASCADE,
                        file_id INTEGER REFERENCES file(id) ON DELETE SET NULL,
                        item_id INTEGER REFERENCES item(id) ON DELETE SET NULL,
                        source_id INTEGER NOT NULL,
                        source_path TEXT NOT NULL,
                        src_mtime INTEGER,
                        path TEXT NOT NULL,
                        size INTEGER NOT NULL DEFAULT 0,
                        bytes_done INTEGER NOT NULL DEFAULT 0,
                        state TEXT NOT NULL DEFAULT 'queued'
                          CHECK (state IN ('queued','running','paused','done','failed','outdated')),
                        error TEXT,
                        error_kind TEXT,
                        created_at INTEGER NOT NULL,
                        completed_at INTEGER);
CREATE INDEX local_copy_file ON local_copy(file_id);
CREATE INDEX local_copy_item ON local_copy(item_id);
CREATE INDEX local_copy_main ON local_copy(main_id);

-- A source file that changed in place, or disappeared, makes its saved copy outdated.
CREATE TRIGGER local_copy_changed AFTER UPDATE OF size, mtime ON file
  WHEN OLD.path = NEW.path AND (OLD.size IS NOT NEW.size OR OLD.mtime IS NOT NEW.mtime)
BEGIN
  UPDATE local_copy SET state = 'outdated'
   WHERE file_id = NEW.id AND kind = 'saved' AND state = 'done';
END;
CREATE TRIGGER local_copy_gone BEFORE DELETE ON file
BEGIN
  UPDATE local_copy SET state = 'outdated'
   WHERE file_id = OLD.id AND kind = 'saved' AND state = 'done';
END;

CREATE TABLE setting(key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#,
    ),
    // 8: music (2.0, SPEC 7 "Music library"): the kinds artist, album and track, content types
    // of library folders, and the music tables. Ids are kept, so files, play state, artwork and
    // offline copies stay attached.
    Migration::Rebuild(music_schema),
    // 9 (issue #63): album versions and editions. Every album becomes a version of an album keyed
    // by its release group, release, or album artist, title and year; copies are merged.
    Migration::Merge(music_versions),
    // 10 (2.5, SPEC 7 "Audiobook library"): the kinds audiobook, person and book_series, the
    // content type audiobooks, and the book tables. Ids are kept.
    Migration::Rebuild(audiobook_schema),
];

/// The old and new kind constraint of `item`.
const KINDS_7: &str = "CHECK (kind IN ('movie','show','season','episode','video'))";
const KINDS_8: &str =
    "CHECK (kind IN ('movie','show','season','episode','video','artist','album','track'))";

fn music_schema(tx: &Transaction) -> crate::Result<()> {
    // `item` as it is now (columns added by earlier migrations included), with the new kinds.
    let sql: String = tx.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'item'",
        [],
        |r| r.get(0),
    )?;
    if !sql.contains(KINDS_7) {
        return Err(crate::DbError::Other(format!(
            "unexpected item table, not migrating: {sql}"
        )));
    }
    let new_sql = sql
        .replacen(KINDS_7, KINDS_8, 1)
        .replacen("CREATE TABLE item ", "CREATE TABLE item_new ", 1)
        .replacen("CREATE TABLE \"item\"", "CREATE TABLE item_new", 1);
    if !new_sql.contains("item_new") {
        return Err(crate::DbError::Other(format!("cannot rename item: {sql}")));
    }
    let indexes: Vec<String> = {
        let mut st = tx.prepare(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND tbl_name = 'item' AND sql IS NOT NULL",
        )?;
        st.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    tx.execute_batch(&new_sql)?;
    tx.execute_batch(
        "INSERT INTO item_new SELECT * FROM item;
         DROP TABLE item;
         ALTER TABLE item_new RENAME TO item;",
    )?;
    for ix in &indexes {
        tx.execute_batch(ix)?;
    }
    tx.execute_batch(MUSIC_TABLES)?;
    Ok(())
}

const MUSIC_TABLES: &str = r#"
ALTER TABLE source ADD COLUMN content TEXT NOT NULL DEFAULT 'video' CHECK (content IN ('video','music'));

CREATE TABLE music      (item_id INTEGER PRIMARY KEY REFERENCES item(id) ON DELETE CASCADE,
                         disc_no INTEGER, disc_total INTEGER, track_no INTEGER, track_total INTEGER,
                         release_date TEXT,
                         release_type TEXT,
                         compilation INTEGER NOT NULL DEFAULT 0,
                         genre TEXT,
                         rg_track_gain REAL, rg_track_peak REAL, rg_album_gain REAL, rg_album_peak REAL);
CREATE TABLE credit     (item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         artist_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         role TEXT NOT NULL CHECK (role IN ('artist','album_artist')),
                         position INTEGER NOT NULL,
                         join_phrase TEXT NOT NULL DEFAULT '',
                         PRIMARY KEY (item_id, role, position));
CREATE INDEX credit_artist ON credit(artist_id, role);
CREATE TABLE external_id(item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         provider TEXT NOT NULL,
                         kind TEXT NOT NULL,
                         value TEXT NOT NULL,
                         PRIMARY KEY (provider, kind, value, item_id));
CREATE INDEX external_id_item ON external_id(item_id);
CREATE TABLE file_audio (file_id INTEGER PRIMARY KEY REFERENCES file(id) ON DELETE CASCADE,
                         codec TEXT, sample_rate INTEGER, bit_depth INTEGER, channels INTEGER,
                         lossless INTEGER NOT NULL DEFAULT 0, bitrate INTEGER, duration_ms INTEGER);
CREATE TABLE play_queue (position INTEGER PRIMARY KEY,
                         item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         file_id INTEGER REFERENCES file(id) ON DELETE SET NULL,
                         shuffled_position INTEGER);
ALTER TABLE playstate ADD COLUMN play_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE playstate ADD COLUMN last_played_at INTEGER;
"#;

fn music_versions(tx: &Transaction) -> crate::Result<()> {
    crate::versions::migration_9(tx)
}

const KINDS_10: &str = "CHECK (kind IN ('movie','show','season','episode','video','artist','album','track','audiobook','person','book_series'))";
const CONTENT_8: &str = "CHECK (content IN ('video','music'))";
const CONTENT_10: &str = "CHECK (content IN ('video','music','audiobooks'))";

/// SQLite's twelve-step table rebuild for a changed CHECK: `table` is recreated from its own
/// current SQL with `from` replaced by `to`, its rows copied with their ids, and its indexes
/// recreated. Runs with foreign keys off (a `Rebuild` migration).
fn rebuild_with(tx: &Transaction, table: &str, from: &str, to: &str) -> crate::Result<()> {
    let sql: String = tx.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |r| r.get(0),
    )?;
    if !sql.contains(from) {
        return Err(crate::DbError::Other(format!(
            "unexpected {table} table, not migrating: {sql}"
        )));
    }
    let tmp = format!("{table}_new");
    let new_sql = sql
        .replacen(from, to, 1)
        .replacen(
            &format!("CREATE TABLE {table} "),
            &format!("CREATE TABLE {tmp} "),
            1,
        )
        .replacen(
            &format!("CREATE TABLE \"{table}\""),
            &format!("CREATE TABLE {tmp}"),
            1,
        );
    if !new_sql.contains(&tmp) {
        return Err(crate::DbError::Other(format!(
            "cannot rename {table}: {sql}"
        )));
    }
    let indexes: Vec<String> = {
        let mut st = tx.prepare(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND tbl_name = ?1 AND sql IS NOT NULL",
        )?;
        st.query_map([table], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    tx.execute_batch(&new_sql)?;
    tx.execute_batch(&format!(
        "INSERT INTO {tmp} SELECT * FROM {table}; DROP TABLE {table}; ALTER TABLE {tmp} RENAME TO {table};"
    ))?;
    for ix in &indexes {
        tx.execute_batch(ix)?;
    }
    Ok(())
}

fn audiobook_schema(tx: &Transaction) -> crate::Result<()> {
    rebuild_with(tx, "item", KINDS_8, KINDS_10)?;
    rebuild_with(tx, "source", CONTENT_8, CONTENT_10)?;
    tx.execute_batch(AUDIOBOOK_TABLES)?;
    Ok(())
}

const AUDIOBOOK_TABLES: &str = r#"
CREATE TABLE book       (item_id INTEGER PRIMARY KEY REFERENCES item(id) ON DELETE CASCADE,
                         key TEXT NOT NULL,
                         series_position REAL,
                         publisher TEXT, published TEXT,
                         language TEXT,
                         isbn TEXT, asin TEXT,
                         duration_ms INTEGER NOT NULL DEFAULT 0,
                         cover_ratio REAL,
                         speed REAL,
                         locked TEXT NOT NULL DEFAULT '[]',
                         drm INTEGER NOT NULL DEFAULT 0);
CREATE UNIQUE INDEX book_key ON book(key);
CREATE TABLE book_credit(book_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         person_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         role TEXT NOT NULL CHECK (role IN ('author','narrator')),
                         position INTEGER NOT NULL,
                         PRIMARY KEY (book_id, role, position));
CREATE INDEX book_credit_person ON book_credit(person_id, role);
CREATE TABLE book_file  (file_id INTEGER PRIMARY KEY REFERENCES file(id) ON DELETE CASCADE,
                         book_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         idx INTEGER NOT NULL,
                         start_ms INTEGER NOT NULL, duration_ms INTEGER NOT NULL);
CREATE INDEX book_file_book ON book_file(book_id, idx);
CREATE TABLE chapter    (book_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
                         idx INTEGER NOT NULL,
                         title TEXT NOT NULL,
                         start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,
                         file_id INTEGER REFERENCES file(id) ON DELETE SET NULL,
                         PRIMARY KEY (book_id, idx));
"#;
