//! One rotating log file for both languages and the "export diagnostics" action (SPEC 10).
//!
//! Rust logs through `tracing`; the C++ shell forwards Qt's messages through [`log_external`],
//! so both end up in `<data_dir>/logs/coyotl.log`. The file rotates at [`MAX_BYTES`] and the
//! last [`KEEP`] files are kept. An export concatenates them, oldest first, after a short
//! system summary, with host names, user names, addresses and keys removed.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use regex::Regex;

pub const MAX_BYTES: u64 = 5 * 1024 * 1024;
/// Rotated files besides the current one: coyotl.1.log (newest) … coyotl.3.log.
pub const KEEP: usize = 3;

pub struct RotatingFile {
    dir: PathBuf,
    file: Option<File>,
    size: u64,
    max: u64,
}

impl RotatingFile {
    pub fn open(dir: &Path) -> io::Result<Self> {
        Self::with_limit(dir, MAX_BYTES)
    }

    pub fn with_limit(dir: &Path, max: u64) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("coyotl.log"))?;
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            dir: dir.to_path_buf(),
            file: Some(file),
            size,
            max,
        })
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file = None;
        let _ = fs::remove_file(self.dir.join(format!("coyotl.{KEEP}.log")));
        for i in (1..KEEP).rev() {
            let _ = fs::rename(
                self.dir.join(format!("coyotl.{i}.log")),
                self.dir.join(format!("coyotl.{}.log", i + 1)),
            );
        }
        fs::rename(self.dir.join("coyotl.log"), self.dir.join("coyotl.1.log"))?;
        self.file = Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join("coyotl.log"))?,
        );
        self.size = 0;
        Ok(())
    }
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.size > 0 && self.size + buf.len() as u64 > self.max {
            // A failed rotation keeps writing to whatever file is open rather than losing logs.
            if let Err(e) = self.rotate() {
                eprintln!("coyotl: log rotation failed: {e}");
            }
        }
        let f = match self.file.as_mut() {
            Some(f) => f,
            None => return Ok(buf.len()),
        };
        let n = f.write(buf)?;
        self.size += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.as_mut().map_or(Ok(()), |f| f.flush())
    }
}

/// Log files, oldest first.
pub fn log_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = (1..=KEEP)
        .rev()
        .map(|i| dir.join(format!("coyotl.{i}.log")))
        .collect();
    v.push(dir.join("coyotl.log"));
    v.into_iter().filter(|p| p.is_file()).collect()
}

/// A message from the C++ shell. `level`: 0 debug, 1 info, 2 warning, 3 critical/fatal.
pub fn log_external(level: u8, category: &str, message: &str) {
    match level {
        0 => tracing::debug!(target: "qt", category, "{message}"),
        1 => tracing::info!(target: "qt", category, "{message}"),
        2 => tracing::warn!(target: "qt", category, "{message}"),
        _ => tracing::error!(target: "qt", category, "{message}"),
    }
}

/// Removes what identifies the user or their network from log text: the hosts, user names and
/// shares of configured sources, credentials in URLs, IP and MAC addresses, `.local` names,
/// API keys and tokens, the home folder and the login name.
pub struct Redactor {
    /// Known values, matched as whole words (longest first).
    literals: Vec<(Regex, String)>,
    patterns: Vec<(Regex, &'static str)>,
}

/// Words that appear in every log and must survive even if a user or share is named after them.
const KEEP_WORDS: &[&str] = &[
    "coyotl",
    "huehuecoyotl",
    "media",
    "video",
    "videos",
    "movies",
    "shows",
    "share",
    "public",
    "guest",
    "admin",
    "user",
    "home",
];

impl Redactor {
    /// `secrets`: (value, placeholder) pairs known to the caller, such as source hosts.
    pub fn new(secrets: Vec<(String, String)>) -> Self {
        let mut literals: Vec<(String, String)> = secrets
            .into_iter()
            .filter(|(v, _)| v.trim().len() >= 2)
            .collect();
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            let home = home.to_string_lossy().to_string();
            if home.len() > 1 {
                literals.push((home, "~".into()));
            }
        }
        for var in ["USER", "USERNAME", "LOGNAME"] {
            if let Ok(u) = std::env::var(var)
                && u.len() >= 3
            {
                literals.push((u, "<login>".into()));
            }
        }
        if let Ok(h) = hostname()
            && h.len() >= 3
        {
            literals.push((h, "<this-host>".into()));
        }
        // Longest first, so "nas.example.org" is replaced before "nas".
        literals.sort_by_key(|(v, _)| std::cmp::Reverse(v.len()));
        literals.dedup_by(|a, b| a.0 == b.0);
        let literals = literals
            .into_iter()
            .filter(|(v, _)| !KEEP_WORDS.contains(&v.to_lowercase().as_str()))
            .filter_map(|(v, ph)| {
                // Whole words only: a user "max" must not turn "maximum" into "<user>imum".
                // Paths (the home folder) start with a separator, so no boundary is needed there.
                let esc = regex::escape(&v);
                let start = if v.starts_with(|c: char| c.is_alphanumeric()) {
                    r"\b"
                } else {
                    ""
                };
                let end = if v.ends_with(|c: char| c.is_alphanumeric()) {
                    r"\b"
                } else {
                    ""
                };
                Regex::new(&format!("(?i){start}{esc}{end}"))
                    .ok()
                    .map(|re| (re, ph))
            })
            .collect();

