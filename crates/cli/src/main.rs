//! `coyotl-cli`: the headless harness (SPEC 2, principle 2). It drives the same core API as
//! the Qt shell, with a channel as its event sink.
//!
//!   coyotl-cli [--data-dir DIR] browse [PATH]
//!   coyotl-cli [--data-dir DIR] open PATH
//!   coyotl-cli [--data-dir DIR] progress PATH POSITION_S DURATION_S [--ended]
//!   coyotl-cli [--data-dir DIR] recent
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
    let core = match Core::open(&data_dir, move |e| {
        let _ = tx.send(e);
    }) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot open core: {e}");
            return ExitCode::FAILURE;
        }
    };
    let wait = || {
        rx.recv_timeout(Duration::from_secs(30))
            .expect("no answer from core")
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
            wait()
        }
        ("recent", []) => {
            core.continue_watching(1, 20);
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
        Event::Error { kind, message, .. } => {
            eprintln!("error ({kind:?}): {message}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
