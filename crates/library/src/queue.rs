//! The music play queue (SPEC 5.7, issue #8): its order, shuffle, repeat, "play next" and "add
//! to queue", stored in `play_queue` and `setting` so it survives a restart. The logic is plain
//! data without I/O; the core holds one `Queue` and saves it after every change.

use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;
use crate::copies::{set_setting, setting};
use crate::music::{art, credit_line, hires};
use crate::queries::DEFAULT_PROFILE;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    One,
    All,
}

impl Repeat {
    pub fn as_str(self) -> &'static str {
        match self {
            Repeat::Off => "off",
            Repeat::One => "one",
            Repeat::All => "all",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "one" => Repeat::One,
            "all" => Repeat::All,
            _ => Repeat::Off,
        }
    }
}

/// Track item ids in their original order (`items`), the play order as indexes into them
/// (`order`: the identity unless shuffled, so turning shuffle off returns to the original
/// order), and the position in the play order.
/// A queue entry: the track item and the file to play (issue #63: the version chosen when it was
/// queued; `None` lets the player choose).
pub type Track = (i64, Option<i64>);

/// Tracks without a chosen file.
pub fn tracks(ids: &[i64]) -> Vec<Track> {
    ids.iter().map(|&i| (i, None)).collect()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Queue {
    items: Vec<Track>,
    order: Vec<usize>,
    pos: usize,
    pub shuffle: bool,
    pub repeat: Repeat,
}

/// A small xorshift generator: shuffling needs no cryptographic quality, and tests pass a seed.
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

fn shuffled(mut v: Vec<usize>, seed: u64) -> Vec<usize> {
    let mut rng = Rng::new(seed);
    for i in (1..v.len()).rev() {
        let j = rng.below(i + 1);
        v.swap(i, j);
    }
    v
}

/// What `step` and `jump` ask the player to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Start the (new) current entry from the beginning.
    Play,
    /// Nothing to go to: the queue stays where it is.
    None,
}

/// "Previous" within this many milliseconds of a track's start goes to the track before;
/// later it restarts the track.
pub const RESTART_MS: u64 = 3_000;

impl Queue {
    pub fn len(&self) -> usize {
        self.order.len()
    }
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
    /// The position in the play order.
    pub fn pos(&self) -> usize {
        self.pos
    }
    pub fn current(&self) -> Option<i64> {
        self.at(self.pos)
    }
    /// The item at a position of the play order.
    pub fn at(&self, pos: usize) -> Option<i64> {
        self.entry(pos).map(|t| t.0)
    }
    /// The entry (item and file) at a position of the play order.
    pub fn entry(&self, pos: usize) -> Option<Track> {
        self.order.get(pos).map(|&i| self.items[i])
    }
    /// Every entry in play order.
    pub fn entries(&self) -> Vec<Track> {
        self.order.iter().map(|&i| self.items[i]).collect()
    }
    /// Every item in play order.
    pub fn play_order(&self) -> Vec<i64> {
        self.order.iter().map(|&i| self.items[i].0).collect()
    }

    /// Replace the queue: `items` in their order, starting at `start`. With shuffle the start
    /// item plays first and the rest follow in random order. Repeat is kept.
    pub fn play(&mut self, items: Vec<Track>, start: usize, shuffle: bool, seed: u64) {
        let start = start.min(items.len().saturating_sub(1));
        self.items = items;
        self.shuffle = shuffle;
        self.reorder(start, seed);
    }

    /// Order the play order from scratch with `current` (an index into `items`) as the
    /// current entry.
    fn reorder(&mut self, current: usize, seed: u64) {
        let n = self.items.len();
        if n == 0 {
            self.order.clear();
            self.pos = 0;
            return;
        }
        if self.shuffle {
            let rest = shuffled((0..n).filter(|&i| i != current).collect(), seed);
            self.order = std::iter::once(current).chain(rest).collect();
            self.pos = 0;
        } else {
            self.order = (0..n).collect();
            self.pos = current;
        }
    }