        let p = |re: &str| Regex::new(re).expect("valid redaction pattern");
        let patterns = vec![
            (
                p(r"(?i)\b([a-z][a-z0-9+.-]*://)[^/@\s]+@"),
                "${1}<credentials>@",
            ),
            (
                p(
                    r"(?i)\b(api_key|apikey|access_token|token|password|passwd|secret)(\s*[=:]\s*)[^\s&,;)]+",
                ),
                "${1}${2}<redacted>",
            ),
            (
                p(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=-]+"),
                "Bearer <redacted>",
            ),
            (p(r"\b[0-9a-fA-F]{32,}\b"), "<hex>"),
            (p(r"\b(?:[0-9a-fA-F]{2}[:-]){5}[0-9a-fA-F]{2}\b"), "<mac>"),
            (
                p(r"\b(?:25[0-5]|2[0-4]\d|1?\d?\d)(?:\.(?:25[0-5]|2[0-4]\d|1?\d?\d)){3}\b"),
                "<ip>",
            ),
            // IPv6: the full form, or a compressed one with "::" (log times like 12:30:45 stay).
            (p(r"(?i)\b(?:[0-9a-f]{1,4}:){7}[0-9a-f]{1,4}\b"), "<ip6>"),
            (
                p(r"(?i)\b(?:[0-9a-f]{1,4}:)*[0-9a-f]{1,4}::(?:[0-9a-f]{1,4}:)*[0-9a-f]{1,4}\b"),
                "<ip6>",
            ),
            (p(r"(?i)\b[a-z0-9][a-z0-9-]*\.local\b\.?"), "<host>.local"),
        ];
        Self { literals, patterns }
    }

    pub fn redact(&self, text: &str) -> String {
        let mut s = text.to_string();
        for (re, ph) in &self.literals {
            if let std::borrow::Cow::Owned(o) = re.replace_all(&s, regex::NoExpand(ph)) {
                s = o;
            }
        }
        for (re, rep) in &self.patterns {
            if let std::borrow::Cow::Owned(o) = re.replace_all(&s, *rep) {
                s = o;
            }
        }
        s
    }
}

fn hostname() -> io::Result<String> {
    if let Ok(h) = std::env::var("HOSTNAME")
        && !h.is_empty()
    {
        return Ok(h);
    }
    fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string())
}

/// Write the export: `header` (already redacted by the caller where needed), then every log
/// file, oldest first, through `redactor`. Returns the bytes written.
pub fn write_export(
    dest: &Path,
    header: &str,
    log_dir: &Path,
    redactor: &Redactor,
) -> io::Result<u64> {
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut out = io::BufWriter::new(File::create(dest)?);
    out.write_all(redactor.redact(header).as_bytes())?;
    for path in log_files(log_dir) {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        writeln!(out, "\n===== {name} =====")?;
        let bytes = fs::read(&path)?;
        for line in String::from_utf8_lossy(&bytes).lines() {
            out.write_all(redactor.redact(line).as_bytes())?;
            out.write_all(b"\n")?;
        }
    }
    out.flush()?;
    Ok(fs::metadata(dest)?.len())
}

