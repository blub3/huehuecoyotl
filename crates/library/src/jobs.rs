//! The persisted job queue (SPEC 7): every scan and metadata step is a row in `job`, so work is
//! resumable after a restart, deduplicated, retried with backoff and prioritised.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

pub const MAX_ATTEMPTS: i64 = 5;

/// Priorities: higher runs first. Items on screen get bumped to `VISIBLE`.
pub mod priority {
    pub const BACKGROUND: i64 = 0;
    pub const NORMAL: i64 = 10;
    pub const USER: i64 = 50;
    pub const VISIBLE: i64 = 80;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub id: i64,
    pub kind: String,
    pub payload: String,
    pub priority: i64,
    pub attempts: i64,
}

fn now_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Queue a job. With a `dedupe` key, an identical queued or running job is not duplicated;
/// its priority is raised instead. Returns true if a new row was created.
pub fn enqueue(
    c: &Connection,
    kind: &str,
    payload: &str,
    prio: i64,
    dedupe: Option<&str>,
) -> Result<bool> {
    if let Some(key) = dedupe {
        let n = c.execute(
            "UPDATE job SET priority = max(priority, ?2) WHERE dedupe = ?1 AND state IN ('queued','running')",
            params![key, prio],
        )?;
        if n > 0 {
            return Ok(false);
        }
    }
    c.execute(
        "INSERT INTO job(kind, payload_json, priority, state, attempts, run_after, dedupe)
         VALUES (?1, ?2, ?3, 'queued', 0, 0, ?4)",
        params![kind, payload, prio, dedupe],
    )?;
    Ok(true)
}

/// Take the next runnable job whose kind is in `kinds`, marking it running.
pub fn claim(c: &mut Connection, kinds: &[&str]) -> Result<Option<Job>> {
    let tx = c.transaction()?;
    let placeholders = vec!["?"; kinds.len()].join(",");
    let sql = format!(
        "SELECT id, kind, payload_json, priority, attempts FROM job
         WHERE state = 'queued' AND run_after <= ? AND kind IN ({placeholders})
         ORDER BY priority DESC, id LIMIT 1"
    );
    let mut args: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(kinds.len() + 1);
    let now = now_s();
    args.push(&now);
    for k in kinds {
        args.push(k);
    }
    let job = tx
        .query_row(&sql, args.as_slice(), |r| {
            Ok(Job {
                id: r.get(0)?,
                kind: r.get(1)?,
                payload: r.get(2)?,
                priority: r.get(3)?,
                attempts: r.get(4)?,
            })
        })
        .optional()?;
    if let Some(j) = &job {
        tx.execute(
            "UPDATE job SET state = 'running', attempts = attempts + 1 WHERE id = ?1",
            [j.id],
        )?;
    }
    tx.commit()?;
    Ok(job.map(|mut j| {
        j.attempts += 1;
        j
    }))
}

pub fn complete(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM job WHERE id = ?1", [id])?;
    Ok(())
}

