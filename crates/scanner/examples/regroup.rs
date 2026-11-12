//! Run the issue #32 repair on a library database (use a copy): opening it applies migration 4
//! (TMDB duplicates merged), then the offline regroup runs as the app's background job would.
//! Prints the number of shows and movies before and after. Reads no media files.
//!
//!   cargo run -p huehuecoyotl-scanner --example regroup -- COPY/library.db
use huehuecoyotl_library::Library;
use huehuecoyotl_library::rusqlite::Connection;
use huehuecoyotl_scanner::parse::{Kind, parse};
use huehuecoyotl_scanner::scan::{find_show, regroup_shows, show_key};

fn counts(c: &Connection) -> String {
    let q = |sql: &str| -> i64 { c.query_row(sql, [], |r| r.get(0)).unwrap_or(-1) };
    format!(
        "shows {}, matched shows {}, distinct TMDB shows {}, movies {}, episodes {}, files {}, watch states {}",
        q("SELECT count(*) FROM item WHERE kind = 'show'"),
        q("SELECT count(*) FROM item WHERE kind = 'show' AND match_state = 'matched'"),
        q("SELECT count(DISTINCT tmdb_id) FROM item WHERE kind = 'show'"),
        q("SELECT count(*) FROM item WHERE kind = 'movie'"),
        q("SELECT count(*) FROM item WHERE kind = 'episode'"),
        q("SELECT count(*) FROM file WHERE item_id IS NOT NULL"),
        q("SELECT count(*) FROM playstate"),
    )
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: regroup LIBRARY.db (a copy)")?;
    println!("before: {}", counts(&Connection::open(&path)?));
    let lib = Library::open(std::path::Path::new(&path))?;
    println!("after migration: {}", lib.read(|c| Ok(counts(c))).await?);
    let report = lib
        .write(|c| {
            regroup_shows(c).map_err(|e| huehuecoyotl_library::DbError::Other(e.to_string()))
        })
        .await?;
    println!(
        "regroup: {} re-keyed, {} merged, {} to match again",
        report.rekeyed,
        report.merged,
        report.rematch.len()
    );
    println!("after regroup: {}", lib.read(|c| Ok(counts(c))).await?);
    // What the next scan would do: every episode file must lead to the show it sits under.
    let (ok, off) = lib
        .read(|c| {
            let mut st = c.prepare(
                "SELECT f.path, se.parent_id FROM file f JOIN item e ON e.id = f.item_id
                 JOIN item se ON se.id = e.parent_id WHERE e.kind = 'episode'",
            )?;
            let rows: Vec<(String, i64)> = st
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            let (mut ok, mut off) = (0, 0);
            for (path, show) in rows {
                let p = parse(&path);
                if p.kind != Kind::Episode {
                    continue;
                }
                let title = p.show_title.clone().unwrap_or(p.title.clone());
                match find_show(c, &show_key(&title), p.show_year.or(p.year))? {
                    Some(s) if s == show => ok += 1,
                    _ => off += 1,
                }
            }
            Ok((ok, off))
        })
        .await?;
    println!("next scan: {ok} episode files find their show, {off} would not");
    Ok(())
}
