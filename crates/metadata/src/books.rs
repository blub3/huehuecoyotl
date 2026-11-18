//! Book metadata (2.5, issue #67, SPEC 7 "Book providers"): the `BookProvider` trait, what it
//! returns, and when a search result is a book's match. Online data only fills what the local
//! metadata left empty; the decision here never matches a book to a work that disagrees with it.

use async_trait::async_trait;
use bytes::Bytes;

use crate::Result;
use crate::music::norm;

/// One search result: a work.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BookHit {
    /// The provider's id of the work ("OL66554W" at Open Library).
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<u16>,
    /// A cover by the provider's cover id.
    pub cover_id: Option<i64>,
    /// ISO 639-2 codes ("eng").
    pub languages: Vec<String>,
    /// Issue #109: the work's authors at the provider ("OL23919A").
    pub author_ids: Vec<String>,
}

/// A work's details.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Work {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub subjects: Vec<String>,
    pub year: Option<u16>,
    pub cover_ids: Vec<i64>,
    /// Issue #109: the work's authors at the provider ("OL23919A").
    pub author_ids: Vec<String>,
}

/// Issue #110: an author as a search or a lookup returns it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Author {
    pub id: String,
    pub name: String,
    /// "1974", "c. 1902": as the provider writes it.
    pub birth: Option<String>,
    pub death: Option<String>,
    pub work_count: Option<u32>,
    pub top_work: Option<String>,
    pub bio: Option<String>,
    pub photo_ids: Vec<i64>,
}

/// How a book was searched for.
#[derive(Clone, Debug, PartialEq)]
pub enum BookQuery<'a> {
    Isbn(&'a str),
    TitleAuthor { title: &'a str, author: &'a str },
}

