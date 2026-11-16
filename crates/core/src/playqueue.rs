//! The music play queue in the core (SPEC 5.7, issue #8). The core owns the queue and stores it
//! after every change; the shell's `MusicPlayer` only feeds mpv with the current entry and the
//! following one, which it gets as playback tickets (`Event::MusicTickets`), and reports when
//! mpv moved on to the following one (`queue_advanced`).
//!
//! Ticket JSON: `{action: "play"|"next"|"stop", startMs, current, next}`, where `current` and
//! `next` are `{ticket, itemId, url, isNetwork, entry}` or null. "play" loads `current` (and
//! queues `next` behind it); "next" replaces only what follows the current entry.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use huehuecoyotl_library::queue::{self, Queue, Repeat, Step};
use serde_json::{Value, json};

use crate::{CResult, Core, CoreError, ErrorKind, Event, Inner, prepare_item};

/// Tickets of the play queue come from their own range, apart from the video player's.
pub(crate) const FIRST_MUSIC_TICKET: u64 = 1 << 40;

/// How many entries before the current one the queue view gets, and how many in all.
const BEFORE: usize = 50;
const WINDOW: usize = 500;

#[derive(Default)]
pub(crate) struct QueueState {
    q: Queue,
    loaded: bool,
    /// The tickets handed out for the current entry and the following one (0: none).
    current: u64,
    following: u64,
}

pub(crate) enum Op {
    Play {
        items: Vec<i64>,
        start_item: i64,
        shuffle: bool,
    },
    Insert {
        items: Vec<i64>,
        next: bool,
    },
    Step {
        delta: i32,
        position_ms: u64,
    },
    Jump(usize),
    Remove(usize),
    Move(usize, usize),
    Mode {
        shuffle: bool,
        repeat: String,
    },
    Advanced(u64),
    /// Load the current entry where it was left; after the end of the queue (`after_end`),
    /// what follows it (added since), or the queue from its start.
    Resume {
        after_end: bool,
    },
    State,
    Clear,
}

enum Then {
    Play(u64),
    Following,
    Stop,
    Nothing,
}

fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

async fn state_json(inner: &Arc<Inner>, q: &Queue) -> CResult<String> {
    let snapshot = q.clone();
    let from = q.pos().saturating_sub(BEFORE);
    let entries = inner
        .lib
        .read(move |c| queue::entries(c, &snapshot, from, WINDOW))
        .await?;
    Ok(json!({
        "count": q.len(),
        "position": q.pos(),
        "shuffle": q.shuffle,
        "repeat": q.repeat.as_str(),
        "offset": from,
        "entries": entries,
    })
    .to_string())
}

async fn ticket_json(inner: &Arc<Inner>, item_id: i64) -> CResult<Value> {
    let id = inner.music_ticket.fetch_add(1, Ordering::Relaxed);
    let t = prepare_item(inner, item_id, None, id).await?;
    let entry = inner
        .lib
        .read(move |c| queue::entry(c, item_id))
        .await?
        .unwrap_or_default();
    Ok(json!({
        "ticket": t.ticket,
        "itemId": item_id,
        "url": t.url,
        "isNetwork": t.is_network,
        "entry": entry,
    }))
}

/// The ticket of the entry that follows the current one, or null (also when it cannot be
/// prepared: the queue then stops after the current track, and a later change tries again).
async fn following_json(inner: &Arc<Inner>, st: &mut QueueState) -> Value {
    st.following = 0;
    let Some(item) = st.q.following().and_then(|p| st.q.at(p)) else {
        return Value::Null;
    };
    match ticket_json(inner, item).await {
        Ok(v) => {
            st.following = v["ticket"].as_u64().unwrap_or(0);
            v
        }
        Err(e) => {
            tracing::warn!(item, "preparing the next track failed: {}", e.message);
            Value::Null
        }
    }
}

