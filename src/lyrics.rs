//! Lyrics: synchronized (`.lrc`) or plain, from a sidecar file or the audio file's own tags.

use std::path::Path;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Line {
    /// Start time; meaningless (0) for unsynchronized lyrics.
    pub time_ms: u32,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    pub synced: bool,
}

/// `mm:ss`, `mm:ss.x`, `mm:ss.xx`, `mm:ss.xxx` or `mm:ss:xx` -> milliseconds.
fn parse_stamp(tag: &str) -> Option<i64> {
    let (min, rest) = tag.split_once(':')?;
    let (sec, frac) = match rest.split_once(['.', ':']) {
        Some((s, f)) => (s, f),
        None => (rest, ""),
    };
    if min.is_empty() || sec.is_empty() || !min.bytes().all(|b| b.is_ascii_digit()) || !sec.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if sec.len() > 2 || frac.len() > 3 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let ms = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().ok()? * 100,
        2 => frac.parse::<i64>().ok()? * 10,
        _ => frac.parse::<i64>().ok()?,
    };
    Some(min.parse::<i64>().ok()? * 60_000 + sec.parse::<i64>().ok()? * 1000 + ms)
}

fn is_meta_tag(tag: &str) -> bool {
    let key = tag.split(':').next().unwrap_or("").trim().to_ascii_lowercase();
    matches!(key.as_str(), "ar" | "ti" | "al" | "au" | "by" | "re" | "ve" | "length" | "la" | "id" | "tool" | "offset" | "lang")
}

/// Removes enhanced-LRC word stamps such as `<00:12.30>`.
fn strip_word_stamps(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('<') {
        let Some(len) = rest[start..].find('>') else { break };
        if parse_stamp(&rest[start + 1..start + len]).is_some() {
            out.push_str(&rest[..start]);
            rest = &rest[start + len + 1..];
        } else {
            out.push_str(&rest[..start + 1]);
            rest = &rest[start + 1..];
        }
    }
    out.push_str(rest);
    out
}

/// Parses LRC or plain text; the result says which one it was.
pub fn parse(text: &str) -> Lyrics {
    let text = text.trim_start_matches('\u{feff}');
    let mut offset = 0i64;
    let mut timed: Vec<(i64, String)> = Vec::new();
    let mut plain: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        let mut rest = line;
        let mut stamps = Vec::new();
        let mut meta = false;
        while let Some(r) = rest.strip_prefix('[') {
            let Some(end) = r.find(']') else { break };
            let tag = &r[..end];
            if let Some(ms) = parse_stamp(tag) {
                stamps.push(ms);
            } else if is_meta_tag(tag) {
                meta = true;
                if let Some(v) = tag.strip_prefix("offset:") {
                    offset = v.trim().parse().unwrap_or(0);
                }
            } else {
                break; // a section label like [Chorus] is part of the text
            }
            rest = &r[end + 1..];
        }
        if stamps.is_empty() {
            if !meta {
                plain.push(line.to_string());
            }
        } else {
            let text = strip_word_stamps(rest).trim().to_string();
            for ms in stamps {
                timed.push((ms, text.clone()));
            }
        }
    }
    if timed.is_empty() {
        // Collapse runs of blank lines and trim the ends.
        let mut lines: Vec<Line> = Vec::new();
        for l in plain {
            if l.is_empty() && lines.last().is_none_or(|p| p.text.is_empty()) {
                continue;
            }
            lines.push(Line { time_ms: 0, text: l });
        }
        while lines.last().is_some_and(|l| l.text.is_empty()) {
            lines.pop();
        }
        return Lyrics { lines, synced: false };
    }
    // A positive LRC offset makes lyrics appear earlier.
    timed.sort_by_key(|(ms, _)| *ms);
    let lines = timed
        .into_iter()
        .map(|(ms, text)| Line { time_ms: (ms - offset).clamp(0, u32::MAX as i64) as u32, text })
        .collect();
    Lyrics { lines, synced: true }
}

/// Index of the line that is current at `pos_ms` (the last one that has started).
pub fn active_line(lines: &[Line], pos_ms: u32) -> Option<usize> {
    lines.partition_point(|l| l.time_ms <= pos_ms).checked_sub(1)
}

/// UTF-8 (with or without BOM), UTF-16 with BOM; anything else degrades gracefully.
fn decode_text(bytes: &[u8]) -> String {
    let utf16 = |b: &[u8], be: bool| {
        let units: Vec<u16> =
            b.chunks_exact(2).map(|c| if be { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) }).collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, false),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn read_sidecar(audio: &Path, ext: &str) -> Option<String> {
    let bytes = std::fs::read(audio.with_extension(ext)).ok()?;
    (bytes.len() < 4 << 20).then(|| decode_text(&bytes))
}

