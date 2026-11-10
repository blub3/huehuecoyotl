//! Numbered schema migrations (SPEC 7). Index `i` brings `user_version` from `i` to `i + 1`.
//! Never edit a migration that has been released; add a new one.

pub const MIGRATIONS: &[&str] = &[
    // 1: initial schema
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
];
