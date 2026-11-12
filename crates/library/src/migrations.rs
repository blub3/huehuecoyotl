//! Numbered schema migrations (SPEC 7). Index `i` brings `user_version` from `i` to `i + 1`.
//! Never edit a migration that has been released; add a new one. A migration is SQL, or a Rust
//! function for repairs that need logic; each runs in its own transaction.

use rusqlite::Transaction;

pub enum Migration {
    Sql(&'static str),
    Rust(fn(&Transaction) -> crate::Result<()>),
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
];