    /// The entry that plays when the current one ends by itself: the same with repeat one,
    /// the first after the last with repeat all.
    pub fn following(&self) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        match self.repeat {
            Repeat::One => Some(self.pos),
            _ if self.pos + 1 < self.len() => Some(self.pos + 1),
            Repeat::All => Some(0),
            Repeat::Off => None,
        }
    }

    /// The current entry ended and the player went on to `following()`.
    pub fn advance(&mut self) -> bool {
        match self.following() {
            Some(p) => {
                self.pos = p;
                true
            }
            None => false,
        }
    }

    /// Next (`delta` > 0) or previous (< 0) as the user asks: next always moves on (with
    /// repeat one too), wrapping only with repeat; previous restarts the track after
    /// `RESTART_MS` and at the first entry.
    pub fn step(&mut self, delta: i32, position_ms: u64) -> Step {
        if self.is_empty() {
            return Step::None;
        }
        if delta > 0 {
            if self.pos + 1 < self.len() {
                self.pos += 1;
            } else if self.repeat != Repeat::Off {
                self.pos = 0;
            } else {
                return Step::None;
            }
        } else if delta < 0 && position_ms <= RESTART_MS {
            if self.pos > 0 {
                self.pos -= 1;
            } else if self.repeat == Repeat::All {
                self.pos = self.len() - 1;
            }
        }
        Step::Play
    }

    pub fn jump(&mut self, pos: usize) -> Step {
        if pos >= self.len() {
            return Step::None;
        }
        self.pos = pos;
        Step::Play
    }

    /// "Play next" (`next`) or "add to queue". An empty queue starts with them.
    pub fn insert(&mut self, ids: &[Track], next: bool) {
        if ids.is_empty() {
            return;
        }
        if self.is_empty() {
            self.play(ids.to_vec(), 0, self.shuffle, 1);
            return;
        }
        let n = ids.len();
        // Where they go in the original order: after the current item, or at the end.
        let at_item = if next {
            self.order[self.pos] + 1
        } else {
            self.items.len()
        };
        let at_order = if next { self.pos + 1 } else { self.order.len() };
        self.items.splice(at_item..at_item, ids.iter().copied());
        for i in self.order.iter_mut() {
            if *i >= at_item {
                *i += n;
            }
        }
        self.order.splice(at_order..at_order, at_item..at_item + n);
    }

    /// Remove the entry at `pos` of the play order. Returns true when it was the current one;
    /// the entry after it is current then (or the last, when it was the last).
    pub fn remove(&mut self, pos: usize) -> bool {
        if pos >= self.len() {
            return false;
        }
        let idx = self.order.remove(pos);
        self.items.remove(idx);
        for i in self.order.iter_mut() {
            if *i > idx {
                *i -= 1;
            }
        }
        let was_current = pos == self.pos;
        if pos < self.pos {
            self.pos -= 1;
        }
        if self.pos >= self.len() {
            self.pos = self.len().saturating_sub(1);
        }
        was_current
    }

    /// Move the entry at `from` of the play order to `to`. Unshuffled the original order moves
    /// with it, so it stays the play order.
    pub fn move_entry(&mut self, from: usize, to: usize) {
        if from >= self.len() || to >= self.len() || from == to {
            return;
        }
        let current = self.order[self.pos];
        if self.shuffle {
            let e = self.order.remove(from);
            self.order.insert(to, e);
            self.pos = self.order.iter().position(|&i| i == current).unwrap_or(0);
        } else {
            let e = self.items.remove(from);
            self.items.insert(to, e);
            self.pos = if self.pos == from {
                to
            } else if from < self.pos && to >= self.pos {
                self.pos - 1
            } else if from > self.pos && to <= self.pos {
                self.pos + 1
            } else {
                self.pos
            };
        }
    }

    /// Turn shuffle on (the current entry stays and the rest is shuffled) or off (back to
    /// the original order, at the current entry).
    pub fn set_shuffle(&mut self, on: bool, seed: u64) {
        if on == self.shuffle {
            return;
        }
        self.shuffle = on;
        if let Some(&current) = self.order.get(self.pos) {
            self.reorder(current, seed);
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.pos = 0;
    }
}

// ---- storage ----------------------------------------------------------------------------------

const POS: &str = "queue_pos";
const CURRENT: &str = "queue_current";
const SHUFFLE: &str = "queue_shuffle";
const REPEAT: &str = "queue_repeat";
const POSITION_MS: &str = "queue_position_ms";

