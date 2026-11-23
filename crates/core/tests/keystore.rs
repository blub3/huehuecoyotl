//! Issue #52: the TMDB key and source passwords survive a restart, "Remove key" removes the
//! keystore entry, a missing keystore is reported instead of losing secrets, and the key never
//! reaches the logs or the diagnostics export.
//!
//! Each step runs in a new process (this test binary, started again), so every lookup really
//! comes from the keystore. It runs only inside the throwaway keyring of `tools/test-keystore.sh`
//! (`COYOTL_TEST_KEYRING=1`: its own D-Bus session and gnome-keyring-daemon in a temporary home),
//! never against the user's keystore.
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crossbeam_channel::unbounded;
use huehuecoyotl_core::secrets::{self, KeyLoad};
use huehuecoyotl_core::{Core, Event};

// Made-up values: they only have to be recognisable in files.
const KEY: &str = "coyotl-test-key-6f1e9d2a4b7c";
const PASSWORD: &str = "coyotl-test-password-91c3e8";
const SOURCE_REF: &str = "source:7";

fn enabled() -> bool {
    std::env::var_os("COYOTL_TEST_KEYRING").is_some()
}

/// Run one step in a new process; returns the lines it reported.
fn step(name: &str, data: &Path, env: &[(&str, &str)]) -> Vec<String> {
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("COYOTL_KEYRING_STEP", name)
        .env("COYOTL_KEYRING_DATA", data)
        .env_remove("COYOTL_TMDB_KEY")
        .env_remove("COYOTL_DISABLE_KEYSTORE")
        .envs(env.iter().copied())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "step {name} failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<String> = stdout
        .lines()
        // libtest prints "test child ... " without a line break before the first line.
        .filter_map(|l| l.split_once("RESULT ").map(|(_, r)| r.to_string()))
        .collect();
    eprintln!("step {name}: {lines:?}");
    lines
}

/// One step, in a process of its own (started by `step`).
#[test]
#[ignore]
fn child() {
    let Ok(name) = std::env::var("COYOTL_KEYRING_STEP") else {
        return;
    };
    let data = std::path::PathBuf::from(std::env::var("COYOTL_KEYRING_DATA").unwrap());
    match name.as_str() {
        "store" => {
            secrets::store_tmdb_key(KEY).unwrap();
            secrets::store_password(SOURCE_REF, PASSWORD).unwrap();
            println!("RESULT stored");
        }
        "load" => {
            println!(
                "RESULT key {}",
                match secrets::load_tmdb_key() {
                    KeyLoad::Found(k, src) => format!("found {} {src:?}", k == KEY),
                    KeyLoad::Absent => "absent".into(),
                    KeyLoad::Unavailable(_) => "unavailable".into(),
                }
            );
            println!(
                "RESULT password {}",
                secrets::load_password(SOURCE_REF) == PASSWORD
            );
        }
        "store-fails" => {
            println!("RESULT store {}", secrets::store_tmdb_key(KEY).is_ok());
        }
        "remove" => {
            secrets::clear_tmdb_key().unwrap();
            secrets::delete_password(SOURCE_REF);
            println!("RESULT removed");
        }
        "core" => {
            // The app's start: logging into the data folder, then the core.
            huehuecoyotl_core::init_logging(&data);
            let (tx, rx) = unbounded();
            let core = Core::open(&data, "en-US", move |e: Event| {
                let _ = tx.send(e);
            })
            .unwrap();
            core.key_status(1);
            let mut seen = 0;
            while seen < 2 {
                match rx
                    .recv_timeout(Duration::from_secs(20))
                    .expect("key status")
                {
                    Event::KeyStatus {
                        request_id, state, ..
                    } => {
                        println!("RESULT status {request_id} {state}");
                        seen += 1;
                    }
                    _ => continue,
                }
            }
            core.export_diagnostics(&data.join("diagnostics.txt"), "test")
                .unwrap();
            drop(core);
        }
        other => panic!("unknown step {other}"),
    }
}

/// Every file under `dir` (logs, database, diagnostics) without the secrets.
fn assert_no_secrets(dir: &Path) {
    for entry in walk(dir) {
        let bytes = std::fs::read(&entry).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(KEY), "the key is in {}", entry.display());
        assert!(
            !text.contains(PASSWORD),
            "the password is in {}",
            entry.display()
        );
    }
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn secrets_survive_a_restart() {
    if !enabled() {
        eprintln!("skipped: run tools/test-keystore.sh");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path();

    assert_eq!(step("store", data, &[]), ["stored"]);
    // A new process finds both.
    assert_eq!(
        step("load", data, &[]),
        ["key found true Keystore", "password true"]
    );
    // The core reports the stored key, also to a request sent before it looked.
    let status = step("core", data, &[]);
    assert!(
        status.contains(&"status 0 keystore".to_string()),
        "{status:?}"
    );
    assert!(
        status.contains(&"status 1 keystore".to_string()),
        "{status:?}"
    );
    assert!(data.join("diagnostics.txt").exists());
    assert!(data.join("logs").exists());
    assert_no_secrets(data);

    // "Remove key": gone after a restart too.
    assert_eq!(step("remove", data, &[]), ["removed"]);
    assert_eq!(step("load", data, &[]), ["key absent", "password false"]);
    let status = step("core", data, &[]);
    assert!(status.contains(&"status 1 none".to_string()), "{status:?}");

    // No keystore to reach: reported as such, and nothing is written anywhere else.
    let gone = [("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus")];
    assert_eq!(step("store-fails", data, &gone), ["store false"]);
    assert_eq!(
        step("load", data, &gone),
        ["key unavailable", "password false"]
    );
    let status = step("core", data, &gone);
    assert!(
        status.contains(&"status 1 unavailable".to_string()),
        "{status:?}"
    );
    assert_no_secrets(data);
}
