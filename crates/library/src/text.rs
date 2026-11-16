//! Title keys shared by the scanner and the library (issue #63: the library's migration and
//! merges must compute the same keys as the scanner).

/// Lower-case alphanumerics and single spaces: the key that makes "The.Matrix" and
/// "the matrix" the same title.
pub fn normalize(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Sort without leading articles: "The Matrix" sorts under M.
pub fn sort_title(title: &str) -> String {
    let l = title.trim().to_lowercase();
    for a in [
        "the ", "a ", "an ", "der ", "die ", "das ", "le ", "la ", "les ", "el ",
    ] {
        if let Some(rest) = l.strip_prefix(a)
            && !rest.is_empty()
        {
            return rest.to_string();
        }
    }
    l
}