/// Store the whole queue (it replaces what was stored).
pub fn save(c: &mut Connection, q: &Queue) -> Result<()> {
    let tx = c.transaction()?;
    tx.execute("DELETE FROM play_queue", [])?;
    {
        let mut st = tx.prepare(
            "INSERT INTO play_queue(position, item_id, file_id, shuffled_position) VALUES (?1, ?2, ?3, ?4)",
        )?;
        let mut shuffled = vec![None; q.items.len()];
        if q.shuffle {
            for (p, &i) in q.order.iter().enumerate() {
                shuffled[i] = Some(p as i64);
            }
        }
        for (i, (id, file)) in q.items.iter().enumerate() {
            st.execute(params![i as i64, id, file, shuffled[i]])?;
        }
    }
    set_setting(&tx, POS, Some(&q.pos.to_string()))?;
    let current = q.current().map(|c| c.to_string());
    set_setting(&tx, CURRENT, current.as_deref())?;
    set_setting(&tx, SHUFFLE, Some(if q.shuffle { "1" } else { "0" }))?;
    set_setting(&tx, REPEAT, Some(q.repeat.as_str()))?;
    tx.commit()?;
    Ok(())
}

/// The stored queue. Tracks removed from the library since are gone from it (`ON DELETE
/// CASCADE`); the current entry is found again by its item.
pub fn load(c: &Connection) -> Result<Queue> {
    let mut st = c.prepare(
        "SELECT item_id, shuffled_position, file_id FROM play_queue
         WHERE item_id IN (SELECT id FROM item WHERE kind = 'track') ORDER BY position",
    )?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut q = Queue {
        shuffle: setting(c, SHUFFLE)?.as_deref() == Some("1"),
        repeat: Repeat::parse(setting(c, REPEAT)?.as_deref().unwrap_or("off")),
        ..Default::default()
    };
    q.items = rows.iter().map(|r| (r.0, r.2)).collect();
    q.order = (0..rows.len()).collect();
    if q.shuffle {
        q.order
            .sort_by_key(|&i| (rows[i].1.is_none(), rows[i].1.unwrap_or(0)));
    }
    let current: Option<i64> = setting(c, CURRENT)?.and_then(|v| v.parse().ok());
    let pos: usize = setting(c, POS)?.and_then(|v| v.parse().ok()).unwrap_or(0);
    q.pos = current
        .and_then(|id| q.order.iter().position(|&i| q.items[i].0 == id))
        .unwrap_or(pos.min(q.len().saturating_sub(1)));
    Ok(q)
}

/// Where in the current track playback is, for the restart.
pub fn save_position(c: &Connection, ms: u64) -> Result<()> {
    set_setting(c, POSITION_MS, Some(&ms.to_string()))
}

pub fn position(c: &Connection) -> Result<u64> {
    Ok(setting(c, POSITION_MS)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0))
}

// ---- play counts ------------------------------------------------------------------------------

/// A track counts as played at half its length or after four minutes, whichever comes first.
pub fn counts_as_played(position_ms: u64, duration_ms: u64) -> bool {
    duration_ms > 0 && position_ms >= (duration_ms / 2).min(240_000)
}

pub fn record_play(c: &Connection, item_id: i64, now_ms: i64) -> Result<()> {
    c.execute(
        "INSERT INTO playstate(item_id, profile_id, position_ms, duration_ms, watched, updated_at,
                               play_count, last_played_at)
         VALUES (?1, ?2, 0, 0, 0, ?3, 1, ?3)
         ON CONFLICT(item_id, profile_id) DO UPDATE SET play_count = play_count + 1,
           last_played_at = excluded.last_played_at, updated_at = excluded.updated_at",
        params![item_id, DEFAULT_PROFILE, now_ms],
    )?;
    Ok(())
}

// ---- what the queue holds ---------------------------------------------------------------------