/// Retry later with exponential backoff (30 s, 2 min, 8 min, ...), or park as failed.
/// `retry_after` overrides the backoff (e.g. from an HTTP 429).
pub fn fail(c: &Connection, id: i64, error: &str, retry_after: Option<i64>) -> Result<()> {
    let attempts: i64 = c
        .query_row("SELECT attempts FROM job WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .unwrap_or(0);
    if attempts >= MAX_ATTEMPTS {
        c.execute(
            "UPDATE job SET state = 'failed', last_error = ?2 WHERE id = ?1",
            params![id, error],
        )?;
    } else {
        let delay = retry_after.unwrap_or(30 * 4i64.pow((attempts.max(1) - 1) as u32));
        c.execute(
            "UPDATE job SET state = 'queued', run_after = ?2, last_error = ?3 WHERE id = ?1",
            params![id, now_s() + delay, error],
        )?;
    }
    Ok(())
}

/// Put a job back without counting the attempt (e.g. no API key yet).
pub fn defer(c: &Connection, id: i64, seconds: i64) -> Result<()> {
    c.execute(
        "UPDATE job SET state = 'queued', attempts = max(0, attempts - 1), run_after = ?2 WHERE id = ?1",
        params![id, now_s() + seconds],
    )?;
    Ok(())
}

/// After a crash, running jobs are runnable again.
pub fn recover(c: &Connection) -> Result<usize> {
    Ok(c.execute(
        "UPDATE job SET state = 'queued' WHERE state = 'running'",
        [],
    )?)
}

/// Make deferred jobs of these kinds runnable now (e.g. after an API key was entered).
pub fn wake(c: &Connection, kinds: &[&str]) -> Result<usize> {
    let mut n = 0;
    for k in kinds {
        n += c.execute(
            "UPDATE job SET run_after = 0 WHERE state = 'queued' AND kind = ?1",
            [k],
        )?;
    }
    Ok(n)
}

/// Raise the priority of queued jobs for these items (items on screen are processed first).
pub fn bump_items(c: &Connection, item_ids: &[i64]) -> Result<()> {
    let mut st = c.prepare(
        "UPDATE job SET priority = max(priority, ?2)
         WHERE state = 'queued' AND json_extract(payload_json, '$.item_id') = ?1",
    )?;
    for id in item_ids {
        st.execute(params![id, priority::VISIBLE])?;
    }
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueueStats {
    pub queued: i64,
    pub running: i64,
    pub failed: i64,
}

pub fn stats(c: &Connection) -> Result<QueueStats> {
    Ok(c.query_row(
        "SELECT sum(state='queued'), sum(state='running'), sum(state='failed') FROM job",
        [],
        |r| {
            Ok(QueueStats {
                queued: r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                running: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                failed: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
            })
        },
    )?)
}

/// Like [`stats`], restricted to some job kinds (e.g. only what can run right now).
pub fn stats_for(c: &Connection, kinds: &[&str]) -> Result<QueueStats> {
    let placeholders = vec!["?"; kinds.len()].join(",");
    let sql = format!(
        "SELECT sum(state='queued'), sum(state='running'), sum(state='failed') FROM job WHERE kind IN ({placeholders})"
    );
    let args: Vec<&dyn rusqlite::ToSql> = kinds.iter().map(|k| k as &dyn rusqlite::ToSql).collect();
    Ok(c.query_row(&sql, args.as_slice(), |r| {
        Ok(QueueStats {
            queued: r.get::<_, Option<i64>>(0)?.unwrap_or(0),
            running: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
            failed: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
        })
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::migrate(&mut c).unwrap();
        c
    }

    #[test]
    fn priority_dedupe_and_retry() {
        let mut c = db();
        assert!(
            enqueue(
                &c,
                "match",
                r#"{"item_id":1}"#,
                priority::NORMAL,
                Some("match:1")
            )
            .unwrap()
        );
        assert!(
            enqueue(
                &c,
                "match",
                r#"{"item_id":2}"#,
                priority::NORMAL,
                Some("match:2")
            )
            .unwrap()
        );
        assert!(
            !enqueue(
                &c,
                "match",
                r#"{"item_id":1}"#,
                priority::USER,
                Some("match:1")
            )
            .unwrap()
        );
        bump_items(&c, &[2]).unwrap();
        // item 2 is visible now and outranks item 1's USER priority
        let j = claim(&mut c, &["match"]).unwrap().unwrap();
        assert_eq!(j.payload, r#"{"item_id":2}"#);
        assert_eq!(j.attempts, 1);
        assert!(claim(&mut c, &["scan"]).unwrap().is_none(), "kind filter");
        fail(&c, j.id, "boom", None).unwrap();
        // backed off: only item 1 is runnable now
        let j1 = claim(&mut c, &["match"]).unwrap().unwrap();
        assert_eq!(j1.payload, r#"{"item_id":1}"#);
        assert!(claim(&mut c, &["match"]).unwrap().is_none());
        complete(&c, j1.id).unwrap();
        wake(&c, &["match"]).unwrap();
        let again = claim(&mut c, &["match"]).unwrap().unwrap();
        assert_eq!(again.attempts, 2);
        assert_eq!(
            stats(&c).unwrap(),
            QueueStats {
                queued: 0,
                running: 1,
                failed: 0
            }
        );
        assert_eq!(recover(&c).unwrap(), 1);
    }

    #[test]
    fn gives_up_after_max_attempts() {
        let mut c = db();
        enqueue(&c, "x", "{}", 0, None).unwrap();
        for _ in 0..MAX_ATTEMPTS {
            wake(&c, &["x"]).unwrap();
            let j = claim(&mut c, &["x"]).unwrap().unwrap();
            fail(&c, j.id, "nope", None).unwrap();
        }
        assert_eq!(stats(&c).unwrap().failed, 1);
    }
}
