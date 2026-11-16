//! `coyotl-cli`: the headless harness (SPEC 2, principle 2). It drives the same core API as
//! the Qt shell, with a channel as its event sink.
//!
//!   coyotl-cli [--data-dir DIR] browse [PATH]
//!   coyotl-cli [--data-dir DIR] open PATH
//!   coyotl-cli [--data-dir DIR] progress PATH POSITION_S DURATION_S [--ended]
//!   coyotl-cli [--data-dir DIR] recent
//!   coyotl-cli [--data-dir DIR] library add PATH | library list | rescan
//!   coyotl-cli [--data-dir DIR] movies | shows | search TEXT | detail ID | review
//!   coyotl-cli [--data-dir DIR] key KEY        (validates and stores the TMDB key)
//!   coyotl-cli [--data-dir DIR] download ITEM [FILE] | downloads list | downloads delete ID
//!   coyotl-cli [--data-dir DIR] downloads pause ID | downloads resume ID | storage [DIR]
//!
//! Commands that start background work (library add, rescan) wait until the job queue is
//! empty, so the harness can scan and match a library headless.
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use crossbeam_channel::unbounded;
use huehuecoyotl_core::{Core, Event, PlayerState};

/// Same location Qt's AppDataLocation gives the app ("coyotl", no organisation).
fn default_data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("COYOTL_DATA_DIR") {
        return d.into();
    }
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    base.unwrap_or_else(|| ".".into()).join("coyotl")
}

