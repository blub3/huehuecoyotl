//! Match scoring (SPEC 7, scan step 4): a low-confidence match goes to a review list instead of
//! becoming a wrong poster.

use crate::Candidate;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Confident: apply the top candidate.
    Matched,
    /// Plausible candidates, but the user should pick.
    NeedsReview,
    /// Nothing usable.
    Unmatched,
}

pub const ACCEPT: f64 = 0.90;
pub const MARGIN: f64 = 0.08;
pub const PLAUSIBLE: f64 = 0.50;
/// Similarity when the candidate's title appears in the file title as whole words.
pub const CONTAINED: f64 = 0.90;
/// Similarity when the file title appears in a longer candidate title.
pub const CONTAINS: f64 = 0.75;
pub const ACCEPT_SINGLE: f64 = 0.82;
pub const POPULAR_FACTOR: f64 = 4.0;
pub const POPULAR_MIN: f64 = 3.0;

fn norm(s: &str) -> String {
    let s = s
        .to_lowercase()
        .replace(['\'', '’', '`'], "") // "Can't" == "Cant"
        .replace('&', " and ")
        .replace('½', " 1 2 ")
        .replace('⅓', " 1 3 ")
        .replace('¼', " 1 4 ");
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(|w| match w {
            // "Fantastic Four" == "Fantastic 4"
            "one" => "1",
            "two" => "2",
            "three" => "3",
            "four" => "4",
            "five" => "5",
            "six" => "6",
            "seven" => "7",
            "eight" => "8",
            "nine" => "9",
            "ten" => "10",
            other => other,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// How well a candidate title `b` fits the file's title `a` (not symmetric): 1.0 for identical
/// normalised titles, falling with edit distance; see the containment rules below.
pub fn similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (norm(a), norm(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let (ac, bc): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let edit = 1.0 - levenshtein(&ac, &bc) as f64 / ac.len().max(bc.len()) as f64;
    // Whole-word containment. The candidate inside the file title is a strong hint: file names
    // add franchise or artist prefixes ("Star Wars Episode V The Empire Strikes Back" /
    // "The Empire Strikes Back", "Dune Part One" / "Dune"). The file title inside a longer
    // candidate is weak: those are premieres, specials and sequels ("Alien" / "Alien: Covenant").
    let inside = |small: &str, big: &str| {
        small.len() >= 4 && format!(" {big} ").contains(&format!(" {small} "))
    };
    if inside(&b, &a) {
        edit.max(CONTAINED)
    } else if inside(&a, &b) {
        edit.max(CONTAINS)
    } else {
        edit
    }
}

pub fn score(c: &Candidate, title: &str, year: Option<u16>) -> f64 {
    let sim = similarity(title, &c.title).max(similarity(title, &c.original_title));
    let year_term = match (year, c.year) {
        (Some(a), Some(b)) => match a.abs_diff(b) {
            0 => 0.15,
            1 => 0.05,
            _ => -0.30,
        },
        // No year to compare: neutral. Identical titles (remakes) still fall to the review
        // list through the margin rule.
        _ => 0.10,
    };
    // Popularity only breaks near-ties.
    let pop = (c.popularity.max(0.0) + 1.0).ln() / 1000.0;
    sim * 0.85 + year_term + pop
}

/// Score and sort candidates (best first), then decide.
pub fn decide(cands: &mut [Candidate], title: &str, year: Option<u16>) -> (Decision, Vec<f64>) {
    let mut scored: Vec<(f64, Candidate)> = cands
        .iter()
        .map(|c| (score(c, title, year), c.clone()))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (slot, (_, c)) in cands.iter_mut().zip(scored.iter()) {
        *slot = c.clone();
    }
    let scores: Vec<f64> = scored.iter().map(|s| s.0).collect();
    let d = match scored.as_slice() {
        [] => Decision::Unmatched,
        [(best, top), rest @ ..] => {
            let clear = match rest.first() {
                None => true,
                Some((second, other)) => {
                    *best - second >= MARGIN
                        // Same title and year several times (shorts, fan films, re-uploads):
                        // the far more popular entry is the film people have on disk.
                        || (top.popularity >= POPULAR_FACTOR * other.popularity.max(0.1)
                            && top.popularity >= POPULAR_MIN)
                }
            };
            // A single result for title + exact year is trustworthy at a lower similarity
            // ("Naked Gun 33 1.3" vs "Naked Gun 33⅓").
            let year_exact = year.is_some() && top.year == year;
            let accept = if rest.is_empty() && year_exact {
                ACCEPT_SINGLE
            } else {
                ACCEPT
            };
            if *best >= accept && clear {
                Decision::Matched
            } else if *best >= PLAUSIBLE {
                Decision::NeedsReview
            } else {
                Decision::Unmatched
            }
        }
    };
    (d, scores)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: i64, title: &str, year: u16, pop: f64) -> Candidate {
        Candidate {
            id,
            title: title.into(),
            year: Some(year),
            popularity: pop,
            ..Default::default()
        }
    }

    #[test]
    fn exact_title_and_year_matches() {
        let mut v = vec![
            c(1, "Alien", 1979, 50.0),
            c(2, "Aliens", 1986, 60.0),
            c(3, "Alien: Romulus", 2024, 90.0),
        ];
        let (d, _) = decide(&mut v, "Alien", Some(1979));
        assert_eq!(d, Decision::Matched);
        assert_eq!(v[0].id, 1);
    }

    #[test]
    fn remakes_without_year_need_review() {
        let mut v = vec![c(1, "The Thing", 1982, 40.0), c(2, "The Thing", 2011, 30.0)];
        assert_eq!(decide(&mut v, "The Thing", None).0, Decision::NeedsReview);
        // with the year it is clear
        assert_eq!(decide(&mut v, "The Thing", Some(2011)).0, Decision::Matched);
        assert_eq!(v[0].id, 2);
    }

    #[test]
    fn wrong_year_or_title_is_not_accepted() {
        let mut v = vec![c(1, "Heat", 1995, 30.0)];
        assert_eq!(decide(&mut v, "Heat", Some(2013)).0, Decision::NeedsReview);
        let mut v = vec![c(1, "Completely Different", 2001, 30.0)];
        assert_eq!(
            decide(&mut v, "Some Home Video", None).0,
            Decision::Unmatched
        );
        assert_eq!(decide(&mut [], "x", None).0, Decision::Unmatched);
    }

    #[test]
    fn real_library_cases() {
        // Duplicate title + year on TMDB: the popular one wins.
        let mut v = vec![
            c(1, "Anna", 2019, 45.0),
            c(2, "Anna", 2019, 1.2),
            c(3, "Anna", 2019, 0.6),
        ];
        let (d, _) = decide(&mut v, "Anna", Some(2019));
        assert_eq!((d, v[0].id), (Decision::Matched, 1));
        // ...but two popular remakes still need review.
        let mut v = vec![
            c(1, "Dinner for One", 1963, 9.0),
            c(2, "Dinner for One", 2006, 4.0),
        ];
        assert_eq!(
            decide(&mut v, "Dinner for one", None).0,
            Decision::NeedsReview
        );
        // Title contained in the file name, exact year.
        let mut v = vec![c(1, "Dune", 2021, 90.0), c(2, "Dune: Part Two", 2024, 80.0)];
        assert_eq!(
            decide(&mut v, "Dune Part One", Some(2021)).0,
            Decision::Matched
        );
        let mut v = vec![c(1, "The Empire Strikes Back", 1980, 40.0)];
        assert_eq!(
            decide(
                &mut v,
                "Star Wars Episode V The Empire Strikes Back",
                Some(1980)
            )
            .0,
            Decision::Matched
        );
        let mut v = vec![
            c(1, "If I Can't Have Love, I Want Power", 2021, 3.0),
            c(
                2,
                "Halsey: If I Can't Have Love, I Want Power - Global Performance Experience",
                2021,
                1.0,
            ),
        ];
        assert_eq!(
            decide(
                &mut v,
                "Halsey If I Cant Have Love I Want Power",
                Some(2021)
            )
            .0,
            Decision::Matched
        );
        // Number words and fractions.
        let mut v = vec![
            c(1, "The Fantastic 4: First Steps", 2025, 300.0),
            c(
                2,
                "Marvel Studios' The Fantastic Four: First Steps - World Premiere",
                2025,
                2.0,
            ),
        ];
        assert_eq!(
            decide(&mut v, "The Fantastic Four First Steps", Some(2025)).0,
            Decision::Matched
        );
        let mut v = vec![c(1, "Naked Gun 33⅓: The Final Insult", 1994, 20.0)];
        assert_eq!(
            decide(&mut v, "Naked Gun 33 1.3 The Final Insult", Some(1994)).0,
            Decision::Matched
        );
        // The only result for title + exact year, close enough: the US and UK titles of one film.
        let mut v = vec![c(
            1,
            "Harry Potter and the Philosopher's Stone",
            2001,
            0.0, // no popularity bonus: the rule must hold on similarity alone
        )];
        assert_eq!(
            decide(&mut v, "Harry Potter and the Sorcerers Stone", Some(2001)).0,
            Decision::Matched
        );
        // An only result that is clearly something else is not accepted.
        let mut v = vec![c(1, "Hamilton", 2025, 5.0)];
        assert_ne!(decide(&mut v, "Hamilton", Some(2020)).0, Decision::Matched);
        let mut v = vec![c(1, "The Rise of Lewis Hamilton", 2020, 1.0)];
        assert_ne!(decide(&mut v, "Hamilton", Some(2020)).0, Decision::Matched);
        // Short names do not match by containment alone.
        let mut v = vec![c(1, "Up", 2009, 50.0)];
        assert!(similarity("Up", "Upgrade") < 0.9);
        assert_eq!(decide(&mut v, "Up", Some(2009)).0, Decision::Matched);
        // Without a year an exact title still beats a longer one containing it.
        let mut v = vec![
            c(1, "Alien", 1979, 50.0),
            c(2, "Alien: Covenant", 2017, 60.0),
        ];
        let (d, _) = decide(&mut v, "Alien", None);
        assert_eq!((d, v[0].id), (Decision::Matched, 1));
    }

    #[test]
    fn punctuation_and_ampersands() {
        assert_eq!(similarity("Fast & Furious 6", "Fast and Furious 6"), 1.0);
        assert_eq!(
            similarity(
                "Spider-Man: Into the Spider-Verse",
                "Spider Man Into the Spider Verse"
            ),
            1.0
        );
        assert!(similarity("Amelie", "Amélie") > 0.8);
    }
}