impl crate::Core {
    /// Write a redacted diagnostics file to `dest`. `app_info` comes from the shell (Qt, mpv
    /// and GPU versions) and is redacted too. Blocking; takes well under a second.
    pub fn export_diagnostics(&self, dest: &Path, app_info: &str) -> crate::CResult<u64> {
        use huehuecoyotl_library::{catalog, jobs};
        let inner = self.inner.clone();
        let (libs, stats) = self.rt.block_on(async {
            let libs = inner.lib.read(catalog::libraries).await?;
            let stats = inner.lib.read(jobs::stats).await?;
            Ok::<_, crate::CoreError>((libs, stats))
        })?;
        let offline = inner.offline.lock().unwrap().clone();

        // Everything a source config says about the user's network becomes a placeholder.
        let mut secrets = Vec::new();
        for (i, l) in libs.iter().enumerate() {
            let cfg: serde_json::Value = serde_json::from_str(&l.config_json).unwrap_or_default();
            for (field, label) in [("host", "host"), ("user", "user"), ("share", "share")] {
                if let Some(v) = cfg.get(field).and_then(|v| v.as_str()) {
                    secrets.push((v.to_string(), format!("<{label}-{}>", i + 1)));
                }
            }
            if l.kind != "local" {
                secrets.push((l.name.clone(), format!("<library-{}>", i + 1)));
            }
        }
        if let crate::secrets::KeyLoad::Found(key, _) = crate::secrets::load_tmdb_key() {
            secrets.push((key, "<tmdb-key>".into()));
        }
        let redactor = Redactor::new(secrets);

        let mut header = String::new();
        use std::fmt::Write as _;
        let _ = writeln!(header, "Huehuecoyotl diagnostics");
        let _ = writeln!(header, "core {}", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(
            header,
            "os {} {} ({})",
            std::env::consts::OS,
            std::env::consts::ARCH,
            os_release()
        );
        let _ = writeln!(header, "language {}", inner.language);
        let _ = writeln!(header, "{}", app_info.trim_end());
        let _ = writeln!(header, "\nlibraries:");
        for (i, l) in libs.iter().enumerate() {
            let _ = writeln!(
                header,
                "  #{} {} files={} online={} scanned={}",
                i + 1,
                l.kind,
                l.files,
                !offline.contains(&l.id),
                l.last_scan.is_some()
            );
        }
        let _ = writeln!(
            header,
            "jobs: queued={} running={} failed={}",
            stats.queued, stats.running, stats.failed
        );
        let _ = writeln!(
            header,
            "metadata: {}",
            if inner.provider().is_some() {
                "TMDB configured"
            } else {
                "no TMDB key"
            }
        );

        write_export(dest, &header, &inner.data_dir.join("logs"), &redactor).map_err(|e| {
            crate::CoreError::new(
                crate::ErrorKind::Internal,
                format!("{}: {e}", dest.display()),
            )
        })
    }
}

fn os_release() -> String {
    fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| {
                l.strip_prefix("PRETTY_NAME=")
                    .map(|v| v.trim_matches('"').to_string())
            })
        })
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_keeps_three_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = RotatingFile::with_limit(dir.path(), 100).unwrap();
        for i in 0..20 {
            writeln!(f, "line {i:02} {}", "x".repeat(40)).unwrap();
        }
        let files = log_files(dir.path());
        assert_eq!(files.len(), KEEP + 1, "{files:?}");
        for p in &files {
            assert!(fs::metadata(p).unwrap().len() <= 100);
        }
        // The newest line is in coyotl.log, the oldest kept line in coyotl.3.log.
        let cur = fs::read_to_string(dir.path().join("coyotl.log")).unwrap();
        assert!(cur.contains("line 19"));
        let old = fs::read_to_string(dir.path().join("coyotl.3.log")).unwrap();
        assert!(!old.contains("line 19"));
    }

    #[test]
    fn redaction() {
        let r = Redactor::new(vec![
            ("nas.example.org".into(), "<host-1>".into()),
            ("nas".into(), "<host-2>".into()),
            ("alice".into(), "<user-1>".into()),
            ("coyotl".into(), "<user-2>".into()),
            ("max".into(), "<user-3>".into()),
        ]);
        let line = "connect smb://alice:hunter2@nas.example.org/media from 192.168.1.20 \
                    fe80::1c2b:3cff:fe4d:5e6f mac 00:11:22:33:44:55 DiskStation.local. \
                    GET /3/search/movie?api_key=0123456789abcdef0123456789abcdef&query=x \
                    authorization: Bearer eyJhbGciOiJIUzI1NiJ9.abc password=secret1 time 12:30:45";
        let out = r.redact(line);
        for leaked in [
            "alice",
            "hunter2",
            "nas.example.org",
            "192.168",
            "fe80",
            "00:11:22",
            "DiskStation",
            "0123456789abcdef",
            "eyJhbG",
            "secret1",
        ] {
            assert!(!out.contains(leaked), "{leaked} leaked: {out}");
        }
        assert!(out.contains("smb://<credentials>@<host-1>/media"), "{out}");
        assert!(out.contains("time 12:30:45"), "times must survive: {out}");
        let out = r.redact("huehuecoyotl_core: coyotl-cli maximum MAX");
        assert_eq!(out, "huehuecoyotl_core: coyotl-cli maximum <user-3>");
    }

    #[test]
    fn export_concatenates_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("coyotl.2.log"), "first\n").unwrap();
        fs::write(dir.path().join("coyotl.1.log"), "second from 10.0.0.5\n").unwrap();
        fs::write(dir.path().join("coyotl.log"), "third\n").unwrap();
        let dest = dir.path().join("out/diag.txt");
        write_export(&dest, "header\n", dir.path(), &Redactor::new(vec![])).unwrap();
        let s = fs::read_to_string(dest).unwrap();
        let (a, b, c) = (
            s.find("first").unwrap(),
            s.find("second").unwrap(),
            s.find("third").unwrap(),
        );
        assert!(a < b && b < c);
        assert!(s.contains("second from <ip>"));
    }
}
