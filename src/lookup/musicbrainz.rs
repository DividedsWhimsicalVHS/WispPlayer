//! Searching MusicBrainz for what a track might be, and ranking the releases that carry it.
//!
//! Everything here is pure (no network): the searches to send, how to read the JSON that comes
//! back, and how the matching releases are ranked for the list the user chooses from. Nothing is
//! ever applied automatically; the ranking only decides the order of the list.
//!
//! A popular title has hundreds of recordings, and a plain title search returns an arbitrary 25 of
//! them, so when the file's length is known the same search is also made among recordings of about
//! that length, which is the strongest thing a file tells about itself.

use serde_json::Value;

use crate::library::filename::Guess;

/// How many rows the list shows.
const MAX_ROWS: usize = 12;
/// How many releases of one recording get a row.
const RELEASES_PER_RECORDING: usize = 3;
/// Recordings this close in length to the file are "the same length".
pub const SAME_LENGTH_MS: u32 = 3_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Release {
    pub id: String,
    pub group_id: String,
    pub title: String,
    /// The album as a whole ("Graduation"), where the release may be a particular edition of it.
    pub group_title: String,
    /// The release's own artist credit when it differs from the recording's ("Various Artists").
    pub artist: Option<String>,
    pub status: String,
    pub primary_type: String,
    pub secondary: Vec<String>,
    pub date: String,
    pub track_no: u16,
    pub disc_no: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Candidate {
    pub title: String,
    /// The full credit as printed: "Kanye West feat. Dwele".
    pub artist: String,
    /// Each credited artist on its own, for matching.
    pub artist_names: Vec<String>,
    pub length_ms: Option<u32>,
    pub first_release: String,
    pub releases: Vec<Release>,
}

/// One line of the list: a song on a release.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub title: String,
    /// The full credit, "primary/featured artist(s)".
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub year: u16,
    pub track_no: u16,
    pub disc_no: u16,
    pub length_ms: Option<u32>,
    /// The release's own id and its album's, for the cover.
    pub release_id: String,
    pub group_id: String,
    /// The recording is about as long as the file.
    pub length_match: bool,
}

// -- text comparison ------------------------------------------------------------------------

/// Lower-case, accents and punctuation removed, "&" read as "and", a leading "the" dropped.
pub fn norm(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in crate::library::fold(s).chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if c == '&' {
            out.push_str(" and ");
        } else {
            out.push(' ');
        }
    }
    let t = out.split_whitespace().collect::<Vec<_>>().join(" ");
    match t.strip_prefix("the ") {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ => t,
    }
}

