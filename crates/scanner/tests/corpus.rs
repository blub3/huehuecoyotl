//! Runs the parser over the fixture corpus and reports every mismatch at once.
use huehuecoyotl_scanner::parse::{Kind, parse};

#[test]
fn filename_corpus() {
    let corpus = include_str!("fixtures/filenames.tsv");
    let mut failures = Vec::new();
    let mut n = 0;
    for line in corpus.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        n += 1;
        let cols: Vec<&str> = line.split('\t').collect();
        let get = |i: usize| cols.get(i).copied().unwrap_or("").trim();
        let path = get(0).replace("\\\\", "\\");
        let p = parse(&path);
        let kind = match p.kind {
            Kind::Movie => "movie",
            Kind::Episode => "episode",
            Kind::Extra => "extra",
        };
        let year = p.year.map(|y| y.to_string()).unwrap_or_default();
        let season = p.season.map(|s| s.to_string()).unwrap_or_default();
        let eps = p
            .episodes
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let part = p.part.map(|s| s.to_string()).unwrap_or_default();
        let got = [kind, &p.title, &year, &season, &eps, &part];
        let want = [get(1), get(2), get(3), get(4), get(5), get(6)];
        let title_ok = if want[0] == "extra" {
            true
        } else {
            got[1] == want[1]
        };
        if got[0] != want[0] || !title_ok || got[2..] != want[2..] {
            failures.push(format!("{path}\n    want {want:?}\n    got  {got:?}"));
        }
    }
    assert!(n > 80, "corpus too small: {n}");
    assert!(
        failures.is_empty(),
        "{} of {n} corpus entries failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
