//! The audiobook pipeline (2.5, issue #65, SPEC 7 "Audiobook library"): for library folders whose
//! content type is audiobooks. Files are grouped into books (a single file, a folder, a folder
//! with disc subfolders); each book's metadata comes from local sources, highest first: a Calibre
//! `metadata.opf`, `desc.txt` and `reader.txt`, a `.nfo` file, the first file's tags, then the
//! folder and file names. Durations come from the tag reader's stream properties (headers, never
//! a whole file). A book is read again only when one of its files or metadata files changed.

use std::collections::{BTreeMap, HashMap};

use huehuecoyotl_library::books::{self, BookInput, FileInput};
use huehuecoyotl_library::{Library, copies, rusqlite::params};
use huehuecoyotl_vfs::{Source, VPath, VfsError};

use crate::music::Cover;
use crate::scan::{ScanError, fingerprint_with_data};
use crate::tags::{self, TrackTags};

/// Audio files an audiobook can be made of; `.aax`/`.aaxc` are Audible's DRM files (listed, not
/// played).
const AUDIO: &[&str] = &[
    "m4b", "m4a", "mp4", "mp3", "ogg", "oga", "opus", "flac", "aax", "aaxc",
];
const DRM: &[&str] = &["aax", "aaxc"];
const COVERS: &[&str] = &["cover", "folder", "front"];
const SKIP_DIRS: &[&str] = &["@eadir", ".trash", "#recycle", "$recycle.bin", ".ds_store"];
/// Metadata files are read whole up to this size.
const META_MAX: u64 = 1 << 20;

fn ext(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn is_book_audio(name: &str) -> bool {
    !name.starts_with("._") && AUDIO.contains(&ext(name).as_str())
}

fn parent(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(p, _)| p)
}

fn base(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

/// `CD1`, `Disc 2`, `Disk 03`, `Part 1`, `Teil 2`.
pub fn is_disc_folder(name: &str) -> bool {
    let l = name.to_lowercase();
    let l = l.trim();
    for p in ["cd", "disc", "disk", "part", "teil", "teil "] {
        if let Some(rest) = l.strip_prefix(p) {
            let rest = rest.trim_start_matches([' ', '_', '-', '.']);
            if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

/// Compare names so that "2" comes before "10".
pub fn natural_key(s: &str) -> Vec<(u8, u64, String)> {
    let mut out = Vec::new();
    let mut num = String::new();
    let mut text = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            if !text.is_empty() {
                out.push((1, 0, std::mem::take(&mut text).to_lowercase()));
            }
            num.push(ch);
        } else {
            if !num.is_empty() {
                out.push((0, num.parse().unwrap_or(u64::MAX), String::new()));
                num.clear();
            }
            text.push(ch);
        }
    }
    if !num.is_empty() {
        out.push((0, num.parse().unwrap_or(u64::MAX), String::new()));
    }
    if !text.is_empty() {
        out.push((1, 0, text.to_lowercase()));
    }
    out
}

// ---- names ----------------------------------------------------------------------------------

/// What a book's path says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Names {
    pub author: Option<String>,
    pub series: Option<String>,
    pub position: Option<f64>,
    pub title: String,
}

/// "Book 2.5 - Title", "01 - Title", "#3 Title", "Vol. 2 - Title" → (position, title).
pub fn split_position(name: &str) -> (Option<f64>, String) {
    let n = name.trim();
    let lower = n.to_lowercase();
    let mut rest = n;
    for p in ["book", "band", "vol.", "vol", "volume", "teil", "part", "#"] {
        if lower.starts_with(p) {
            rest = n[p.len()..].trim_start();
            break;
        }
    }
    let digits: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .collect();
    let number = digits.trim_end_matches(['.', ',']).replace(',', ".");
    if !number.is_empty()
        && let Ok(pos) = number.parse::<f64>()
    {
        let after = rest[digits.len()..].trim_start_matches([' ', '-', '–', '.', ':', '_', ')']);
        if !after.is_empty() {
            return (Some(pos), after.trim().to_string());
        }
    }
    (None, n.to_string())
}

/// `Title - Author` (a flat file or folder name).
fn title_author(name: &str) -> Option<(String, String)> {
    let (t, a) = name.rsplit_once(" - ")?;
    let (t, a) = (t.trim(), a.trim());
    (!t.is_empty() && !a.is_empty() && !a.chars().all(|c| c.is_ascii_digit()))
        .then(|| (t.to_string(), a.to_string()))
}

/// The layouts of SPEC 2: `Author/Title`, `Author/Series/01 - Title`,
/// `Author/Series/Book 2.5 - Title`, and `Title - Author` flat. `rel` is the book's folder or
/// file, relative to the library folder.
pub fn names(rel: &str, single_file: bool) -> Names {
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    let own = parts
        .last()
        .map(|p| if single_file { stem(p) } else { p })
        .unwrap_or("");
    let (position, title) = split_position(own);
    let mut n = Names {
        position,
        title,
        ..Default::default()
    };
    match parts.len() {
        0 | 1 => {
            if let Some((t, a)) = title_author(&n.title) {
                n.title = t;
                n.author = Some(a);
            }
        }
        2 => n.author = Some(parts[0].to_string()),
        _ => {
            n.author = Some(parts[parts.len() - 3].to_string());
            n.series = Some(parts[parts.len() - 2].to_string());
        }
    }
    if n.title.is_empty() {
        n.title = own.to_string();
    }
    n
}

// ---- metadata files -------------------------------------------------------------------------

/// What a Calibre `metadata.opf`, `.nfo`, `desc.txt` or `reader.txt` gives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub narrators: Vec<String>,
    pub series: Option<String>,
    pub position: Option<f64>,
    pub publisher: Option<String>,
    pub published: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub asin: Option<String>,
    pub description: Option<String>,
    /// Issue #67: from the tags only (Picard writes it).
    pub mb_release: Option<String>,
}

