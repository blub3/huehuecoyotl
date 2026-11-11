//! Fill a data folder with a synthetic library for the performance budgets (SPEC 9): N movies,
//! each with its own pre-scaled grid poster, as the scanner and artwork worker would leave them.
//!
//!   cargo run --release -p huehuecoyotl-library --example bench_seed -- DATA_DIR [N] [--originals]
//!
//! With `--originals` each movie also gets a 780 px full-size poster (role `poster`), as a
//! library from before the 540 px grid tier had: the app's `tiers` job then makes that tier.
//!
//! The library folder points at a path that does not exist, so a rescan finds it unreachable
//! and leaves the items alone. Items are locked and matched, so no metadata jobs are queued.

use std::path::PathBuf;

use huehuecoyotl_library::{migrate, rusqlite};
use image::{ImageBuffer, Rgb};

const WORDS: &[&str] = &[
    "Night", "River", "Coyote", "Silent", "Last", "Iron", "Golden", "Winter", "Shadow", "City",
    "Dance", "Storm", "Lost", "Garden", "Machine", "Desert", "Mirror", "Echo", "Harbor", "Kingdom",
];

fn poster(id: u32, w: u32) -> Vec<u8> {
    // 360 px wide like the artwork worker's grid thumbnails (artwork::GRID_WIDTH), 2:3; 780 px
    // for a full-size original.
    let h = w * 3 / 2;
    let hue = (id.wrapping_mul(2_654_435_761) >> 8) % 360;
    let base = hsv(hue as f32, 0.55, 0.55);
    let img = ImageBuffer::from_fn(w, h, |x, y| {
        let t = y as f32 / h as f32;
        let band = ((x * 360 / w / 40 + y * 540 / h / 60 + id) % 7) as f32 / 30.0;
        let k = 1.15 - 0.7 * t + band;
        Rgb([
            (base[0] * k).min(255.0) as u8,
            (base[1] * k).min(255.0) as u8,
            (base[2] * k).min(255.0) as u8,
        ])
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82)
        .encode_image(&img)
        .expect("encode poster");
    out
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [(r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().ok_or("usage: bench_seed DATA_DIR [N]")?);
    let rest: Vec<String> = args.collect();
    let originals = rest.iter().any(|a| a == "--originals");
    let n: u32 = rest
        .iter()
        .find(|a| !a.starts_with("--"))
        .map_or(Ok(5000), |s| s.parse())?;
    let art = dir.join("artwork").join("bench");
    std::fs::create_dir_all(&art)?;

    let mut c = rusqlite::Connection::open(dir.join("library.db"))?;
    c.pragma_update(None, "journal_mode", "WAL")?;
    migrate(&mut c)?;
    let tx = c.transaction()?;
    let missing = dir.join("bench-media-not-mounted");
    tx.execute(
        "INSERT INTO source(kind, name, root, last_scan) VALUES ('local', 'Benchmark', ?1, 1)",
        [missing.to_string_lossy()],
    )?;
    let source = tx.last_insert_rowid();
    let now = 1_762_732_800_000i64;
    for i in 0..n {
        let title = format!(
            "{} {} {}",
            WORDS[(i as usize * 7) % WORDS.len()],
            WORDS[(i as usize * 13 + 3) % WORDS.len()],
            i + 1
        );
        let year = 1950 + (i % 75) as i64;
        tx.execute(
            "INSERT INTO item(kind, title, sort_title, year, match_state, locked, added_at, group_key)
             VALUES ('movie', ?1, lower(?1), ?2, 'matched', 1, ?3, ?4)",
            rusqlite::params![title, year, now + i as i64, format!("bench:{i}")],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO item_fts(rowid, title) VALUES (?1, ?2)",
            rusqlite::params![id, title],
        )?;
        let path = missing.join(format!("{title} ({year}).mkv"));
        tx.execute(
            "INSERT INTO file(source_id, path, size, mtime, item_id, last_seen_scan) VALUES (?1, ?2, ?3, 0, ?4, 1)",
            rusqlite::params![source, path.to_string_lossy(), 4_000_000_000i64, id],
        )?;
        let jpg = art.join(format!("{id}.jpg"));
        std::fs::write(&jpg, poster(i, 360))?;
        if originals {
            let full = art.join(format!("{id}-full.jpg"));
            std::fs::write(&full, poster(i, 780))?;
            tx.execute(
                "INSERT INTO artwork(item_id, role, remote_path, local_path, width, height) VALUES (?1, 'poster', ?2, ?3, 780, 1170)",
                rusqlite::params![id, format!("/bench/{id}.jpg"), full.to_string_lossy()],
            )?;
        }
        tx.execute(
            "INSERT INTO artwork(item_id, role, remote_path, local_path, width, height) VALUES (?1, 'poster_grid', ?2, ?3, 360, 540)",
            rusqlite::params![id, format!("/bench/{id}.jpg"), jpg.to_string_lossy()],
        )?;
    }
    tx.commit()?;
    println!("seeded {n} movies into {}", dir.display());
    Ok(())
}
