//! Reading a file name, or text typed in its place, as an artist and a title.
//!
//! `04. Daft Punk - Around the World (Official Audio) [320kbps].mp3` reads as artist "Daft Punk",
//! title "Around the World". The order of the two halves is not reliable (`Title - Artist` is
//! common too), so [`hypotheses`] returns every plausible reading and the lookup tries them all.
//! [`cleaned`] is the same cleaning as text, to fill the editor's search box.

/// One reading of a file name.
#[derive(Clone, Debug, PartialEq)]
pub struct Guess {
    pub artist: Option<String>,
    pub title: String,
}

/// Words that make a bracketed part a release/upload tag rather than part of the title.
const NOISE: &[&str] = &[
    "official", "audio", "video", "lyric", "lyrics", "visualizer", "visualiser", "hq", "hd", "4k", "1080p", "720p",
    "480p", "kbps", "320", "256", "192", "128", "explicit", "clean", "remaster", "remastered", "mv", "m/v",
    "download", "free", "out", "now", "bonus", "full", "song", "only", "with", "music", "version", "upload",
    "copyright", "ncs", "release",
];

/// Words that make a bracketed part a version of the song, which matters for matching.
const VERSION: &[&str] = &[
    "remix", "mix", "edit", "live", "acoustic", "instrumental", "demo", "cover", "unplugged", "session", "extended",
    "rework", "bootleg", "dub", "orchestral", "piano", "reprise",
];

fn words(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !c.is_alphanumeric() && c != '/').filter(|w| !w.is_empty()).map(|w| w.to_lowercase())
}

/// A bracketed part that only labels the upload ("Official Video", "320kbps", "Remastered 2011").
fn is_noise(inner: &str, square: bool) -> bool {
    let ws: Vec<String> = words(inner).collect();
    if ws.is_empty() {
        return true;
    }
    if ws.iter().any(|w| VERSION.contains(&w.as_str())) {
        return false;
    }
    let digits = |w: &String| w.chars().all(|c| c.is_ascii_digit());
    let tag_like = |w: &String| NOISE.contains(&w.as_str()) || w.ends_with("kbps") || digits(w);
    // Square/curly brackets are almost always tags; parentheses only when they say so (or hold
    // nothing but a year).
    square || ws.iter().all(digits) || ws.iter().any(|w| NOISE.contains(&w.as_str()) || w.ends_with("kbps")) && ws.iter().all(tag_like)
}

fn is_feat(word: &str) -> bool {
    matches!(word.to_lowercase().trim_end_matches('.'), "feat" | "ft" | "featuring")
}