fn non_empty(l: Lyrics) -> Option<Lyrics> {
    l.lines.iter().any(|l| !l.text.trim().is_empty()).then_some(l)
}

/// Looks for lyrics: `song.lrc` next to the file, then the file's own lyrics tag (which may itself
/// be LRC), then `song.txt`.
pub fn load(audio: &Path, embedded: Option<&str>) -> Option<Lyrics> {
    if let Some(l) = read_sidecar(audio, "lrc").map(|t| parse(&t)).and_then(non_empty) {
        return Some(l);
    }
    if let Some(l) = embedded.map(parse).and_then(non_empty) {
        return Some(l);
    }
    read_sidecar(audio, "txt").map(|t| parse(&t)).and_then(non_empty)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(l: &Lyrics) -> Vec<&str> {
        l.lines.iter().map(|l| l.text.as_str()).collect()
    }

    #[test]
    fn parses_timestamps_in_every_common_spelling() {
        assert_eq!(parse_stamp("00:12.34"), Some(12_340));
        assert_eq!(parse_stamp("01:02.5"), Some(62_500));
        assert_eq!(parse_stamp("1:02"), Some(62_000));
        assert_eq!(parse_stamp("120:00.000"), Some(7_200_000));
        assert_eq!(parse_stamp("00:12:34"), Some(12_340), "colon before the fraction");
        assert_eq!(parse_stamp("ar:Someone"), None);
        assert_eq!(parse_stamp("00:12.3456"), None);
        assert_eq!(parse_stamp(":12"), None);
    }

    #[test]
    fn synced_lyrics_with_metadata_offsets_and_repeated_stamps() {
        let l = parse(
            "\u{feff}[ti:Song]\n[ar:Artist]\n[offset:+500]\n[00:10.00]First line\n[00:05.00][00:20.00]Chorus\n[00:30.00]\n[Chorus]\n[00:40.5]<00:40.5>Word <00:41.0>by word\n",
        );
        assert!(l.synced);
        assert_eq!(texts(&l), ["Chorus", "First line", "Chorus", "", "Word by word"]);
        let times: Vec<u32> = l.lines.iter().map(|l| l.time_ms).collect();
        assert_eq!(times, [4_500, 9_500, 19_500, 29_500, 40_000], "a +500 ms offset shows lines 500 ms earlier");
    }

    #[test]
    fn plain_lyrics_keep_section_labels_and_collapse_blank_runs() {
        let l = parse("\n\n[Verse 1]\nHello there\n\n\n\nSecond verse\n\n");
        assert!(!l.synced);
        assert_eq!(texts(&l), ["[Verse 1]", "Hello there", "", "Second verse"]);
    }

    #[test]
    fn active_line_follows_playback_position() {
        let l = parse("[00:01.00]a\n[00:03.00]b\n[00:03.00]b2\n[00:09.00]c");
        assert_eq!(active_line(&l.lines, 0), None, "before the first line");
        assert_eq!(active_line(&l.lines, 1_000), Some(0));
        assert_eq!(active_line(&l.lines, 2_999), Some(0));
        assert_eq!(active_line(&l.lines, 3_000), Some(2), "simultaneous lines: the later one is active");
        assert_eq!(active_line(&l.lines, 999_999), Some(3));
        assert_eq!(active_line(&[], 5), None);
    }

    #[test]
    fn text_decoding_handles_utf16_and_bom() {
        let mut utf16 = vec![0xFF, 0xFE];
        for u in "[00:01.00]héllo ♪".encode_utf16() {
            utf16.extend(u.to_le_bytes());
        }
        assert_eq!(decode_text(&utf16), "[00:01.00]héllo ♪");
        assert_eq!(decode_text(b"\xEF\xBB\xBFabc"), "abc");
        assert_eq!(decode_text("日本語".as_bytes()), "日本語");
    }

    #[test]
    fn sidecar_beats_embedded_which_beats_txt_and_nothing_is_none() {
        let dir = std::env::temp_dir().join(format!("wisp-lyrics-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let audio = dir.join("song.flac");
        assert!(load(&audio, None).is_none(), "no lyrics anywhere");
        assert!(load(&audio, Some("   \n ")).is_none(), "blank embedded lyrics are not lyrics");
        std::fs::write(dir.join("song.txt"), "from txt").unwrap();
        assert_eq!(texts(&load(&audio, None).unwrap()), ["from txt"]);
        assert_eq!(texts(&load(&audio, Some("[00:01.00]embedded")).unwrap()), ["embedded"]);
        std::fs::write(dir.join("song.lrc"), "[00:02.00]sidecar").unwrap();
        let l = load(&audio, Some("[00:01.00]embedded")).unwrap();
        assert!(l.synced && texts(&l) == ["sidecar"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
