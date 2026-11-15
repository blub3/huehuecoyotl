//! Media probe jobs (issue #33): every library file is analysed once in the background with
//! FFmpeg's libavformat (crates/probe), reading through the same sources as playback, so network
//! shares work like local folders. Only the start of a file is read. The result is stored as
//! `file.probe_json`; a changed file is probed again after the next scan.

use std::sync::Arc;

use huehuecoyotl_library::{catalog, jobs};
use huehuecoyotl_probe::{ProbeError, ProbeInfo, ReadAt, SCHEMA};
use huehuecoyotl_vfs::{Source, VPath};
use serde_json::json;

use crate::{Event, Inner};

pub const PROBE_KINDS: &[&str] = &["probe"];

/// Queue every file that has no probe data yet (low priority; already queued files are skipped).
pub(crate) async fn enqueue_unprobed(inner: &Arc<Inner>) {
    if let Err(e) = huehuecoyotl_probe::ffmpeg() {
        // Builds against a system libmpv of another FFmpeg version: playback fills in instead.
        tracing::info!("media probe unavailable: {e}");
        return;
    }
    let r = inner
        .lib
        .write(|c| {
            let ids = catalog::files_to_probe(c, SCHEMA)?;
            for id in &ids {
                jobs::enqueue(
                    c,
                    "probe",
                    &json!({ "file_id": id }).to_string(),
                    jobs::priority::BACKGROUND,
                    Some(&format!("probe:{id}")),
                )?;
            }
            Ok(ids.len())
        })
        .await;
    match r {
        Ok(0) => {}
        Ok(n) => {
            tracing::info!(files = n, "media probe queued");
            inner.wake.notify_waiters();
        }
        Err(e) => tracing::error!("queueing media probes failed: {e}"),
    }
}

/// "Analyse now": the file's probe goes to the front of the queue.
pub(crate) async fn probe_now(inner: &Arc<Inner>, file_id: i64) {
    let r = inner
        .lib
        .write(move |c| {
            c.execute("UPDATE file SET probe_json = NULL WHERE id = ?1", [file_id])?;
            jobs::enqueue(
                c,
                "probe",
                &json!({ "file_id": file_id }).to_string(),
                jobs::priority::USER,
                Some(&format!("probe:{file_id}")),
            )
        })
        .await;
    if let Err(e) = r {
        tracing::error!(file_id, "queueing a probe failed: {e}");
    }
    inner.wake.notify_waiters();
}

/// Reads through a [`Source`] from a blocking thread.
struct SourceReader {
    src: Arc<dyn Source>,
    path: VPath,
    rt: tokio::runtime::Handle,
}

impl ReadAt for SourceReader {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = buf.len().min(u32::MAX as usize) as u32;
        let b = self
            .rt
            .block_on(self.src.read_at(&self.path, offset, len))
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let n = b.len().min(buf.len());
        buf[..n].copy_from_slice(&b[..n]);
        Ok(n)
    }
}

/// The `probe` job. A file FFmpeg cannot read gets an entry with the error (not retried until
/// it changes); a source that cannot be reached is retried later.
pub(crate) async fn run(inner: &Arc<Inner>, file_id: i64) -> Result<(), String> {
    let target = inner
        .lib
        .read(move |c| catalog::probe_target(c, file_id))
        .await
        .map_err(|e| e.to_string())?;
    let Some((source_id, kind, path, size, _)) = target else {
        return Ok(()); // the file is gone
    };
    // Playback reads outrank the probe's (SPEC 6, connection handling).
    let src = inner
        .background_source(source_id, &kind)
        .await
        .map_err(|e| e.to_string())?;
    let name = path.rsplit('/').next().unwrap_or(&path).to_string();
    let mut reader = SourceReader {
        src,
        path: VPath::new(&path),
        rt: tokio::runtime::Handle::current(),
    };
    let started = std::time::Instant::now();
    let result =
        tokio::task::spawn_blocking(move || huehuecoyotl_probe::probe(&mut reader, size, &name))
            .await
            .map_err(|e| e.to_string())?;
    let info = match result {
        Ok(p) => {
            tracing::debug!(
                file_id,
                bytes = p.bytes_read,
                ms = started.elapsed().as_millis() as u64,
                "probed"
            );
            p.info
        }
        Err(ProbeError::Io(e)) => return Err(e),
        Err(ProbeError::Unavailable(e)) => return Err(e.to_string()),
        Err(e @ ProbeError::Unreadable(_)) => ProbeInfo {
            schema: SCHEMA,
            source: "ffmpeg".into(),
            size,
            error: Some(e.to_string()),
            ..Default::default()
        },
    };
    let json = serde_json::to_string(&info).map_err(|e| e.to_string())?;
    let item = inner
        .lib
        .write(move |c| catalog::set_probe(c, file_id, &json))
        .await
        .map_err(|e| e.to_string())?;
    if let Some(item_id) = item {
        inner.sink.post(Event::ItemChanged { item_id });
    }
    Ok(())
}