async fn run(inner: &Arc<Inner>, st: &mut QueueState, op: Op, request_id: u64) -> CResult<()> {
    if !st.loaded {
        st.q = inner.lib.read(queue::load).await?;
        st.loaded = true;
    }
    let expand = |items: Vec<i64>| {
        let inner = inner.clone();
        async move { inner.lib.read(move |c| queue::expand(c, &items)).await }
    };
    let mut save = true;
    let then = match op {
        Op::Play {
            items,
            start_item,
            shuffle,
        } => {
            let tracks = expand(items).await?;
            if tracks.is_empty() {
                return Err(CoreError::new(ErrorKind::NotFound, "nothing to play"));
            }
            let start = match tracks.iter().position(|&t| t == start_item) {
                Some(i) => i,
                None if shuffle => (seed() % tracks.len() as u64) as usize,
                None => 0,
            };
            st.q.play(tracks, start, shuffle, seed());
            Then::Play(0)
        }
        Op::Insert { items, next } => {
            let tracks = expand(items).await?;
            let was_empty = st.q.is_empty();
            st.q.insert(&tracks, next);
            if was_empty && !st.q.is_empty() {
                Then::Play(0)
            } else {
                Then::Following
            }
        }
        Op::Step { delta, position_ms } => match st.q.step(delta, position_ms) {
            Step::Play => Then::Play(0),
            Step::None => Then::Nothing,
        },
        Op::Jump(pos) => match st.q.jump(pos) {
            Step::Play => Then::Play(0),
            Step::None => Then::Nothing,
        },
        Op::Remove(pos) => {
            if !st.q.remove(pos) {
                Then::Following
            } else if st.q.is_empty() {
                Then::Stop
            } else if st.current == 0 {
                Then::Nothing // nothing loaded: the next "play" starts the new current one
            } else {
                Then::Play(0)
            }
        }
        Op::Move(from, to) => {
            st.q.move_entry(from, to);
            Then::Following
        }
        Op::Mode { shuffle, repeat } => {
            st.q.set_shuffle(shuffle, seed());
            st.q.repeat = Repeat::parse(&repeat);
            Then::Following
        }
        Op::Advanced(ticket) => {
            // Only the ticket handed out as the following entry moves the queue on; a late
            // report after the user skipped elsewhere is ignored.
            if ticket == 0 || ticket != st.following {
                return Ok(());
            }
            st.q.advance();
            st.current = ticket;
            st.following = 0;
            inner.lib.write(|c| queue::save_position(c, 0)).await?;
            Then::Following
        }
        Op::Resume { after_end } => {
            if st.q.is_empty() {
                save = false;
                Then::Nothing
            } else if after_end {
                let next = st.q.following().unwrap_or(0);
                st.q.jump(next);
                Then::Play(0)
            } else {
                save = false;
                Then::Play(inner.lib.read(queue::position).await.unwrap_or(0))
            }
        }
        Op::State => {
            save = false;
            Then::Nothing
        }
        Op::Clear => {
            st.q.clear();
            Then::Stop
        }
    };
    if save {
        let q = st.q.clone();
        inner.lib.write(move |c| queue::save(c, &q)).await?;
    }
    inner.sink.post(Event::QueueChanged {
        request_id,
        json: state_json(inner, &st.q).await?,
    });
    let tickets = match then {
        Then::Play(start_ms) => {
            let Some(item) = st.q.current() else {
                return Ok(());
            };
            let current = ticket_json(inner, item).await?;
            st.current = current["ticket"].as_u64().unwrap_or(0);
            if start_ms == 0 {
                inner.lib.write(|c| queue::save_position(c, 0)).await?;
            }
            let next = following_json(inner, st).await;
            json!({ "action": "play", "startMs": start_ms, "current": current, "next": next })
        }
        Then::Following if st.current != 0 => {
            let next = following_json(inner, st).await;
            json!({ "action": "next", "next": next })
        }
        Then::Stop => {
            st.current = 0;
            st.following = 0;
            json!({ "action": "stop" })
        }
        _ => return Ok(()),
    };
    inner.sink.post(Event::MusicTickets {
        json: tickets.to_string(),
    });
    Ok(())
}