/// Removes bracketed noise and bracketed "(feat. X)". Brackets that name a version stay.
fn strip_brackets(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let close = match c {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            _ => None,
        };
        if let Some(close) = close {
            if let Some(len) = chars[i + 1..].iter().position(|&x| x == close) {
                let inner: String = chars[i + 1..i + 1 + len].iter().collect();
                let first = inner.split_whitespace().next().unwrap_or("");
                if is_feat(first) || is_noise(&inner, c != '(') {
                    i += len + 2;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// A bare "feat. X" runs to the end of its part: "A ft. B - Title" becomes "A - Title".
fn strip_bare_features(s: &str) -> String {
    s.split(" - ")
        .map(|part| part.split_whitespace().take_while(|w| !is_feat(w)).collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join(" - ")
}

/// "04. ", "1) ", "01 - ", "003 " and "Track 5 " at the start; "7 Rings" keeps its number.
fn strip_track_number(s: &str) -> &str {
    let t = s.trim_start();
    if t.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("track")) {
        let rest = t[5..].trim_start();
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 {
            let tail = rest[digits..].trim_start_matches(|c: char| c == '.' || c == '-' || c == ')' || c.is_whitespace());
            if !tail.is_empty() {
                return tail;
            }
        }
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 3 {
        return t;
    }
    let rest = &t[digits..];
    let marked = rest.starts_with('.') || rest.starts_with(')') || rest.trim_start().starts_with("- ") || rest.starts_with('-');
    let zero_padded = digits >= 2 && t.starts_with('0') && rest.starts_with(' ');
    if !(marked || zero_padded) {
        return t;
    }
    let tail = rest.trim_start_matches(|c: char| c == '.' || c == ')' || c == '-' || c == '_' || c.is_whitespace());
    if tail.is_empty() { t } else { tail }
}

fn tidy(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").trim_matches(|c: char| c == '-' || c == '.' || c == ',' || c.is_whitespace()).to_string()
}

/// Spaced dashes and other separators become " - "; a lone hyphen inside a word ("Jay-Z") stays.
fn normalize_separators(s: &str) -> String {
    let mut t = s.to_string();
    for sep in [" – ", " — ", " ~ ", " | ", " // ", " -- ", "–", "—"] {
        t = t.replace(sep, " - ");
    }
    // "Artist -Title" and "Artist- Title" are separators too.
    t = t.replace(" -", " - ").replace("- ", " - ");
    tidy(&t.split(" - ").map(str::trim).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" - "))
}

/// The cleaned file name, without track number, tags and "feat." credits.
fn clean(stem: &str) -> String {
    let mut s = stem.replace('_', " ");
    if !s.contains(' ') && s.matches('.').count() >= 2 {
        s = s.replace('.', " ");
    }
    let s = strip_brackets(&s);
    let s = normalize_separators(&s);
    let s = strip_bare_features(&s);
    tidy(strip_track_number(&s))
}

/// The file name as plain text, without track number, upload tags and "feat." credits:
/// `Daft Punk - Around the World`.
pub fn cleaned(stem: &str) -> String {
    clean(stem)
}

/// Plausible readings of a file name, most likely first. Never empty.
pub fn hypotheses(stem: &str) -> Vec<Guess> {
    let cleaned = clean(stem);
    let fallback = tidy(&stem.replace('_', " "));
    let parts: Vec<String> = cleaned
        .split(" - ")
        .map(|p| tidy(strip_track_number(p)))
        .filter(|p| !p.is_empty() && !p.chars().all(|c| c.is_ascii_digit()))
        .collect();
    let guess = |artist: &str, title: &str| Guess { artist: Some(artist.to_string()), title: title.to_string() };
    let mut out = Vec::new();
    match parts.as_slice() {
        [] => out.push(Guess { artist: None, title: if fallback.is_empty() { stem.to_string() } else { fallback } }),
        [only] => {
            out.push(Guess { artist: None, title: only.clone() });
            // "Artist-Title" with a bare hyphen: a weak reading, tried last.
            if let Some((a, b)) = only.split_once('-') {
                if a.trim().len() >= 2 && b.trim().len() >= 2 && !only.contains(' ') {
                    out.push(guess(a.trim(), b.trim()));
                    out.push(guess(b.trim(), a.trim()));
                }
            }
        }
        [a, b] => {
            out.push(guess(a, b));
            out.push(guess(b, a));
        }
        [first, middle @ .., last] => {
            out.push(guess(first, last));
            let rest = std::iter::once(middle.join(" - ")).chain(std::iter::once(last.clone())).collect::<Vec<_>>().join(" - ");
            out.push(guess(first, &rest));
            out.push(guess(last, first));
        }
    }
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(stem: &str) -> (Option<String>, String) {
        let g = hypotheses(stem).swap_remove(0);
        (g.artist, g.title)
    }

    fn pair(a: &str, t: &str) -> (Option<String>, String) {
        (Some(a.to_string()), t.to_string())
    }

    #[test]
    fn noise_numbers_and_separators_are_removed() {
        assert_eq!(first("04. Daft Punk - Around the World (Official Audio)"), pair("Daft Punk", "Around the World"));
        assert_eq!(first("Daft_Punk_-_Around_the_World_[320kbps]"), pair("Daft Punk", "Around the World"));
        assert_eq!(first("Daft.Punk.-.Around.the.World"), pair("Daft Punk", "Around the World"));
        assert_eq!(first("01 - Coldplay - Viva La Vida (Official Video) [HD]"), pair("Coldplay", "Viva La Vida"));
        assert_eq!(first("Queen - Bohemian Rhapsody (Remastered 2011)"), pair("Queen", "Bohemian Rhapsody"));
        assert_eq!(first("Beyoncé — Halo"), pair("Beyoncé", "Halo"));
        assert_eq!(first("Artist -Title"), pair("Artist", "Title"));
        assert_eq!(first("Track 05 - Moby - Porcelain"), pair("Moby", "Porcelain"));
    }

    #[test]
    fn featured_artists_are_dropped_but_versions_stay() {
        assert_eq!(first("Clean Bandit - Rather Be (feat. Jess Glynne) [Official Video]"), pair("Clean Bandit", "Rather Be"));
        assert_eq!(first("Daft Punk ft. Pharrell Williams - Get Lucky (Radio Edit)"), pair("Daft Punk", "Get Lucky (Radio Edit)"));
        assert_eq!(first("Disclosure - Latch feat. Sam Smith"), pair("Disclosure", "Latch"));
        assert_eq!(first("Lorde - Royals (Clean Bandit Remix)"), pair("Lorde", "Royals (Clean Bandit Remix)"));
    }

    #[test]
    fn titles_that_start_with_numbers_keep_them() {
        assert_eq!(first("7 Rings"), (None, "7 Rings".into()));
        assert_eq!(first("99 Luftballons"), (None, "99 Luftballons".into()));
        assert_eq!(first("Iron Maiden - 22 Acacia Avenue"), pair("Iron Maiden", "22 Acacia Avenue"));
    }

    #[test]
    fn both_orders_are_offered_for_the_lookup_to_decide() {
        let h = hypotheses("Around the World - Daft Punk");
        assert_eq!(h[0], Guess { artist: Some("Around the World".into()), title: "Daft Punk".into() });
        assert_eq!(h[1], Guess { artist: Some("Daft Punk".into()), title: "Around the World".into() });
        let h = hypotheses("Artist - Album Name - 03 - Song");
        assert_eq!(h[0], Guess { artist: Some("Artist".into()), title: "Song".into() });
        assert!(h.contains(&Guess { artist: Some("Song".into()), title: "Artist".into() }));
    }

    #[test]
    fn the_cleaned_text_is_what_the_search_box_starts_with() {
        assert_eq!(cleaned("04. Daft Punk - Around the World (Official Audio) [320kbps]"), "Daft Punk - Around the World");
        assert_eq!(cleaned("track_009"), "track 009");
        assert_eq!(cleaned("Kanye_West_-_Flashing_Lights_(feat._Dwele)"), "Kanye West - Flashing Lights");
    }

    #[test]
    fn cryptic_names_still_produce_a_title() {
        assert_eq!(first("track_009"), (None, "track 009".into()));
        assert_eq!(first("AUDIO_1234"), (None, "AUDIO 1234".into()));
        assert_eq!(first("[]"), (None, "[]".into()));
    }
}
