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
//!
//! Commands that start background work (library add, rescan) wait until the job queue is
//! empty, so the harness can scan and match a library headless.
use std::path::PathBuf;
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

fn usage() -> ExitCode {
    eprintln!(
        "usage: coyotl-cli [--data-dir DIR] browse [PATH] | open PATH | progress PATH POS_S DUR_S [--ended] | recent"
    );
    ExitCode::from(2)
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
            Event::ItemChanged { .. } | Event::LibraryChanged { .. } | Event::ScanState { .. } => {
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
                false,
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
                    "  {:>3}  {}  ({} files{})",
                    l.id,
                    l.root,
                    l.files,
                    if l.scanning { ", scanning" } else { "" }
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
        Event::Candidates { .. } | Event::LibraryChanged { .. } | Event::ScanState { .. } => {}
        Event::Error { kind, message, .. } => {
            eprintln!("error ({kind:?}): {message}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