/// Tracks to queue for items: a track itself, an album's tracks by disc and number, an
/// artist's albums by year with their tracks.
pub fn expand(
    c: &Connection,
    ids: &[i64],
    version: Option<i64>,
    preference: &str,
    offline: &HashSet<i64>,
) -> Result<Vec<Track>> {
    use crate::versions;
    // Issue #63: an album plays one version for all its tracks (gapless stays consistent): the
    // one asked for, else the one played last, else by preference.
    let album_version = |album: i64| -> Result<Option<i64>> {
        if let Some(v) = version {
            let belongs: bool = c.query_row(
                "SELECT EXISTS (SELECT 1 FROM album_version WHERE id = ?1 AND album_id = ?2)",
                params![v, album],
                |r| r.get(0),
            )?;
            if belongs {
                return Ok(Some(v));
            }
        }
        let mut vs = versions::versions(c, album)?;
        for v in &mut vs {
            v.online = !offline.contains(&v.source_id);
        }
        let choices: Vec<versions::Choice> = vs.iter().map(versions::Choice::from).collect();
        Ok(versions::preselect(
            &choices,
            versions::last_version(c, album)?,
            preference,
        ))
    };
    let tracks_of = |album: i64| -> Result<Vec<Track>> {
        if let Some(v) = album_version(album)? {
            return Ok(versions::version_tracks(c, v, preference)?
                .into_iter()
                .map(|(t, f)| (t, Some(f)))
                .collect());
        }
        // Without versions (a library before migration 9): every track, the player chooses.
        let mut st = c.prepare_cached(
            "SELECT t.id FROM item t WHERE t.parent_id = ?1 AND t.kind = 'track'
               AND EXISTS (SELECT 1 FROM file f WHERE f.item_id = t.id)
             ORDER BY coalesce(t.season_no, 1), coalesce(t.episode_no, 0), t.sort_title",
        )?;
        Ok(st
            .query_map([album], |r| Ok((r.get(0)?, None)))?
            .collect::<rusqlite::Result<_>>()?)
    };
    let mut out = Vec::new();
    for &id in ids {
        let row: Option<(String, Option<i64>)> = c
            .query_row(
                "SELECT kind, parent_id FROM item WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            Some((kind, parent)) if kind == "track" => {
                // A single track: its file in the album's version, when that version has it.
                let file = match parent.map(album_version).transpose()?.flatten() {
                    Some(v) => versions::track_file(c, id, v, preference)?,
                    None => None,
                };
                out.push((id, file));
            }
            Some((kind, _)) if kind == "album" => out.extend(tracks_of(id)?),
            Some((kind, _)) if kind == "artist" => {
                let mut st = c.prepare_cached(
                    "SELECT id FROM item WHERE parent_id = ?1 AND kind = 'album'
                     ORDER BY coalesce(year, 9999), sort_title",
                )?;
                let albums: Vec<i64> = st
                    .query_map([id], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?;
                for a in albums {
                    out.extend(tracks_of(a)?);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// A queue entry as the shell shows it (Now Playing, the queue, MPRIS).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Position in the play order.
    pub index: usize,
    pub id: i64,
    /// Issue #63: the file queued for it (0: the player chooses), and whether the player had to
    /// fall back to another version of the track (set by the core).
    pub file_id: i64,
    pub fallback: bool,
    pub title: String,
    pub artists: String,
    pub album: String,
    pub album_id: i64,
    pub artist_id: i64,
    pub duration_ms: i64,
    pub poster: String,
    pub poster_full: String,
    pub codec: String,
    pub sample_rate: i64,
    pub bit_depth: i64,
    pub lossless: bool,
    pub hires: bool,
}

/// An entry with the file it plays (`file`; else the track's largest).
pub fn entry(c: &Connection, id: i64, file: Option<i64>) -> Result<Option<Entry>> {
    let e = c
        .query_row(
            "SELECT coalesce(t.title, ''), coalesce(a.title, ''), coalesce(a.id, 0),
                    coalesce(a.parent_id, 0), coalesce(fa.duration_ms, 0), coalesce(fa.codec, ''),
                    coalesce(fa.sample_rate, 0), coalesce(fa.bit_depth, 0), coalesce(fa.lossless, 0)
             FROM item t LEFT JOIN item a ON a.id = t.parent_id
             LEFT JOIN file f ON f.id = coalesce(?2, (SELECT id FROM file WHERE item_id = t.id ORDER BY size DESC LIMIT 1))
             LEFT JOIN file_audio fa ON fa.file_id = f.id
             WHERE t.id = ?1 AND t.kind = 'track'",
            params![id, file],
            |r| {
                let lossless = r.get::<_, i64>(8)? != 0;
                let (rate, depth): (i64, i64) = (r.get(6)?, r.get(7)?);
                Ok(Entry {
                    id,
                    title: r.get(0)?,
                    album: r.get(1)?,
                    album_id: r.get(2)?,
                    artist_id: r.get(3)?,
                    duration_ms: r.get(4)?,
                    codec: r.get(5)?,
                    sample_rate: rate,
                    bit_depth: depth,
                    lossless,
                    hires: hires(lossless, Some(depth), Some(rate)),
                    ..Default::default()
                })
            },
        )
        .optional()?;
    let Some(mut e) = e else { return Ok(None) };
    e.artists = credit_line(c, id, "artist")?;
    if e.artists.is_empty() && e.artist_id != 0 {
        e.artists = c
            .query_row(
                "SELECT coalesce(title, '') FROM item WHERE id = ?1",
                [e.artist_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_default();
    }
    if e.album_id != 0 {
        e.poster = art(c, e.album_id, "poster_grid")?;
        e.poster_full = art(c, e.album_id, "poster")?;
    }
    Ok(Some(e))
}

/// The entries of the play order from `from` (at most `limit`).
pub fn entries(c: &Connection, q: &Queue, from: usize, limit: usize) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    for (index, (id, file)) in q.entries().into_iter().enumerate().skip(from).take(limit) {
        if let Some(mut e) = entry(c, id, file)? {
            e.index = index;
            e.file_id = file.unwrap_or(0);
            out.push(e);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(n: i64) -> Queue {
        let mut q = Queue::default();
        q.play(tracks(&(1..=n).collect::<Vec<_>>()), 0, false, 7);
        q
    }

    #[test]
    fn plays_in_order_and_stops_at_the_end() {
        let mut q = q(3);
        assert_eq!(q.current(), Some(1));
        assert!(q.advance());
        assert!(q.advance());
        assert_eq!(q.current(), Some(3));
        assert_eq!(q.following(), None);
        assert!(!q.advance());
        assert_eq!(q.step(1, 0), Step::None);
    }

    #[test]
    fn repeat_one_and_all() {
        let mut q = q(3);
        q.repeat = Repeat::One;
        assert_eq!(q.following(), Some(0));
        // Next by hand still moves on.
        assert_eq!(q.step(1, 0), Step::Play);
        assert_eq!(q.current(), Some(2));
        q.repeat = Repeat::All;
        q.jump(2);
        assert_eq!(q.following(), Some(0));
        assert_eq!(q.step(1, 0), Step::Play);
        assert_eq!(q.current(), Some(1));
        // Previous at the first entry wraps with repeat all.
        q.step(-1, 0);
        assert_eq!(q.current(), Some(3));
    }

    #[test]
    fn previous_restarts_a_track_that_played_a_while() {
        let mut q = q(3);
        q.jump(1);
        assert_eq!(q.step(-1, 10_000), Step::Play);
        assert_eq!(q.current(), Some(2));
        q.step(-1, 1_000);
        assert_eq!(q.current(), Some(1));
        assert_eq!(q.step(-1, 0), Step::Play); // restarts the first
        assert_eq!(q.current(), Some(1));
    }

    #[test]
    fn shuffle_starts_with_the_chosen_track_and_turns_back() {
        let mut q = Queue::default();
        q.play(tracks(&(1..=20).collect::<Vec<_>>()), 4, true, 42);
        assert_eq!(q.current(), Some(5));
        let mut all = q.play_order();
        assert_ne!(all, (1..=20).collect::<Vec<_>>());
        all.sort();
        assert_eq!(all, (1..=20).collect::<Vec<_>>());
        q.advance();
        let now = q.current().unwrap();
        q.set_shuffle(false, 0);
        assert_eq!(q.play_order(), (1..=20).collect::<Vec<_>>());
        assert_eq!(q.current(), Some(now));
        q.set_shuffle(true, 9);
        assert_eq!(q.current(), Some(now));
        assert_eq!(q.pos(), 0);
    }

    #[test]
    fn play_next_and_add_to_queue() {
        let mut q = q(3);
        q.jump(1);
        q.insert(&tracks(&[10, 11]), true);
        assert_eq!(q.play_order(), vec![1, 2, 10, 11, 3]);
        q.insert(&tracks(&[20]), false);
        assert_eq!(q.play_order(), vec![1, 2, 10, 11, 3, 20]);
        assert_eq!(q.current(), Some(2));
        // Shuffled: next after the current, and back in the original order without shuffle.
        q.set_shuffle(true, 3);
        q.insert(&tracks(&[30]), true);
        assert_eq!(q.at(q.pos() + 1), Some(30));
        q.set_shuffle(false, 0);
        assert_eq!(q.play_order(), vec![1, 2, 30, 10, 11, 3, 20]);
        // Into an empty queue: it starts with them.
        let mut e = Queue::default();
        e.insert(&tracks(&[5, 6]), false);
        assert_eq!(e.current(), Some(5));
    }

    #[test]
    fn remove_and_move() {
        let mut q = q(5);
        q.jump(2);
        assert!(!q.remove(0));
        assert_eq!(q.current(), Some(3));
        assert!(q.remove(1)); // the current one: the next is current
        assert_eq!(q.current(), Some(4));
        assert_eq!(q.play_order(), vec![2, 4, 5]);
        q.move_entry(2, 0);
        assert_eq!(q.play_order(), vec![5, 2, 4]);
        assert_eq!(q.current(), Some(4));
        q.move_entry(2, 0);
        assert_eq!(q.current(), Some(4));
        assert_eq!(q.pos(), 0);
        assert!(!q.remove(2));
        assert!(!q.remove(1));
        assert!(q.remove(0));
        assert!(q.is_empty());
    }

    #[test]
    fn plays_count_at_half_or_four_minutes() {
        assert!(!counts_as_played(59_000, 120_000));
        assert!(counts_as_played(60_000, 120_000));
        assert!(!counts_as_played(239_000, 1_200_000));
        assert!(counts_as_played(240_000, 1_200_000));
        assert!(!counts_as_played(10, 0));
    }

    #[test]
    fn saved_and_loaded() {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrate(&mut c).unwrap();
        let mut ids = Vec::new();
        for i in 0..6 {
            c.execute(
                "INSERT INTO item(kind, title, sort_title, added_at) VALUES ('track', ?1, ?1, 0)",
                [format!("t{i}")],
            )
            .unwrap();
            ids.push(c.last_insert_rowid());
        }
        // Issue #63: every entry with the file chosen for it.
        c.execute(
            "INSERT INTO source(id, kind, name) VALUES (2, 'local', 'Music')",
            [],
        )
        .unwrap();
        let mut entries = Vec::new();
        for (n, id) in ids.iter().enumerate() {
            c.execute(
                "INSERT INTO file(source_id, path, item_id) VALUES (2, ?1, ?2)",
                params![format!("{n}.flac"), id],
            )
            .unwrap();
            entries.push((*id, Some(c.last_insert_rowid())));
        }
        let mut q = Queue {
            repeat: Repeat::All,
            ..Default::default()
        };
        q.play(entries.clone(), 2, true, 5);
        q.advance();
        save(&mut c, &q).unwrap();
        save_position(&c, 12_345).unwrap();
        assert_eq!(load(&c).unwrap(), q);
        // The same files come back.
        let mut back = load(&c).unwrap().entries();
        back.sort();
        entries.sort();
        assert_eq!(back, entries);
        assert_eq!(position(&c).unwrap(), 12_345);
        // A track gone from the library leaves the queue; the current one is kept.
        let current = q.current().unwrap();
        let gone = ids.iter().copied().find(|&i| i != current).unwrap();
        c.execute("DELETE FROM item WHERE id = ?1", [gone]).unwrap();
        let l = load(&c).unwrap();
        assert_eq!(l.len(), 5);
        assert_eq!(l.current(), Some(current));
        assert!(l.shuffle);
        record_play(&c, current, 99).unwrap();
        record_play(&c, current, 100).unwrap();
        let (n, at): (i64, i64) = c
            .query_row(
                "SELECT play_count, last_played_at FROM playstate WHERE item_id = ?1",
                [current],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((n, at), (2, 100));
    }
}