impl Meta {
    /// Fill what is empty here from `lower` (a source of lower priority).
    pub fn fill(&mut self, lower: Meta) {
        fn opt<T>(a: &mut Option<T>, b: Option<T>) {
            if a.is_none() {
                *a = b;
            }
        }
        opt(&mut self.title, lower.title);
        if self.authors.is_empty() {
            self.authors = lower.authors;
        }
        if self.narrators.is_empty() {
            self.narrators = lower.narrators;
        }
        opt(&mut self.series, lower.series);
        opt(&mut self.position, lower.position);
        opt(&mut self.publisher, lower.publisher);
        opt(&mut self.published, lower.published);
        opt(&mut self.language, lower.language);
        opt(&mut self.isbn, lower.isbn);
        opt(&mut self.asin, lower.asin);
        opt(&mut self.description, lower.description);
        opt(&mut self.mb_release, lower.mb_release);
    }
}

fn clean(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Text of an HTML description, roughly: tags out, entities of the common kind decoded.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One XML element with its attributes (local names, lower case) and its text, entities
/// resolved. Documents are flattened to this, so the parsers below do not depend on the XML
/// library's event details.
#[derive(Clone, Debug, Default)]
struct El {
    name: String,
    attrs: HashMap<String, String>,
    text: String,
}

fn entity(name: &str) -> String {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok(),
            None => num.parse().ok(),
        };
        return code
            .and_then(char::from_u32)
            .map(String::from)
            .unwrap_or_default();
    }
    match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        _ => "",
    }
    .to_string()
}

fn unescape(raw: &str) -> String {
    quick_xml::escape::unescape(raw).map_or_else(|_| raw.to_string(), |c| c.into_owned())
}

