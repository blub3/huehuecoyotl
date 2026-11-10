//! Language tags. Containers use ISO 639-2/B ("ger"), file names often ISO 639-1 ("de"), some
//! tools ISO 639-2/T ("deu"). The core stores the B code and gives the player every variant.

/// (639-2/B, 639-2/T, 639-1, endonym)
const LANGS: &[(&str, &str, &str, &str)] = &[
    ("eng", "eng", "en", "English"),
    ("ger", "deu", "de", "Deutsch"),
    ("fre", "fra", "fr", "Français"),
    ("spa", "spa", "es", "Español"),
    ("ita", "ita", "it", "Italiano"),
    ("por", "por", "pt", "Português"),
    ("dut", "nld", "nl", "Nederlands"),
    ("swe", "swe", "sv", "Svenska"),
    ("nor", "nor", "no", "Norsk"),
    ("dan", "dan", "da", "Dansk"),
    ("fin", "fin", "fi", "Suomi"),
    ("pol", "pol", "pl", "Polski"),
    ("cze", "ces", "cs", "Čeština"),
    ("hun", "hun", "hu", "Magyar"),
    ("gre", "ell", "el", "Ελληνικά"),
    ("tur", "tur", "tr", "Türkçe"),
    ("rus", "rus", "ru", "Русский"),
    ("ukr", "ukr", "uk", "Українська"),
    ("ara", "ara", "ar", "العربية"),
    ("heb", "heb", "he", "עברית"),
    ("hin", "hin", "hi", "हिन्दी"),
    ("jpn", "jpn", "ja", "日本語"),
    ("kor", "kor", "ko", "한국어"),
    ("chi", "zho", "zh", "中文"),
    ("tha", "tha", "th", "ไทย"),
    ("vie", "vie", "vi", "Tiếng Việt"),
    ("ind", "ind", "id", "Bahasa Indonesia"),
];

fn find(tag: &str) -> Option<&'static (&'static str, &'static str, &'static str, &'static str)> {
    let t = tag.trim().to_ascii_lowercase();
    // "pt-BR", "en_US" -> primary subtag
    let primary = t.split(['-', '_']).next().unwrap_or("");
    LANGS
        .iter()
        .find(|(b, tt, one, _)| primary == *b || primary == *tt || primary == *one)
}

/// The canonical (639-2/B) code for a tag, or the lower-cased tag if unknown. Empty stays empty.
pub fn normalize(tag: &str) -> String {
    find(tag).map_or_else(|| tag.trim().to_ascii_lowercase(), |l| l.0.to_string())
}

/// Every spelling of the language, for mpv's `alang`/`slang` lists.
pub fn variants(tag: &str) -> Vec<String> {
    match find(tag) {
        Some((b, t, one, _)) => {
            let mut v = vec![b.to_string(), t.to_string(), one.to_string()];
            v.dedup();
            v
        }
        None if tag.trim().is_empty() => vec![],
        None => vec![tag.trim().to_ascii_lowercase()],
    }
}

/// Is this file-name token a language code we know?
pub fn is_language(token: &str) -> bool {
    find(token).is_some() && (token.len() == 2 || token.len() == 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(normalize("de"), "ger");
        assert_eq!(normalize("DEU"), "ger");
        assert_eq!(normalize("pt-BR"), "por");
        assert_eq!(normalize("tlh"), "tlh");
        assert_eq!(variants("ger"), ["ger", "deu", "de"]);
        assert_eq!(variants("eng"), ["eng", "en"]);
        assert!(variants("").is_empty());
        assert!(is_language("en") && !is_language("forced") && !is_language("english"));
    }
}