/// Book providers. Open Library is the one implementation; MusicBrainz serves tagged books
/// through the music provider.
#[async_trait]
pub trait BookProvider: Send + Sync {
    /// Works for an ISBN, or by title and author, best first.
    async fn search(&self, q: BookQuery<'_>) -> Result<Vec<BookHit>>;
    /// A work's description, subjects, first publication year and covers.
    async fn work(&self, id: &str) -> Result<Work>;
    /// A cover by cover id, the large size; `None` when there is none.
    async fn cover(&self, cover_id: i64) -> Result<Option<Bytes>>;
    /// Issue #110: authors by name, best first.
    async fn search_authors(&self, _name: &str) -> Result<Vec<Author>> {
        Ok(vec![])
    }
    /// Issue #110: an author's details: biography, photos, dates.
    async fn author(&self, _id: &str) -> Result<Author> {
        Err(crate::MetaError::NotFound)
    }
    /// Issue #110: the titles of an author's works, to see which of the user's books are theirs.
    async fn author_works(&self, _id: &str) -> Result<Vec<String>> {
        Ok(vec![])
    }
    /// Issue #110: an author photo by photo id, the large size.
    async fn author_photo(&self, _photo_id: i64) -> Result<Option<Bytes>> {
        Ok(None)
    }
}

/// Issue #110: do these two names name one person? ("Tolkien, J. R. R." and "J.R.R. Tolkien".)
pub fn same_author(a: &str, b: &str) -> bool {
    same_person(a, b)
}

/// Issue #110: does a work title name one of these titles? Leading articles, subtitles and
/// punctuation do not count.
pub fn same_title(a: &str, b: &str) -> bool {
    let (x, y) = (title_key(a), title_key(b));
    !x.is_empty() && x == y
}

/// What a search means for a book.
#[derive(Clone, Debug, PartialEq)]
pub enum BookDecision {
    Matched(BookHit),
    /// Possible, not certain: the book waits in "Review matches" with these.
    Review(Vec<BookHit>),
    None,
}

/// "The Hobbit: or There and Back Again" compares as "The Hobbit"; a leading article and the
/// punctuation do not count.
fn title_key(s: &str) -> String {
    let main = s.split([':', '(', '[']).next().unwrap_or(s).trim();
    let lower = main.replace('&', " and ").to_lowercase();
    for a in [
        "the", "a", "an", "der", "die", "das", "le", "la", "les", "el",
    ] {
        if let Some(rest) = lower.strip_prefix(&format!("{a} ")) {
            return norm(rest);
        }
    }
    norm(&lower)
}

/// "Tolkien, J. R. R." and "J.R.R. Tolkien" agree: the same set of name parts, else the same
/// last name and initials.
fn same_person(a: &str, b: &str) -> bool {
    let parts = |s: &str| -> Vec<String> {
        let s = match s.split_once(',') {
            Some((last, first)) => format!("{first} {last}"),
            None => s.to_string(),
        };
        s.split(|c: char| c.is_whitespace() || c == '.')
            .map(norm)
            .filter(|p| !p.is_empty())
            .collect()
    };
    let (pa, pb) = (parts(a), parts(b));
    if pa.is_empty() || pb.is_empty() {
        return false;
    }
    if pa == pb {
        return true;
    }
    // Same last name, and every other part of the shorter one is an initial of the other's.
    if pa.last() != pb.last() {
        return false;
    }
    let initials = |p: &[String]| -> String {
        p[..p.len() - 1]
            .iter()
            .filter_map(|x| x.chars().next())
            .collect()
    };
    let (ia, ib) = (initials(&pa), initials(&pb));
    !ia.is_empty() && (ia == ib || ia.starts_with(&ib) || ib.starts_with(&ia))
}

/// A hit agrees with the book when the titles agree and an author agrees (or the book has no
/// author to compare).
pub fn agrees(h: &BookHit, title: &str, authors: &[String]) -> bool {
    title_key(&h.title) == title_key(title)
        && (authors.is_empty()
            || authors
                .iter()
                .any(|a| h.authors.iter().any(|b| same_person(a, b))))
}

/// Decide from search results. An ISBN search names the edition: its work is the match when the
/// titles agree. A title search matches the first hit that agrees in title and author (catalogues
/// list the same work twice at times, so later agreeing hits do not make it doubtful); hits that
/// only share words go to review; never a guess.
pub fn decide(hits: &[BookHit], title: &str, authors: &[String], by_isbn: bool) -> BookDecision {
    if by_isbn
        && let Some(h) = hits.first()
        && title_key(&h.title) == title_key(title)
    {
        return BookDecision::Matched(h.clone());
    }
    if let Some(h) = hits.iter().find(|h| agrees(h, title, authors)) {
        return BookDecision::Matched(h.clone());
    }
    let words: Vec<String> = title
        .split_whitespace()
        .map(norm)
        .filter(|w| w.len() > 2)
        .collect();
    let review: Vec<BookHit> = hits
        .iter()
        .filter(|h| {
            let t = norm(&h.title);
            words.iter().any(|w| t.contains(w.as_str()))
        })
        .take(8)
        .cloned()
        .collect();
    if review.is_empty() {
        BookDecision::None
    } else {
        BookDecision::Review(review)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: &str, title: &str, author: &str) -> BookHit {
        BookHit {
            id: id.into(),
            title: title.into(),
            authors: vec![author.into()],
            ..Default::default()
        }
    }

    #[test]
    fn names_and_titles_compare_loosely() {
        assert!(same_person("J. R. R. Tolkien", "J.R.R. Tolkien"));
        assert!(same_person("Tolkien, J. R. R.", "J. R. R. Tolkien"));
        assert!(same_person("John Ronald Reuel Tolkien", "J. R. R. Tolkien"));
        assert!(!same_person("Christopher Tolkien", "J. R. R. Tolkien"));
        assert!(!same_person("Jane Austen", "Jane Smith"));
        assert_eq!(title_key("The Hobbit: or There and Back Again"), "hobbit");
        assert_eq!(title_key("Hobbit"), "hobbit");
    }

    #[test]
    fn the_first_agreeing_work_matches() {
        let hits = [
            hit("OL1W", "Pride and Prejudice", "Jane Austen"),
            hit("OL2W", "Pride and Prejudice", "Jane Austen"),
        ];
        let d = decide(&hits, "Pride and Prejudice", &["Jane Austen".into()], false);
        assert_eq!(d, BookDecision::Matched(hits[0].clone()));
    }

    #[test]
    fn never_a_confident_wrong_match() {
        // The same title by someone else; a collection that contains the title.
        let hits = [
            hit("OL1W", "Emma", "Someone Else"),
            hit("OL2W", "Novels (Emma / Persuasion)", "Jane Austen"),
        ];
        assert!(matches!(
            decide(&hits, "Emma", &["Jane Austen".into()], false),
            BookDecision::Review(_)
        ));
        assert_eq!(
            decide(&[hit("OL3W", "Unrelated", "X")], "Emma", &[], false),
            BookDecision::None
        );
        // An ISBN that names another book.
        assert!(!matches!(
            decide(
                &[hit("OL4W", "Persuasion", "Jane Austen")],
                "Emma",
                &[],
                true
            ),
            BookDecision::Matched(_)
        ));
    }

    #[test]
    fn an_isbn_match_needs_only_the_title() {
        let hits = [hit("OL1W", "Pride and Prejudice", "Jane Austen")];
        assert!(matches!(
            decide(&hits, "Pride & Prejudice", &["J. Austen".into()], true),
            BookDecision::Matched(_)
        ));
    }
}