/// Network share passwords come from the environment so they never show up in `ps` or history.
fn share_password() -> String {
    std::env::var("COYOTL_SHARE_PASSWORD").unwrap_or_default()
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: coyotl-cli [--data-dir DIR] browse [PATH] | open PATH | progress PATH POS_S DUR_S [--ended] | recent\n\
         \x20      | library add PATH | library add-share SPEC_JSON | library list | share-ls SPEC_JSON [PATH]\n\
         \x20      | discover | diagnostics FILE | rescan | movies | shows | search TEXT | detail ID | review | key KEY\n\
         \x20      | probe FILE (technical details as JSON; FFmpeg from COYOTL_FFMPEG_DIR or the system)\n\
         \x20      | download ITEM [FILE] (save offline copies and wait for them) | downloads list\n\
         \x20      | downloads delete|pause|resume ID | storage [DIR] (where saved copies go)\n\
         network shares: SPEC_JSON is {{\"protocol\":\"smb\",\"host\":\"nas\",\"share\":\"media\",\"user\":\"me\",\"path\":\"/Movies\"}};\n\
         the password comes from COYOTL_SHARE_PASSWORD (never from the command line)"
    );
    ExitCode::from(2)
}

/// `probe FILE`: what the media probe reads from a file, as JSON, and how many bytes it took.
fn probe(path: &Path) -> ExitCode {
    let size = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mut r = match std::fs::File::open(path) {
        Ok(f) => huehuecoyotl_probe::FileReader(f),
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    match huehuecoyotl_probe::probe(&mut r, size, &name) {
        Ok(p) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&p.info).unwrap_or_default()
            );
            eprintln!("read {} of {size} bytes", p.bytes_read);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// After `download`: print progress until no copy is queued or running, then the list.
fn follow_downloads(core: &Core, rx: &crossbeam_channel::Receiver<Event>) -> Event {
    let mut last_print = std::time::Instant::now();
    let mut req = 100;
    loop {
        match rx.recv_timeout(Duration::from_millis(1000)) {
            Ok(Event::DownloadProgress {
                id,
                main_id,
                state,
                bytes_done,
                size,
                ..
            }) if id == main_id => {
                if last_print.elapsed() >= Duration::from_secs(1) || state != "running" {
                    last_print = std::time::Instant::now();
                    let pct = (bytes_done * 100).checked_div(size).unwrap_or(100);
                    eprintln!("  copy {id}: {state} {pct}% ({bytes_done} of {size} bytes)");
                }
                continue;
            }
            Ok(Event::Downloads { items, .. })
                if items
                    .iter()
                    .any(|d| d.state == "queued" || d.state == "running") =>
            {
                std::thread::sleep(Duration::from_millis(500));
            }
            Ok(e @ Event::Downloads { .. }) => return e,
            Ok(_) | Err(_) => {}
        }
        req += 1;
        core.downloads(req);
    }
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let data_dir = match args.iter().position(|a| a == "--data-dir") {
        Some(i) if i + 1 < args.len() => {
            let d = PathBuf::from(args.remove(i + 1));
            args.remove(i);
            d
        }
        _ => default_data_dir(),
    };
    let Some(cmd) = args.first().cloned() else {
        return usage();
    };
    // The media probe needs no library: it reads one local file.
    if let ("probe", [file]) = (cmd.as_str(), &args[1..]) {
        return probe(Path::new(file));
    }

    let (tx, rx) = unbounded();
    let core = match Core::open(&data_dir, "en-US", move |e| {
        let _ = tx.send(e);
    }) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot open core: {e}");
            return ExitCode::FAILURE;
        }
    };
    let wait = || loop {
        // Background notifications are not answers; skip them.
        match rx
            .recv_timeout(Duration::from_secs(60))
            .expect("no answer from core")
        {
            Event::ItemChanged { .. }
            | Event::LibraryChanged { .. }
            | Event::ScanState { .. }
            | Event::DownloadProgress { .. } => {
                continue;
            }
            e => break e,
        }
    };
    // Wait until no scan runs, no job is queued, and nothing happened for two seconds.
    let settle = || {
        let mut running = 0i32;
        let mut queued = 1i64; // unknown until the first report
        let mut quiet = 0;
        let start = std::time::Instant::now();
        while quiet < 4 || running > 0 || (queued > 0 && start.elapsed() < Duration::from_secs(600))
        {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Event::LibraryChanged { queued: q, failed }) => {
                    queued = q;
                    quiet = 0;
                    eprintln!(
                        "  … {q} jobs left{}",
                        if failed > 0 {
                            format!(", {failed} failed")
                        } else {
                            String::new()
                        }
                    );
                }
                Ok(Event::ScanState {
                    running: r,
                    message,
                    ..
                }) => {
                    quiet = 0;
                    if r {
                        running += 1;
                    } else {
                        running -= 1;
                        eprintln!("  scan: {message}");
                    }
                }
                Ok(_) => quiet = 0,
                Err(_) => {
                    quiet += 1;
                    if quiet >= 4 && running <= 0 && queued > 0 {
                        // No report arrived at all: nothing to wait for.
                        queued = 0;
                    }
                }
            }
        }
    };
    let page = |items: &[huehuecoyotl_core::Summary]| {
        for i in items {
            let year = i.year.map(|y| format!(" ({y})")).unwrap_or_default();
            let extra = if i.kind == "show" {
                format!("  [{} unwatched]", i.unwatched)
            } else if i.watched {
                "  ✓".into()
            } else {
                String::new()
            };
            let art = if i.poster.is_empty() { "" } else { "  [art]" };
            println!("  {:>6}  {}{year}{extra}{art}", i.id, i.title);
        }
    };

    let ev = match (cmd.as_str(), &args[1..]) {
        ("browse", rest) => {
            core.browse(1, rest.first().map_or("", String::as_str));
            wait()
        }
        ("open", [path]) => {
            core.open_path(1, path);
            wait()
        }
        ("progress", [path, pos, dur, rest @ ..]) => {
            core.open_path(1, path);
            let Event::Playback { ticket, .. } = wait() else {
                return usage();
            };
            let (Ok(p), Ok(d)) = (pos.parse::<f64>(), dur.parse::<f64>()) else {
                return usage();
            };
            let state = if rest.iter().any(|a| a == "--ended") {
                PlayerState::Ended
            } else {
                PlayerState::Paused
            };
            core.report_progress(
                ticket.ticket,
                (p * 1000.0) as u64,
                (d * 1000.0) as u64,
                state,
            );
            loop {
                let e = rx
                    .recv_timeout(Duration::from_secs(30))
                    .expect("no answer from core");
                if matches!(e, Event::ItemChanged { .. }) {
                    break e;
                }
            }
        }
        ("recent", []) => {
            core.continue_watching(1, 20);
            wait()
        }
        ("library", [sub, path]) if sub == "add" => {
            core.add_library(1, path);
            let ev = wait();
            settle();
            ev
        }
        ("library", [sub, spec]) if sub == "add-share" => {
            core.add_network_library(1, spec, &share_password());
            let ev = wait();
            settle();
            ev
        }
        ("share-ls", [spec, rest @ ..]) if rest.len() <= 1 => {
            let path = rest.first().map_or("/", |p| p.as_str());
            core.browse_network(1, spec, &share_password(), path);
            wait()
        }
        ("discover", []) => {
            core.discover(1);
            wait()
        }
        ("diagnostics", [dest]) => {
            return match core.export_diagnostics(Path::new(dest), "app coyotl-cli") {
                Ok(n) => {
                    println!("wrote {dest} ({n} bytes)");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            };
        }
        ("library", [sub]) if sub == "list" => {
            core.libraries(1);
            wait()
        }
        ("rescan", []) => {
            core.rescan(0);
            settle();
            core.libraries(1);
            wait()
        }
        ("movies" | "shows", []) => {
            core.browse_items(
                1,
                if cmd == "movies" { "movie" } else { "show" },
                0,
                "title",
                "",
                0,
                500,
            );
            wait()
        }
        ("search", words) if !words.is_empty() => {
            core.search(1, &words.join(" "));
            wait()
        }
        ("detail", [id]) => {
            let Ok(id) = id.parse() else { return usage() };
            core.item_detail(1, id);
            wait()
        }
        ("review", []) => {
            core.review(1);
            wait()
        }
        ("key", [key]) => {
            core.set_tmdb_key(1, key);
            wait()
        }
        ("download", [item, rest @ ..]) if rest.len() <= 1 => {
            let (Ok(item), Ok(file)) = (
                item.parse(),
                rest.first().map_or(Ok(0), |f| f.parse::<i64>()),
            ) else {
                return usage();
            };
            core.save_item(1, item, file);
            match wait() {
                Event::Downloads { .. } => follow_downloads(&core, &rx),
                e => e,
            }
        }
        ("downloads", [sub]) if sub == "list" => {
            core.downloads(1);
            wait()
        }
        ("downloads", [sub, id]) if ["delete", "pause", "resume"].contains(&sub.as_str()) => {
            let Ok(id) = id.parse() else { return usage() };
            match sub.as_str() {
                "delete" => core.delete_download(1, id),
                "pause" => {
                    core.pause_download(id);
                    core.downloads(1);
                }
                _ => {
                    core.resume_download(id);
                    core.downloads(1);
                }
            }
            wait()
        }
        ("storage", rest) if rest.len() <= 1 => {
            match rest.first() {
                Some(d) => core.set_storage_dir(1, d),
                None => core.downloads(1),
            }
            wait()
        }
        _ => return usage(),
    };

    match ev {
        Event::Folder { path, entries, .. } => {
            println!("{}", if path.is_empty() { "(places)" } else { &path });
            for e in entries {
                let mark = if e.is_dir {
                    "/"
                } else if e.watched {
                    " ✓"
                } else {
                    ""
                };
                let prog = if e.progress > 0.0 && !e.watched {
                    format!("  [{:>3.0}%]", e.progress * 100.0)
                } else {
                    String::new()
                };
                println!("  {}{mark}{prog}    {}", e.name, e.path);
            }
        }
        Event::Playback { ticket, .. } => {
            println!(
                "ticket {} item {} \"{}\"",
                ticket.ticket, ticket.item_id, ticket.title
            );
            println!("  url        {}", ticket.url);
            println!("  resume     {:.1} s", ticket.resume_ms as f64 / 1000.0);
            for s in &ticket.subtitles {
                println!(
                    "  subtitle   {} [{}{}]",
                    s.url,
                    s.lang,
                    if s.forced { ", forced" } else { "" }
                );
            }
            println!(
                "  prefs      audio={:?} sub={:?} off={}",
                ticket.prefs.audio_lang, ticket.prefs.sub_lang, ticket.prefs.sub_off
            );
        }
        Event::ItemChanged { item_id } => println!("saved progress for item {item_id}"),
        Event::ContinueWatching { items, .. } => {
            for i in items {
                println!(
                    "  {:>5.1}%  {}  ({})",
                    i.position_ms as f64 * 100.0 / i.duration_ms.max(1) as f64,
                    i.title,
                    i.path
                );
            }
        }
        Event::Libraries { libraries, .. } => {
            for l in libraries {
                println!(
                    "  {:>3}  {}  ({} files{}{})",
                    l.id,
                    l.root,
                    l.files,
                    if l.scanning { ", scanning" } else { "" },
                    if l.online { "" } else { ", offline" }
                );
            }
        }
        Event::Page { total, items, .. } => {
            println!("{total} items");
            page(&items);
        }
        Event::Detail { detail: d, .. } => {
            let i = &d.item;
            println!(
                "{} {}{}  [{}] tmdb={:?}",
                i.kind,
                i.title,
                i.year.map(|y| format!(" ({y})")).unwrap_or_default(),
                d.match_state,
                d.tmdb_id
            );
            if !i.overview.is_empty() {
                println!("  {}", i.overview);
            }
            for f in &d.files {
                println!("  file   {} ({} MB)", f.path, f.size / 1_000_000);
            }
            if let Some(n) = &d.next_up {
                println!("  next   {} (id {})", n.title, n.id);
            }
            page(&d.children);
        }
        Event::Review { items, .. } => {
            for r in items {
                println!("  {:>6}  {}", r.item.id, r.item.title);
                for c in r.candidates {
                    println!(
                        "            {:.2}  {} ({})  tmdb {}",
                        c.score,
                        c.title,
                        c.year.map_or("?".into(), |y| y.to_string()),
                        c.provider_id
                    );
                }
            }
        }
        Event::KeyStatus {
            has_key, message, ..
        } => {
            println!(
                "TMDB key {}{}",
                if has_key { "set" } else { "not set" },
                if message.is_empty() {
                    String::new()
                } else {
                    format!(": {message}")
                }
            );
        }
        Event::Discovered { devices, .. } => {
            if devices.is_empty() {
                println!("no file servers announced themselves");
            }
            for d in devices {
                println!(
                    "  {:<8} {}:{}  {}",
                    if d.tls { "webdavs" } else { &d.protocol },
                    d.host,
                    d.port,
                    d.name
                );
            }
        }
        Event::Downloads {
            items,
            used_bytes,
            storage_dir,
            ..
        } => {
            println!(
                "storage: {storage_dir} ({} MB used)",
                used_bytes / 1_000_000
            );
            let mut failed = false;
            for d in items {
                let pct = (d.bytes_done * 100).checked_div(d.size).unwrap_or(100);
                println!(
                    "{:>5}  {:<6} {:<9} {:>3}%  {}  {}{}",
                    d.id,
                    d.kind,
                    d.state,
                    pct,
                    d.title,
                    d.path,
                    if d.error.is_empty() {
                        String::new()
                    } else {
                        format!("  [{}: {}]", d.error_kind, d.error)
                    }
                );
                failed |= d.state == "failed";
            }
            if failed {
                return ExitCode::FAILURE;
            }
        }
        Event::Candidates { .. }
        | Event::LibraryChanged { .. }
        | Event::ScanState { .. }
        | Event::DownloadProgress { .. }
        | Event::SaveState { .. } => {}
        Event::Error { kind, message, .. } => {
            eprintln!("error ({kind:?}): {message}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