impl Core {
    /// Run one queue operation. One task runs them one at a time, in call order, and owns the
    /// queue.
    pub(crate) fn queue_op(&self, request_id: u64, op: Op) {
        let mut tx = self.inner.queue_tx.lock().unwrap();
        if tx.is_none() {
            let (send, mut recv) = tokio::sync::mpsc::unbounded_channel::<(u64, Op)>();
            let inner = self.inner.clone();
            self.rt.spawn(async move {
                let mut st = QueueState::default();
                while let Some((request_id, op)) = recv.recv().await {
                    if let Err(e) = run(&inner, &mut st, op, request_id).await {
                        inner.sink.post(Event::Error {
                            request_id,
                            kind: e.kind,
                            message: e.message,
                        });
                    }
                }
            });
            *tx = Some(send);
        }
        if let Some(tx) = tx.as_ref() {
            let _ = tx.send((request_id, op));
        }
    }

    /// Play items (tracks, albums, artists) from `start_item` (0: the first, or a random one
    /// with shuffle).
    pub fn queue_play(&self, request_id: u64, items: Vec<i64>, start_item: i64, shuffle: bool) {
        self.queue_op(
            request_id,
            Op::Play {
                items,
                start_item,
                shuffle,
            },
        );
    }
    /// "Play next" (`next`) or "add to queue".
    pub fn queue_insert(&self, request_id: u64, items: Vec<i64>, next: bool) {
        self.queue_op(request_id, Op::Insert { items, next });
    }
    /// Next (`delta` 1) or previous (-1) as the user asks.
    pub fn queue_step(&self, request_id: u64, delta: i32, position_ms: u64) {
        self.queue_op(request_id, Op::Step { delta, position_ms });
    }
    pub fn queue_jump(&self, request_id: u64, position: u32) {
        self.queue_op(request_id, Op::Jump(position as usize));
    }
    pub fn queue_remove(&self, request_id: u64, position: u32) {
        self.queue_op(request_id, Op::Remove(position as usize));
    }
    pub fn queue_move(&self, request_id: u64, from: u32, to: u32) {
        self.queue_op(request_id, Op::Move(from as usize, to as usize));
    }
    /// Shuffle on or off, repeat "off", "one" or "all".
    pub fn queue_set_mode(&self, request_id: u64, shuffle: bool, repeat: &str) {
        self.queue_op(
            request_id,
            Op::Mode {
                shuffle,
                repeat: repeat.to_string(),
            },
        );
    }
    /// mpv went on to the following entry (by itself, at the end of the current one).
    pub fn queue_advanced(&self, ticket: u64) {
        self.queue_op(0, Op::Advanced(ticket));
    }
    /// Load the current entry where it was left (after a restart); after the end of the queue,
    /// go on with what was added since, or start the queue over.
    pub fn queue_resume(&self, request_id: u64, after_end: bool) {
        self.queue_op(request_id, Op::Resume { after_end });
    }
    pub fn queue_state(&self, request_id: u64) {
        self.queue_op(request_id, Op::State);
    }
    pub fn queue_clear(&self, request_id: u64) {
        self.queue_op(request_id, Op::Clear);
    }

    /// Progress of a queue ticket: keeps the position for a restart and counts the track as
    /// played at half its length or after four minutes (once per ticket).
    pub fn music_progress(&self, ticket: u64, position_ms: u64, duration_ms: u64) {
        let Some(item_id) = self.inner.tickets.lock().unwrap().get(&ticket).copied() else {
            return;
        };
        let count = queue::counts_as_played(position_ms, duration_ms)
            && self.inner.counted.lock().unwrap().insert(ticket);
        let inner = self.inner.clone();
        // Played to the end: the next start is from the beginning.
        let keep = if duration_ms > 0 && position_ms + 1_000 >= duration_ms {
            0
        } else {
            position_ms
        };
        let fut = self.inner.lib.submit(move |c| {
            queue::save_position(c, keep)?;
            if count {
                queue::record_play(c, item_id, now_ms())?;
            }
            Ok(count)
        });
        self.rt.spawn(async move {
            match fut.await {
                Ok(true) => inner.sink.post(Event::ItemChanged { item_id }),
                Ok(false) => {}
                Err(e) => tracing::error!(item_id, "saving music progress failed: {e}"),
            }
        });
    }
}