/// Without bracketed parts: "Get Lucky (Radio Edit)" -> "Get Lucky".
pub fn base_title(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Dice coefficient over character pairs of the normalized strings: 1.0 for equal text, robust
/// to small typos and extra words.
pub fn similarity(a: &str, b: &str) -> f32 {
    let (a, b) = (norm(a), norm(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let pairs = |s: &str| {
        let cs: Vec<char> = s.chars().collect();
        cs.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>()
    };
    let (x, mut y) = (pairs(&a), pairs(&b));
    if x.is_empty() || y.is_empty() {
        return 0.0;
    }
    let total = x.len() + y.len();
    let mut common = 0;
    for p in &x {
        if let Some(i) = y.iter().position(|q| q == p) {
            y.swap_remove(i);
            common += 1;
        }
    }
    2.0 * common as f32 / total as f32
}

fn title_similarity(guess: &str, title: &str) -> f32 {
    let full = similarity(guess, title);
    let base = similarity(&base_title(guess), &base_title(title)) * 0.95;
    full.max(base)
}

fn artist_similarity(guess: &str, c: &Candidate) -> f32 {
    c.artist_names.iter().chain(std::iter::once(&c.artist)).map(|n| similarity(guess, n)).fold(0.0, f32::max)
}

/// How well a recording's length fits the file's, 0..1 (0.5 when either is unknown).
pub fn duration_fit(file_ms: u32, length: Option<u32>) -> f32 {
    match length {
        Some(l) if file_ms > 0 && l > 0 => match (file_ms as i64 - l as i64).abs() {
            0..=1_000 => 1.0,
            1_001..=3_000 => 0.85,
            3_001..=8_000 => 0.6,
            8_001..=20_000 => 0.3,
            _ => 0.0,
        },
        _ => 0.5,
    }
}

// -- the searches ---------------------------------------------------------------------------

fn escape_phrase(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The searches for what the user typed, most useful first.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The song among official, non-live releases (live takes and bootlegs would crowd out the
    /// studio recording in a list of 25).
    pub official: String,
    /// The song among recordings of about the file's length, when that is known.
    pub by_length: Option<String>,
    /// Anything at all, for a song that exists only on live or unofficial releases.
    pub anything: String,
}

/// Searches for each reading of the text (artist and title may be either way round).
pub fn plan(readings: &[Guess], ms: u32) -> Plan {
    let song = readings
        .iter()
        .take(2)
        .map(|g| match &g.artist {
            Some(a) => format!("(recording:\"{}\" AND artist:\"{}\")", escape_phrase(&g.title), escape_phrase(a)),
            None => format!("recording:\"{}\"", escape_phrase(&g.title)),
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    let song = format!("({song})");
    Plan {
        official: format!("{song} AND status:official AND NOT secondarytype:live"),
        by_length: (ms > 0).then(|| {
            let (lo, hi) = (ms.saturating_sub(4_000), ms.saturating_add(4_000));
            format!("{song} AND dur:[{lo} TO {hi}]")
        }),
        anything: song,
    }
}

// -- reading the response -------------------------------------------------------------------

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn credit(v: &Value) -> (String, Vec<String>) {
    let mut full = String::new();
    let mut names = Vec::new();
    for c in credits(v) {
        let name = text(c, "name");
        full.push_str(&name);
        full.push_str(&text(c, "joinphrase"));
        names.push(name);
        if let Some(n) = c.get("artist").and_then(|a| a.get("name")).and_then(Value::as_str) {
            if !names.iter().any(|x| x == n) {
                names.push(n.to_string());
            }
        }
    }
    (full, names)
}

fn credits(v: &Value) -> impl Iterator<Item = &Value> {
    v.get("artist-credit").and_then(Value::as_array).into_iter().flatten()
}

fn number(s: &str) -> u16 {
    let digits: String = s.chars().filter(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

fn release(r: &Value) -> Release {
    let group = r.get("release-group").unwrap_or(&Value::Null);
    let (artist, _) = credit(r);
    let medium = r.get("media").and_then(Value::as_array).and_then(|m| m.first()).unwrap_or(&Value::Null);
    let track = medium.get("track").and_then(Value::as_array).and_then(|t| t.first()).unwrap_or(&Value::Null);
    // The position on the disc; the printed number can be a vinyl side ("B4") or missing.
    let track_no = match medium.get("track-offset").and_then(Value::as_u64) {
        Some(offset) => offset.min(u16::MAX as u64 - 1) as u16 + 1,
        None => number(&text(track, "number")),
    };
    Release {
        id: text(r, "id"),
        group_id: text(group, "id"),
        title: text(r, "title"),
        group_title: text(group, "title"),
        artist: (!artist.is_empty()).then_some(artist),
        status: text(r, "status"),
        primary_type: text(group, "primary-type"),
        secondary: group
            .get("secondary-types")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default(),
        date: text(r, "date"),
        track_no,
        disc_no: medium.get("position").and_then(Value::as_u64).unwrap_or(1) as u16,
    }
}

/// The recordings of a `/ws/2/recording` search response.
pub fn parse_recordings(v: &Value) -> Vec<Candidate> {
    let Some(list) = v.get("recordings").and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .map(|r| {
            let (artist, artist_names) = credit(r);
            Candidate {
                title: text(r, "title"),
                artist,
                artist_names,
                length_ms: r.get("length").and_then(Value::as_u64).map(|l| l.min(u32::MAX as u64) as u32),
                first_release: text(r, "first-release-date"),
                releases: r.get("releases").and_then(Value::as_array).map(|a| a.iter().map(release).collect()).unwrap_or_default(),
            }
        })
        .filter(|c| !c.title.is_empty())
        .collect()
}

// -- ranking --------------------------------------------------------------------------------

/// Prefers the official studio album over singles, EPs, compilations, soundtracks and live or
/// bootleg releases, and the artist's own release over "Various Artists".
fn release_rank(r: &Release, artist: &str) -> i32 {
    let mut k = match r.status.to_ascii_lowercase().as_str() {
        "official" => 100,
        "" => 60,
        _ => 0,
    };
    k += match r.primary_type.as_str() {
        "Album" => 40,
        "EP" => 30,
        "Single" => 20,
        "" => 5,
        _ => 10,
    };
    k -= 25 * r.secondary.len() as i32;
    match &r.artist {
        Some(a) if norm(a) == "various artists" => k -= 40,
        Some(a) if similarity(a, artist) < 0.8 => k -= 15,
        _ => k += 10,
    }
    k
}

fn year(date: &str) -> u16 {
    let y: String = date.chars().take(4).collect();
    if y.len() == 4 { y.parse().unwrap_or(0) } else { 0 }
}

fn primary_artist(c: &Candidate) -> String {
    c.artist_names.first().cloned().unwrap_or_else(|| c.artist.clone())
}

/// The list to choose from: every plausible song on each of its best releases, best first. A
/// reading's artist and title count most, then the length (a recording within a second of the
/// file is very likely the same take), then how much the release looks like the song's album
/// (official studio albums first, compilations last), then how widely the recording was released.
pub fn rank(readings: &[Guess], candidates: &[Candidate], ms: u32) -> Vec<Row> {
    let mut scored: Vec<(f32, Row)> = Vec::new();
    for c in candidates {
        let base = readings
            .iter()
            .map(|g| {
                let artist = g.artist.as_deref().map_or(0.5, |a| artist_similarity(a, c));
                0.45 * title_similarity(&g.title, &c.title) + 0.28 * artist
            })
            .fold(0.0, f32::max);
        let fit = duration_fit(ms, c.length_ms);
        let popularity = (c.releases.len().min(20) as f32) / 20.0;
        let artist = primary_artist(c);
        let mut releases: Vec<(i32, &Release)> = c.releases.iter().map(|r| (release_rank(r, &artist), r)).collect();
        releases.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.date.cmp(&b.1.date)));
        let make = |r: Option<&Release>, rank: i32| {
            let album = r.map(|r| if r.group_title.is_empty() { r.title.clone() } else { r.group_title.clone() }).unwrap_or_default();
            let album_artist = r.and_then(|r| r.artist.clone()).filter(|a| norm(a) != "various artists").unwrap_or_else(|| artist.clone());
            let release_part = if r.is_some() { ((rank + 100) as f32 / 250.0).clamp(0.0, 1.0) } else { 0.0 };
            let score = base + 0.12 * fit + 0.12 * release_part + 0.04 * popularity;
            let row = Row {
                title: c.title.clone(),
                artist: if c.artist.is_empty() { artist.clone() } else { c.artist.clone() },
                album,
                album_artist,
                year: r.map(|r| year(&r.date)).filter(|y| *y > 0).unwrap_or_else(|| year(&c.first_release)),
                track_no: r.map_or(0, |r| r.track_no),
                disc_no: r.map_or(0, |r| r.disc_no),
                length_ms: c.length_ms,
                release_id: r.map(|r| r.id.clone()).unwrap_or_default(),
                group_id: r.map(|r| r.group_id.clone()).unwrap_or_default(),
                length_match: ms > 0 && c.length_ms.is_some_and(|l| (ms as i64 - l as i64).unsigned_abs() <= SAME_LENGTH_MS as u64),
            };
            (score, row)
        };
        if releases.is_empty() {
            scored.push(make(None, 0));
        }
        for (rank, r) in releases.into_iter().take(RELEASES_PER_RECORDING) {
            scored.push(make(Some(r), rank));
        }
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    // One row per song and album: several recordings and editions of the same thing add nothing.
    let mut seen: Vec<(String, String, String)> = Vec::new();
    let mut rows = Vec::new();
    for (_, row) in scored {
        let key = (norm(&base_title(&row.title)), norm(&row.artist), norm(&row.album));
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        rows.push(row);
        if rows.len() == MAX_ROWS {
            break;
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn guess(artist: Option<&str>, title: &str) -> Guess {
        Guess { artist: artist.map(str::to_string), title: title.to_string() }
    }

    fn rel(id: &str, title: &str, status: &str, kind: &str, secondary: &[&str], date: &str, track: u32, va: bool) -> Value {
        let mut r = json!({
            "id": id, "title": title, "status": status, "date": date,
            "release-group": { "id": format!("rg-{id}"), "title": title, "primary-type": kind, "secondary-types": secondary },
            "media": [{ "position": 1, "track-offset": track - 1, "track": [{ "number": track.to_string(), "title": "x" }] }],
        });
        if va {
            r["artist-credit"] = json!([{ "name": "Various Artists", "joinphrase": "" }]);
        }
        r
    }

    fn rec(title: &str, credit: &[(&str, &str)], length: u64, releases: Vec<Value>) -> Value {
        let credits: Vec<Value> = credit.iter().map(|(n, j)| json!({ "name": n, "joinphrase": j, "artist": { "name": n } })).collect();
        json!({ "title": title, "length": length, "score": 100, "first-release-date": "2007-09-11", "artist-credit": credits, "releases": releases })
    }

    /// Shaped like a real answer for "Flashing Lights" among recordings of about 3:57: the studio
    /// recording on its album and many compilations, a namesake by an unknown artist, and a live take.
    fn flashing_lights() -> Vec<Candidate> {
        let compilations: Vec<Value> =
            (0..8).map(|i| rel(&format!("promo{i}"), &format!("Promo Only: Urban Radio {i}"), "Official", "Album", &["Compilation"], "2008-01-01", 3, true)).collect();
        let mut studio = vec![
            rel("grad-us", "Graduation", "Official", "Album", &[], "2007-09-11", 9, false),
            rel("grad-uk", "Graduation", "Official", "Album", &[], "2007-09-10", 9, false),
            rel("single", "Flashing Lights", "Official", "Single", &[], "2008-01-01", 1, false),
        ];
        studio.extend(compilations);
        let v = json!({ "recordings": [
            rec("Flashing Lights", &[("Chelan", "")], 239_000, vec![rel("towers", "Towers", "Official", "Album", &[], "2013-05-01", 9, false)]),
            rec("Flashing Lights", &[("Kanye West", " feat. "), ("Dwele", "")], 237_440, studio),
            rec("Flashing Lights", &[("Kanye West", "")], 401_000, vec![rel("vh1", "VH1 Storytellers", "Official", "Album", &["Live"], "2008-07-01", 4, false)]),
        ]});
        parse_recordings(&v)
    }

    #[test]
    fn normalization_and_similarity() {
        assert_eq!(norm("The Beatles"), "beatles");
        assert_eq!(norm("Beyoncé & Jay-Z!"), "beyonce and jay z");
        assert_eq!(similarity("Around the World", "around the world"), 1.0);
        assert!(similarity("Arround the Wrld", "Around the World") > 0.75);
        assert!(similarity("Daft Punk", "Red Hot Chili Peppers") < 0.3);
        assert_eq!(similarity("", "x"), 0.0);
    }

    #[test]
    fn the_length_must_be_close_to_count_for_much() {
        assert_eq!(duration_fit(237_490, Some(237_440)), 1.0);
        assert_eq!(duration_fit(237_490, Some(239_000)), 0.85, "1.5 s off is a worse fit than 0.05 s off");
        assert_eq!(duration_fit(237_490, Some(300_000)), 0.0);
        assert_eq!(duration_fit(0, Some(300_000)), 0.5, "no length, no evidence");
        assert_eq!(duration_fit(237_490, None), 0.5);
    }

    #[test]
    fn searches_cover_both_orders_the_official_releases_and_the_files_length() {
        let p = plan(&[guess(Some("Kanye West"), "Flashing Lights"), guess(Some("Flashing Lights"), "Kanye West")], 237_490);
        assert_eq!(
            p.official,
            "((recording:\"Flashing Lights\" AND artist:\"Kanye West\") OR (recording:\"Kanye West\" AND artist:\"Flashing Lights\")) AND status:official AND NOT secondarytype:live"
        );
        let by_length = p.by_length.unwrap();
        assert!(by_length.ends_with("AND dur:[233490 TO 241490]"), "{by_length}");
        assert!(plan(&[guess(None, "Intro")], 0).by_length.is_none(), "no length, no length search");
        assert_eq!(plan(&[guess(None, "Say \"Hi\"")], 0).anything, "(recording:\"Say \\\"Hi\\\"\")");
    }

    /// The case that went wrong when songs were filed automatically: the title alone cannot say
    /// whose "Flashing Lights" this is, so every candidate is listed, the one that fits the file
    /// best and is on the album first.
    #[test]
    fn the_list_puts_the_studio_album_first_and_keeps_the_namesake_visible() {
        let rows = rank(&[guess(None, "Flashing Lights")], &flashing_lights(), 237_490);
        assert_eq!((rows[0].artist.as_str(), rows[0].album.as_str(), rows[0].year, rows[0].track_no), ("Kanye West feat. Dwele", "Graduation", 2007, 9));
        assert!(rows[0].length_match);
        assert!(rows.iter().any(|r| r.artist == "Chelan" && r.album == "Towers"), "the namesake is on the list, below");
        let chelan = rows.iter().position(|r| r.artist == "Chelan").unwrap();
        assert!(chelan > 0);
        // A live take and compilations come after the studio album.
        let live = rows.iter().position(|r| r.album == "VH1 Storytellers").unwrap();
        assert!(live > 0 && !rows[live].length_match);
        assert!(rows.len() <= MAX_ROWS);
    }

    #[test]
    fn the_artist_typed_decides_between_namesakes() {
        let rows = rank(&[guess(Some("Chelan"), "Flashing Lights")], &flashing_lights(), 237_490);
        assert_eq!((rows[0].artist.as_str(), rows[0].album.as_str()), ("Chelan", "Towers"));
        let rows = rank(&[guess(Some("Flashing Lights"), "Chelan"), guess(Some("Chelan"), "Flashing Lights")], &flashing_lights(), 0);
        assert_eq!(rows[0].artist, "Chelan", "the order the artist and title were typed in does not matter");
    }

    #[test]
    fn editions_of_one_album_collapse_into_one_row() {
        let rows = rank(&[guess(Some("Kanye West"), "Flashing Lights")], &flashing_lights(), 237_490);
        let graduation = rows.iter().filter(|r| r.artist.starts_with("Kanye West feat") && r.album == "Graduation").count();
        assert_eq!(graduation, 1, "the US and UK editions are one album");
    }

    #[test]
    fn a_recording_on_no_release_is_still_listed() {
        let v = json!({ "recordings": [rec("Loose Song", &[("Someone", "")], 100_000, vec![])] });
        let rows = rank(&[guess(None, "Loose Song")], &parse_recordings(&v), 0);
        assert_eq!((rows[0].title.as_str(), rows[0].album.as_str(), rows[0].release_id.as_str()), ("Loose Song", "", ""));
        assert_eq!(rows[0].year, 2007, "from the recording's first release date");
        assert!(rank(&[guess(None, "x")], &[], 0).is_empty());
    }
}
