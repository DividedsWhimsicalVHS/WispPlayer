//! Lyrics from LRCLIB (lrclib.net): reading its search answers and picking the entry that is the
//! same song as a row of the list. Pure, like `musicbrainz`: the request itself is in `net`.

use serde_json::Value;

use super::musicbrainz::{base_title, duration_fit, norm, similarity};

/// One lyrics record: a song as someone submitted it, with its text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub seconds: f32,
    pub instrumental: bool,
    pub plain: String,
    /// Timestamped (`[00:12.34] line`), the form Wisp follows along with.
    pub synced: String,
}

/// The text of a lyrics record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lyrics {
    pub plain: String,
    pub synced: String,
}

impl Lyrics {
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.plain.trim().is_empty() && self.synced.trim().is_empty()
    }
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// The records of a `/api/search` answer (a JSON array).
pub fn parse(v: &Value) -> Vec<Entry> {
    let Some(list) = v.as_array() else { return Vec::new() };
    list.iter()
        .map(|e| Entry {
            title: text(e, "trackName"),
            artist: text(e, "artistName"),
            album: text(e, "albumName"),
            seconds: e.get("duration").and_then(Value::as_f64).unwrap_or(0.0) as f32,
            instrumental: e.get("instrumental").and_then(Value::as_bool).unwrap_or(false),
            plain: text(e, "plainLyrics"),
            synced: text(e, "syncedLyrics"),
        })
        .filter(|e| !e.title.is_empty())
        .collect()
}

/// The credited names of an artist field: "Kanye West, Dwele" or "Kanye West feat. Dwele".
fn names(artist: &str) -> Vec<String> {
    artist.split([',', '&']).flat_map(|p| p.split(" feat. ")).flat_map(|p| p.split(" ft. ")).map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

/// The lyrics of the entry that is this song: the title (a "(Clean)" or "- Dirty" cut of it
/// counts for less), an artist among the credited names, about the same length, and, among equals,
/// one with timestamps. `None` when nothing matches or the song has no words.
pub fn best(entries: &[Entry], title: &str, artist: &str, album: &str, length_ms: Option<u32>) -> Option<Lyrics> {
    let wanted: Vec<String> = names(artist);
    let mut best: Option<(f32, &Entry)> = None;
    for e in entries {
        if e.instrumental || (e.plain.trim().is_empty() && e.synced.trim().is_empty()) {
            continue;
        }
        let t = similarity(&base_title(title), &base_title(&e.title)).max(similarity(title, &e.title));
        let exact = norm(title) == norm(&e.title);
        if t < 0.85 {
            continue;
        }
        let a = wanted.iter().flat_map(|w| names(&e.artist).into_iter().map(move |n| similarity(w, &n))).fold(0.0, f32::max);
        if a < 0.6 {
            continue;
        }
        // A cut of another length is another version of the song.
        let fit = match (length_ms, e.seconds) {
            (Some(l), s) if s > 0.0 => {
                let ms = (s * 1000.0) as u32;
                if (ms as i64 - l as i64).abs() > 10_000 {
                    continue;
                }
                duration_fit(l, Some(ms))
            }
            _ => 0.5,
        };
        let score = 0.45 * t + 0.05 * exact as u8 as f32 + 0.25 * a + 0.10 * similarity(album, &e.album) + 0.12 * fit + 0.03 * (!e.synced.trim().is_empty()) as u8 as f32;
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, e));
        }
    }
    best.map(|(_, e)| Lyrics { plain: e.plain.clone(), synced: e.synced.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(title: &str, artist: &str, album: &str, seconds: f64, synced: bool) -> Value {
        json!({
            "trackName": title, "artistName": artist, "albumName": album, "duration": seconds, "instrumental": false,
            "plainLyrics": format!("plain words of {title}"),
            "syncedLyrics": if synced { format!("[00:01.00] words of {title}") } else { String::new() },
        })
    }

    fn answer() -> Vec<Entry> {
        let mut instrumental = entry("Flashing Lights", "Kanye West", "Graduation", 237.0, true);
        instrumental["instrumental"] = json!(true);
        parse(&json!([
            entry("Flashing Lights (Clean)", "Kanye West", "Flashing Lights", 173.66, true),
            entry("Flashing Lights", "Kanye West, Dwele", "Flashing Lights", 238.0, false),
            entry("Flashing Lights", "Kanye West", "Graduation (UK Version)", 237.0, true),
            entry("Flashing Lights", "Some Cover Band", "Live", 237.0, true),
            entry("Flashing Lights - Dirty", "Kanye West", "Flashing Lights", 237.5, true),
            instrumental,
        ]))
    }

    #[test]
    fn the_same_song_of_the_same_artist_and_length_is_found_with_its_timestamps() {
        let l = best(&answer(), "Flashing Lights", "Kanye West feat. Dwele", "Graduation", Some(237_440)).unwrap();
        assert_eq!(l.synced, "[00:01.00] words of Flashing Lights", "the synced entry of the right length wins over the plain one");
        assert!(l.plain.contains("plain words"));
    }

    #[test]
    fn another_cut_another_artist_or_an_instrumental_is_not_this_song() {
        let only_clean = parse(&json!([entry("Flashing Lights (Clean)", "Kanye West", "x", 173.66, true)]));
        assert!(best(&only_clean, "Flashing Lights", "Kanye West", "", Some(237_440)).is_none(), "a shorter cut");
        let cover = parse(&json!([entry("Flashing Lights", "Some Cover Band", "x", 237.0, true)]));
        assert!(best(&cover, "Flashing Lights", "Kanye West", "", Some(237_440)).is_none(), "another artist");
        let mut instrumental = entry("Flashing Lights", "Kanye West", "x", 237.0, true);
        instrumental["instrumental"] = json!(true);
        assert!(best(&parse(&json!([instrumental])), "Flashing Lights", "Kanye West", "", None).is_none(), "no words");
    }

    #[test]
    fn without_a_length_the_title_and_artist_decide() {
        let l = best(&answer(), "Flashing Lights", "Kanye West", "", None).unwrap();
        assert!(!l.is_empty());
        assert!(parse(&json!({ "message": "not an array" })).is_empty());
    }

    #[test]
    fn credited_names_are_split_the_ways_people_write_them() {
        assert_eq!(names("Kanye West, Dwele"), ["Kanye West", "Dwele"]);
        assert_eq!(names("Kanye West feat. Dwele"), ["Kanye West", "Dwele"]);
        assert_eq!(names("Simon & Garfunkel"), ["Simon", "Garfunkel"]);
    }
}