fn elements(xml: &str) -> Vec<El> {
    use quick_xml::Reader;
    use quick_xml::events::Event as X;
    let mut r = Reader::from_str(xml);
    let mut stack: Vec<El> = Vec::new();
    let mut out = Vec::new();
    let open = |e: &quick_xml::events::BytesStart| {
        let mut el = El {
            name: e.local_name().as_ref().to_lowercase(),
            ..Default::default()
        };
        for a in e.attributes().flatten() {
            let k = a.key.local_name().as_ref().to_lowercase();
            let v = unescape(&a.value);
            el.attrs.insert(k, v);
        }
        el
    };
    loop {
        match r.read_event() {
            Ok(X::Start(e)) => stack.push(open(&e)),
            Ok(X::Empty(e)) => out.push(open(&e)),
            Ok(X::Text(t)) => {
                if let Some(el) = stack.last_mut() {
                    el.text.push_str(&unescape(&t));
                }
            }
            Ok(X::CData(t)) => {
                if let Some(el) = stack.last_mut() {
                    el.text.push_str(&t);
                }
            }
            Ok(X::GeneralRef(e)) => {
                if let Some(el) = stack.last_mut() {
                    el.text.push_str(&entity(&e));
                }
            }
            Ok(X::End(_)) => {
                if let Some(el) = stack.pop() {
                    out.push(el);
                }
            }
            Ok(X::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// A Calibre OPF (OPF 2 with Calibre's `<meta name="calibre:series">`, or OPF 3 with
/// `belongs-to-collection`).
pub fn parse_opf(xml: &str) -> Meta {
    let mut m = Meta::default();
    let mut collections: Vec<(String, String)> = Vec::new();
    let mut positions: HashMap<String, String> = HashMap::new();
    for el in elements(xml) {
        let attr = |k: &str| el.attrs.get(k).map(String::as_str);
        let Some(text) = clean(&el.text).or_else(|| attr("content").and_then(clean)) else {
            continue;
        };
        match el.name.as_str() {
            "meta" => match (attr("name"), attr("property")) {
                (Some("calibre:series"), _) => m.series = Some(text),
                (Some("calibre:series_index"), _) => m.position = text.parse().ok(),
                (_, Some("belongs-to-collection")) => {
                    collections.push((attr("id").unwrap_or("").to_string(), text))
                }
                (_, Some("group-position")) => {
                    positions.insert(
                        attr("refines")
                            .unwrap_or("")
                            .trim_start_matches('#')
                            .to_string(),
                        text,
                    );
                }
                _ => {}
            },
            "title" => {
                if m.title.is_none() {
                    m.title = Some(text)
                }
            }
            "creator" | "contributor" => match attr("role").map(str::to_lowercase).as_deref() {
                Some("nrt") | Some("narrator") => m.narrators.push(text),
                Some("aut") | None if el.name == "creator" => m.authors.push(text),
                _ => {}
            },
            "publisher" => m.publisher = Some(text),
            "date" => {
                if m.published.is_none() {
                    m.published = Some(text.chars().take(10).collect())
                }
            }
            "language" => {
                m.language = Some(text.chars().take(2).collect::<String>().to_lowercase())
            }
            "description" => m.description = clean(&strip_html(&text)),
            "identifier" => {
                let lower = text.to_lowercase();
                match attr("scheme").map(str::to_lowercase).as_deref() {
                    Some("isbn") => m.isbn = Some(digits_x(&text)),
                    Some("asin") | Some("amazon") | Some("audible") => m.asin = Some(text),
                    _ if lower.starts_with("urn:isbn:") || lower.starts_with("isbn:") => {
                        m.isbn = Some(digits_x(&text))
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    if m.series.is_none()
        && let Some((id, name)) = collections.into_iter().next()
    {
        m.series = Some(name);
        if m.position.is_none() {
            m.position = positions.get(&id).and_then(|p| p.trim().parse().ok());
        }
    }
    m
}

fn digits_x(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
        .collect::<String>()
        .to_uppercase()
}

/// A Kodi-style `.nfo` (XML): title, author, narrator, series, year, plot. Anything else (a
/// plain-text release note) gives nothing.
pub fn parse_nfo(text: &str) -> Meta {
    let mut m = Meta::default();
    if !text.trim_start().starts_with('<') {
        return m;
    }
    for el in elements(text) {
        let Some(t) = clean(&el.text) else { continue };
        match el.name.as_str() {
            "title" => m.title = Some(t),
            "author" | "artist" | "writer" => m.authors.push(t),
            "narrator" | "reader" => m.narrators.push(t),
            "series" | "set" => m.series = Some(t),
            "seriesindex" | "series_index" | "position" => m.position = t.parse().ok(),
            "year" | "premiered" | "releasedate" => {
                m.published = Some(t.chars().take(10).collect())
            }
            "plot" | "description" | "outline" => m.description = Some(t),
            "publisher" | "studio" => m.publisher = Some(t),
            "isbn" => m.isbn = Some(digits_x(&t)),
            "asin" => m.asin = Some(t),
            _ => {}
        }
    }
    m
}

/// `reader.txt`: one narrator per line (or separated by commas).
fn parse_readers(text: &str) -> Vec<String> {
    text.lines()
        .flat_map(|l| l.split([',', ';']))
        .filter_map(clean)
        .collect()
}

/// What a book's first file's tags give (album = title, album artist or artist = author,
/// composer = narrator, movement = series).
fn from_tags(t: &TrackTags) -> Meta {
    let authors: Vec<String> = if !t.album_artists.is_empty() {
        t.album_artists.iter().map(|c| c.name.clone()).collect()
    } else {
        t.artists.iter().map(|c| c.name.clone()).collect()
    };
    Meta {
        title: t.album.clone().or_else(|| t.title.clone()),
        authors,
        narrators: t.book.narrators.clone(),
        series: t.book.series.clone(),
        position: t
            .book
            .series_part
            .as_deref()
            .and_then(|p| p.trim().replace(',', ".").parse().ok()),
        publisher: t.book.publisher.clone(),
        published: t.date.clone(),
        language: t.book.language.clone(),
        isbn: t.book.isbn.clone(),
        asin: t.book.asin.clone(),
        description: t.book.description.clone(),
        mb_release: t.mb_release.clone(),
    }
}

// ---- walking and grouping -------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Walked {
    rel: String,
    path: VPath,
    size: u64,
    mtime: i64,
}

/// A book found on disk: its key path (folder or file), whether it is a single file, its audio
/// files in order and the metadata files next to it.
#[derive(Clone, Debug, Default)]
struct Plan {
    rel: String,
    single: bool,
    files: Vec<Walked>,
    meta: Vec<Walked>,
}

async fn walk(src: &dyn Source, root: &VPath) -> Result<(Vec<Walked>, Vec<Walked>), VfsError> {
    let prefix = format!("{}/", root.as_str().trim_end_matches('/'));
    let rel = |p: &VPath| {
        p.as_str()
            .strip_prefix(&prefix)
            .unwrap_or(p.as_str())
            .to_string()
    };
    let (mut audio, mut meta) = (Vec::new(), Vec::new());
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let entries = match src.list(&dir).await {
            Ok(e) => e,
            Err(e) if dir == *root => return Err(e),
            Err(e) => {
                tracing::warn!(%dir, "skipping unreadable folder: {e}");
                continue;
            }
        };
        for e in entries {
            let lower = e.name.to_lowercase();
            let w = || Walked {
                rel: rel(&e.path),
                path: e.path.clone(),
                size: e.size,
                mtime: e.mtime,
            };
            if e.is_dir {
                if !SKIP_DIRS.contains(&lower.as_str()) {
                    stack.push(e.path.clone());
                }
            } else if is_book_audio(&e.name) {
                audio.push(w());
            } else if lower == "metadata.opf"
                || lower == "desc.txt"
                || lower == "reader.txt"
                || lower.ends_with(".nfo")
                || (COVERS.contains(&stem(&lower))
                    && matches!(ext(&lower).as_str(), "jpg" | "jpeg" | "png"))
            {
                meta.push(w());
            }
        }
    }
    Ok((audio, meta))
}

/// Files into books (SPEC 7, scan step 1): files directly in the library folder are one book
/// each; a folder of M4B or DRM files holds one book per file; any other folder with audio files
/// is one book, with its disc subfolders.
fn plan(audio: Vec<Walked>, meta: &[Walked]) -> Vec<Plan> {
    let mut folders: BTreeMap<String, Vec<Walked>> = BTreeMap::new();
    for w in audio {
        let mut dir = parent(&w.rel).to_string();
        if is_disc_folder(base(&dir)) && !parent(&dir).is_empty() {
            dir = parent(&dir).to_string();
        }
        folders.entry(dir).or_default().push(w);
    }
    let meta_of = |dir: &str| -> Vec<Walked> {
        meta.iter()
            .filter(|m| parent(&m.rel) == dir)
            .cloned()
            .collect()
    };
    let mut out = Vec::new();
    for (dir, mut files) in folders {
        let whole_books = files.iter().all(|f| {
            let e = ext(&f.rel);
            e == "m4b" || DRM.contains(&e.as_str())
        }) && files.len() > 1;
        if dir.is_empty() || whole_books {
            for f in files {
                out.push(Plan {
                    rel: f.rel.clone(),
                    single: true,
                    files: vec![f],
                    meta: if dir.is_empty() {
                        vec![]
                    } else {
                        meta_of(&dir)
                    },
                });
            }
        } else {
            files.sort_by(|a, b| {
                let (da, db) = (disc_of(&a.rel, &dir), disc_of(&b.rel, &dir));
                (da, natural_key(base(&a.rel))).cmp(&(db, natural_key(base(&b.rel))))
            });
            out.push(Plan {
                rel: dir.clone(),
                single: false,
                files,
                meta: meta_of(&dir),
            });
        }
    }
    out
}

/// The disc a file of `book_dir` sits in (from its `CD2` subfolder), 0 when none.
fn disc_of(rel: &str, book_dir: &str) -> u64 {
    let dir = parent(rel);
    if dir == book_dir {
        return 0;
    }
    base(dir)
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

// ---- scanning -------------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct BookReport {
    pub files_seen: usize,
    pub books: usize,
    pub read: usize,
    pub removed: usize,
    pub new_books: Vec<i64>,
    /// Books with a cover from their tags or a sidecar image, to write into the poster tiers.
    pub covers: Vec<(i64, Cover)>,
    /// Books whose files should go to the probe for embedded chapters (single files, M4B).
    pub probe: Vec<i64>,
}

/// The signature of a book's files and metadata files: when it is unchanged, the book is not
/// read again.
fn signature(p: &Plan) -> String {
    p.files
        .iter()
        .chain(p.meta.iter())
        .map(|w| format!("{}:{}:{}", w.rel, w.size, w.mtime))
        .collect::<Vec<_>>()
        .join("\n")
}

async fn read_text(src: &dyn Source, w: &Walked) -> Option<String> {
    if w.size == 0 || w.size > META_MAX {
        return None;
    }
    let data = src.read_at(&w.path, 0, w.size as u32).await.ok()?;
    Some(
        String::from_utf8_lossy(&data)
            .trim_start_matches('\u{feff}')
            .to_string(),
    )
}

pub async fn scan_audiobook_source(
    lib: &Library,
    src: &dyn Source,
    source_id: i64,
    root: &VPath,
) -> Result<BookReport, ScanError> {
    let (audio, meta) = walk(src, root).await?;
    let existing: HashMap<String, i64> = lib
        .read(move |c| {
            let mut st = c.prepare("SELECT path, id FROM file WHERE source_id = ?1")?;
            let rows = st.query_map([source_id], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))?;
            Ok(rows.collect::<huehuecoyotl_library::rusqlite::Result<_>>()?)
        })
        .await?;
    if audio.is_empty() && !existing.is_empty() {
        return Err(ScanError::LooksUnreachable);
    }
    let mut report = BookReport {
        files_seen: audio.len(),
        ..Default::default()
    };
    let plans = plan(audio, &meta);
    report.books = plans.len();
    let now = crate::scan::now_ms();
    // Which books changed since the last scan.
    let keys: Vec<(String, String)> = plans
        .iter()
        .map(|p| (format!("b:{source_id}:{}", p.rel), signature(p)))
        .collect();
    let stored: Vec<Option<String>> = {
        let keys = keys.clone();
        lib.read(move |c| {
            keys.iter()
                .map(|(k, _)| copies::setting(c, &format!("book_scan:{k}")))
                .collect::<huehuecoyotl_library::Result<Vec<_>>>()
        })
        .await?
    };
    let mut seen_paths: Vec<String> = Vec::new();
    for p in &plans {
        seen_paths.extend(p.files.iter().map(|f| f.rel.clone()));
    }
    for (i, p) in plans.iter().enumerate() {
        let (key, sig) = &keys[i];
        if stored[i].as_deref() == Some(sig.as_str()) {
            continue;
        }
        report.read += 1;
        let book = read_book(src, p, source_id, key).await;
        let (input, cover, sig) = (book.0, book.1, sig.clone());
        let files: Vec<(String, u64, i64, Option<Vec<u8>>)> = book.2;
        let key = key.clone();
        let single = p.single || p.files.len() == 1;
        let (id, created) = lib
            .write(move |c| {
                let tx = c.transaction()?;
                let mut input = input;
                for (f, (rel, size, mtime, fp)) in input.files.iter_mut().zip(files) {
                    tx.execute(
                        "INSERT INTO file(source_id, path, size, mtime, fingerprint, last_seen_scan) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                         ON CONFLICT(source_id, path) DO UPDATE SET size = excluded.size, mtime = excluded.mtime, last_seen_scan = excluded.last_seen_scan",
                        params![source_id, rel, size as i64, mtime, fp, now],
                    )?;
                    f.file_id = tx.query_row(
                        "SELECT id FROM file WHERE source_id = ?1 AND path = ?2",
                        params![source_id, rel],
                        |r| r.get(0),
                    )?;
                }
                let r = books::upsert(&tx, &input, now)?;
                copies::set_setting(&tx, &format!("book_scan:{key}"), Some(&sig))?;
                tx.commit()?;
                Ok(r)
            })
            .await?;
        if created {
            report.new_books.push(id);
        }
        if let Some(cv) = cover {
            report.covers.push((id, cv));
        }
        if single || p.files.iter().any(|f| ext(&f.rel) == "m4b") {
            report.probe.push(id);
        }
    }
    // Files gone: their rows, and books, series and people nobody needs any more.
    let gone: Vec<i64> = existing
        .iter()
        .filter(|(p, _)| !seen_paths.contains(p))
        .map(|(_, id)| *id)
        .collect();
    report.removed = gone.len();
    let live: std::collections::HashSet<String> =
        keys.iter().map(|(k, _)| format!("book_scan:{k}")).collect();
    lib.write(move |c| {
        let tx = c.transaction()?;
        for id in &gone {
            tx.execute("DELETE FROM file WHERE id = ?1", [id])?;
        }
        // Signatures of books that are gone.
        let stale: Vec<String> = {
            let mut st = tx.prepare("SELECT key FROM setting WHERE key LIKE ?1")?;
            st.query_map([format!("book_scan:b:{source_id}:%")], |r| r.get(0))?
                .collect::<huehuecoyotl_library::rusqlite::Result<Vec<String>>>()?
                .into_iter()
                .filter(|k| !live.contains(k))
                .collect()
        };
        for k in stale {
            copies::set_setting(&tx, &k, None)?;
        }
        books::prune(&tx)?;
        tx.execute(
            "UPDATE source SET last_scan = ?2 WHERE id = ?1",
            params![source_id, now],
        )?;
        tx.commit()?;
        Ok(())
    })
    .await?;
    tracing::info!(
        source_id,
        files = report.files_seen,
        books = report.books,
        read = report.read,
        removed = report.removed,
        "audiobook scan finished"
    );
    Ok(report)
}

type FileRow = (String, u64, i64, Option<Vec<u8>>);

/// Read one book: tags and durations of its files, its metadata files, its names.
async fn read_book(
    src: &dyn Source,
    p: &Plan,
    _source_id: i64,
    key: &str,
) -> (BookInput, Option<Cover>, Vec<FileRow>) {
    let mut files = Vec::new();
    let mut rows = Vec::new();
    let mut first: Option<TrackTags> = None;
    let mut cover = None;
    let mut drm = false;
    for (i, w) in p.files.iter().enumerate() {
        let (fp, seed) = match fingerprint_with_data(src, &w.path, w.size).await {
            Ok((fp, seed)) => (Some(fp), seed),
            Err(_) => (None, Vec::new()),
        };
        let e = ext(&w.rel);
        drm |= DRM.contains(&e.as_str());
        let tags = if DRM.contains(&e.as_str()) {
            None
        } else {
            match tags::read_seeded(src, &w.path, w.size, seed).await {
                Ok(r) => Some(r.tags),
                Err(err) => {
                    tracing::warn!(path = %w.rel, "tags not readable: {err}");
                    None
                }
            }
        };
        let name = stem(base(&w.rel)).to_string();
        let title = tags
            .as_ref()
            .and_then(|t| t.title.clone())
            .filter(|_| p.files.len() > 1)
            .unwrap_or_else(|| split_position(&name).1);
        files.push(FileInput {
            file_id: 0,
            duration_ms: tags.as_ref().map_or(0, |t| t.audio.duration_ms as i64),
            title,
        });
        rows.push((w.rel.clone(), w.size, w.mtime, fp));
        if i == 0 {
            if let Some(t) = &tags
                && let Some(c) = &t.cover
            {
                cover = Some(Cover::Embedded(c.clone()));
            }
            first = tags;
        }
    }
    // Local metadata, highest first.
    let mut m = Meta::default();
    for w in &p.meta {
        if base(&w.rel).eq_ignore_ascii_case("metadata.opf")
            && let Some(t) = read_text(src, w).await
        {
            m.fill(parse_opf(&t));
        }
    }
    for w in &p.meta {
        let name = base(&w.rel).to_lowercase();
        if name == "desc.txt"
            && let Some(t) = read_text(src, w).await
        {
            m.fill(Meta {
                description: clean(&t),
                ..Default::default()
            });
        } else if name == "reader.txt"
            && let Some(t) = read_text(src, w).await
        {
            m.fill(Meta {
                narrators: parse_readers(&t),
                ..Default::default()
            });
        }
    }
    for w in &p.meta {
        if base(&w.rel).to_lowercase().ends_with(".nfo")
            && let Some(t) = read_text(src, w).await
        {
            m.fill(parse_nfo(&t));
        }
    }
    if let Some(t) = &first {
        m.fill(from_tags(t));
    }
    let n = names(&p.rel, p.single);
    m.fill(Meta {
        title: Some(n.title.clone()),
        authors: n.author.clone().into_iter().collect(),
        series: n.series.clone(),
        position: n.position.filter(|_| n.series.is_some()),
        ..Default::default()
    });
    if cover.is_none() {
        // A sidecar image: cover, then folder, then front.
        let mut best: Option<(usize, &Walked)> = None;
        for w in &p.meta {
            let s = stem(base(&w.rel)).to_lowercase();
            if let Some(rank) = COVERS.iter().position(|c| *c == s)
                && best.is_none_or(|(r, _)| rank < r)
            {
                best = Some((rank, w));
            }
        }
        cover = best.map(|(_, w)| Cover::Sidecar(w.path.clone()));
    }
    let input = BookInput {
        key: key.to_string(),
        title: m.title.unwrap_or(n.title),
        authors: m.authors,
        narrators: m.narrators,
        series: m.series,
        series_position: m.position,
        publisher: m.publisher,
        published: m.published,
        language: m.language,
        isbn: m.isbn,
        asin: m.asin,
        description: m.description,
        mb_release: m.mb_release,
        drm,
        files,
        chapters: vec![],
    };
    (input, cover, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts() {
        let n = names("Ann Author/The Title", false);
        assert_eq!(
            (
                n.author.as_deref(),
                n.series.as_deref(),
                n.position,
                n.title.as_str()
            ),
            (Some("Ann Author"), None, None, "The Title")
        );
        let n = names("Ann Author/Saga/01 - First", false);
        assert_eq!(
            (
                n.author.as_deref(),
                n.series.as_deref(),
                n.position,
                n.title.as_str()
            ),
            (Some("Ann Author"), Some("Saga"), Some(1.0), "First")
        );
        let n = names("Ann Author/Saga/Book 2.5 - Between", false);
        assert_eq!(
            (n.series.as_deref(), n.position, n.title.as_str()),
            (Some("Saga"), Some(2.5), "Between")
        );
        let n = names("The Title - Ann Author.m4b", true);
        assert_eq!(
            (n.author.as_deref(), n.title.as_str()),
            (Some("Ann Author"), "The Title")
        );
        let n = names("Books/Ann Author/Saga/Band 3 - Third", false);
        assert_eq!(
            (n.author.as_deref(), n.series.as_deref(), n.position),
            (Some("Ann Author"), Some("Saga"), Some(3.0))
        );
        assert_eq!(split_position("1984"), (None, "1984".into()));
        assert_eq!(split_position("#3 Third"), (Some(3.0), "Third".into()));
    }

    #[test]
    fn disc_folders_and_order() {
        assert!(is_disc_folder("CD1") && is_disc_folder("Disc 2") && is_disc_folder("disk_03"));
        assert!(!is_disc_folder("Cdrom") && !is_disc_folder("Discovery"));
        let mut names = vec!["10.mp3", "2.mp3", "1.mp3"];
        names.sort_by_key(|n| natural_key(n));
        assert_eq!(names, ["1.mp3", "2.mp3", "10.mp3"]);
    }

    #[test]
    fn opf() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>Second Book</dc:title>
    <dc:creator opf:role="aut">Ann Author</dc:creator>
    <dc:creator opf:role="nrt">Ned Narrator</dc:creator>
    <dc:publisher>Test House</dc:publisher>
    <dc:date>2001-05-04T00:00:00+00:00</dc:date>
    <dc:language>eng</dc:language>
    <dc:identifier opf:scheme="ISBN">978-0-00-000000-2</dc:identifier>
    <dc:description>&lt;p&gt;A &lt;b&gt;fine&lt;/b&gt; book.&lt;/p&gt;</dc:description>
    <meta name="calibre:series" content="Saga"/>
    <meta name="calibre:series_index" content="2.0"/>
  </metadata>
</package>"#;
        let m = parse_opf(xml);
        assert_eq!(m.title.as_deref(), Some("Second Book"));
        assert_eq!(m.authors, ["Ann Author"]);
        assert_eq!(m.narrators, ["Ned Narrator"]);
        assert_eq!(m.series.as_deref(), Some("Saga"));
        assert_eq!(m.position, Some(2.0));
        assert_eq!(m.published.as_deref(), Some("2001-05-04"));
        assert_eq!(m.language.as_deref(), Some("en"));
        assert_eq!(m.isbn.as_deref(), Some("9780000000002"));
        assert_eq!(m.description.as_deref(), Some("A fine book."));
    }

    #[test]
    fn nfo_and_priority() {
        let nfo = "<audiobook><title>From NFO</title><author>Nfo Author</author><narrator>Nfo Reader</narrator><plot>Plot.</plot></audiobook>";
        let mut m = Meta {
            title: Some("From OPF".into()),
            ..Default::default()
        };
        m.fill(parse_nfo(nfo));
        assert_eq!(m.title.as_deref(), Some("From OPF"));
        assert_eq!(m.authors, ["Nfo Author"]);
        assert_eq!(m.narrators, ["Nfo Reader"]);
        assert_eq!(parse_nfo("Release notes, not XML"), Meta::default());
        assert_eq!(
            parse_readers("Ned Narrator, Nora Reader\n"),
            ["Ned Narrator", "Nora Reader"]
        );
    }
}
