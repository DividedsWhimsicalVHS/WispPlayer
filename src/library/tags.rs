//! Reads the tags and duration the library index needs. Opens the container only: no decoding.
//!
//! It also notes what the tags leave out (see [`flag`]), so that Settings can list the tracks that
//! still need an artist, a proper title or a cover. Nothing is guessed from the file name here:
//! a track without a title is listed under its file name, and flagged.

use std::path::Path;

use symphonia::core::formats::TrackType;
use symphonia::core::meta::{MetadataRevision, StandardTag};

use super::{flag, fold};
use crate::player::{can_decode, open_format, track_duration_ms};

#[derive(Debug, Default, PartialEq)]
pub struct Meta {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track_no: u16,
    pub disc_no: u16,
    pub year: u16,
    pub duration_ms: u32,
    /// What the tags leave out, as [`flag`] bits.
    pub flags: u8,
}

/// "1999", "1999-04-05", "1999/04" -> 1999.
fn year_of(date: &str) -> u16 {
    let digits: String = date.trim().chars().take_while(char::is_ascii_digit).collect();
    if digits.len() == 4 { digits.parse().unwrap_or(0) } else { 0 }
}

/// Reads one metadata revision into `m`. Returns whether it carried a picture.
fn absorb(m: &mut Meta, rev: &MetadataRevision) -> bool {
    let tags = rev.media.tags.iter().chain(rev.per_track.iter().flat_map(|t| t.metadata.tags.iter()));
    // The first non-empty value wins: the container's own tags come before trailing ID3v1.
    let set = |slot: &mut String, v: &str| {
        let v = v.trim();
        if slot.is_empty() && !v.is_empty() {
            *slot = v.to_string();
        }
    };
    for tag in tags {
        match &tag.std {
            Some(StandardTag::TrackTitle(s)) => set(&mut m.title, s),
            Some(StandardTag::Artist(s)) => set(&mut m.artist, s),
            Some(StandardTag::Album(s)) => set(&mut m.album, s),
            Some(StandardTag::AlbumArtist(s)) => set(&mut m.album_artist, s),
            Some(StandardTag::TrackNumber(n)) if m.track_no == 0 => m.track_no = (*n).min(u16::MAX as u64) as u16,
            Some(StandardTag::DiscNumber(n)) if m.disc_no == 0 => m.disc_no = (*n).min(u16::MAX as u64) as u16,
            Some(StandardTag::RecordingYear(y) | StandardTag::ReleaseYear(y) | StandardTag::OriginalReleaseYear(y))
                if m.year == 0 =>
            {
                m.year = *y;
            }
            Some(StandardTag::RecordingDate(d) | StandardTag::ReleaseDate(d) | StandardTag::OriginalReleaseDate(d))
                if m.year == 0 =>
            {
                m.year = year_of(d);
            }
            _ => {}
        }
    }
    !rev.media.visuals.is_empty() || rev.per_track.iter().any(|t| !t.metadata.visuals.is_empty())
}

/// An artist tag that says nothing.
pub fn unknown_artist(artist: &str) -> bool {
    let a = fold(artist);
    let a = a.trim();
    a.is_empty() || ["unknown artist", "unknown", "<unknown>", "[unknown]", "no artist", "n/a", "none", "?"].contains(&a)
}

/// Words that rippers and recorders use for the title of a track they could not name.
const GENERIC_TITLES: &[&str] = &["track", "audio", "audiotrack", "recording", "newrecording", "voice", "untitled", "unknown", "noname"];

/// Phrases that mark a title as copied from a download's file name.
const UPLOAD_JUNK: &[&str] = &[
    "official video", "official audio", "official music video", "official lyric video", "lyric video", "lyrics video", "kbps",
    "[hq]", "[hd]", "(hd)", "(hq)", "free download", "full album",
];

/// Whether a title tag is really a file name: `track_01`, `AUDIO_294`, `Untitled 3`, `01 - Song`,
/// `Song.mp3`, `Daft_Punk_-_Around_the_World`, or a download's "(Official Video)" and "[320kbps]".
/// A title that merely equals the file's name is fine: many people name files after the song.
pub fn looks_like_filename(title: &str) -> bool {
    let t = title.trim();
    if t.is_empty() {
        return true;
    }
    let lower = t.to_lowercase();
    // An extension, or the junk of an upload.
    if [".mp3", ".flac", ".wav", ".m4a", ".mp4", ".ogg", ".opus", ".aac", ".webm", ".mkv", ".wma"].iter().any(|e| lower.ends_with(e)) {
        return true;
    }
    if UPLOAD_JUNK.iter().any(|j| lower.contains(j)) {
        return true;
    }
    // Words glued by underscores, with no spaces: a name as a file system wants it.
    if !t.contains(' ') && t.split('_').filter(|w| !w.is_empty()).count() >= 2 {
        return true;
    }
    let letters: String = lower.chars().filter(|c| c.is_alphabetic()).collect();
    let digits = lower.chars().filter(char::is_ascii_digit).count();
    let plain = lower.chars().all(|c| c.is_alphanumeric() || " _-.#".contains(c));
    // "track_01", "Audio 294", "Untitled": a generic word, usually with a number.
    if plain && GENERIC_TITLES.contains(&letters.as_str()) && (digits > 0 || matches!(letters.as_str(), "untitled" | "unknown" | "noname")) {
        return true;
    }
    // "01", "007": a bare number of two or three digits is a track number, not a title.
    if letters.is_empty() && plain && (2..=3).contains(&digits) && lower.chars().filter(|c| c.is_alphanumeric()).count() == digits {
        return true;
    }
    // "01 - Song", "04. Song", "12) Song", "01_Song": a track number, a separator, then words.
    let lead = t.chars().take_while(char::is_ascii_digit).count();
    if (1..=3).contains(&lead) {
        let rest = &t[lead..];
        let after = [". ", ") ", " - ", " – ", "_"].iter().find_map(|sep| rest.strip_prefix(sep));
        if after.is_some_and(|a| a.trim_start().starts_with(char::is_alphabetic)) {
            return true;
        }
    }
    false
}

/// What a song's tags leave out, as [`flag`] bits. The artist counts as missing only when the album
/// artist says nothing either.
pub fn flags_of(title: &str, artist: &str, album_artist: &str, has_cover: bool) -> u8 {
    let mut flags = 0;
    if unknown_artist(artist) && unknown_artist(album_artist) {
        flags |= flag::NO_ARTIST;
    }
    if looks_like_filename(title) {
        flags |= flag::BAD_TITLE;
    }
    if !has_cover {
        flags |= flag::NO_COVER;
    }
    flags
}

pub fn read(path: &Path) -> Result<Meta, String> {
    let mut fmt = open_format(path)?;
    let duration = {
        let track = fmt.default_track(TrackType::Audio).ok_or("no audio track")?;
        let audio = track.codec_params.as_ref().and_then(|p| p.audio());
        // A video's soundtrack can be in a codec nothing here decodes (Dolby Digital, DTS): leave
        // such a file out rather than list a song that never plays.
        if !audio.is_some_and(can_decode) {
            return Err("unsupported audio format".into());
        }
        track_duration_ms(&*fmt, track, audio.and_then(|a| a.sample_rate))
    };
    let mut m = Meta { duration_ms: duration.unwrap_or(0).min(u32::MAX as u64) as u32, ..Default::default() };
    let mut has_cover = false;
    let mut md = fmt.metadata();
    loop {
        if let Some(rev) = md.current() {
            has_cover |= absorb(&mut m, rev);
        }
        if md.pop().is_none() {
            break;
        }
    }
    m.flags = flags_of(&m.title, &m.artist, &m.album_artist, has_cover);
    if m.title.is_empty() {
        m.title = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    }
    if unknown_artist(&m.artist) {
        m.artist = if unknown_artist(&m.album_artist) { "Unknown Artist".into() } else { m.album_artist.clone() };
    }
    if m.album_artist.is_empty() {
        m.album_artist = m.artist.clone();
    }
    if m.album.is_empty() {
        m.album = "Unknown Album".into();
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn years_come_from_dates() {
        assert_eq!(year_of("1999"), 1999);
        assert_eq!(year_of("2004-11-23"), 2004);
        assert_eq!(year_of(" 1987/05 "), 1987);
        assert_eq!(year_of("99"), 0);
        assert_eq!(year_of("unknown"), 0);
    }

    #[test]
    fn unknown_artists_are_recognized() {
        for a in ["", "  ", "Unknown Artist", "unknown", "<Unknown>", "[unknown]", "N/A"] {
            assert!(unknown_artist(a), "{a:?}");
        }
        for a in ["Radiohead", "Various Artists", "Unknown Mortal Orchestra", "Ye"] {
            assert!(!unknown_artist(a), "{a:?}");
        }
    }

    #[test]
    fn raw_file_names_are_told_from_real_titles() {
        for t in [
            "",
            "track_01",
            "Track 05",
            "AUDIO_294",
            "audio294",
            "Untitled",
            "Untitled 3",
            "unknown",
            "New Recording 12",
            "01",
            "007",
            "01_Intro",
            "01 - Flashing Lights",
            "04. Around the World",
            "12) Intro",
            "Song.mp3",
            "Daft_Punk_-_Around_the_World",
            "Around the World (Official Audio)",
            "Creep [320kbps]",
            "Halo (Official Music Video)",
        ] {
            assert!(looks_like_filename(t), "{t:?} should look like a file name");
        }
        for t in [
            "Flashing Lights",
            "7 Rings",
            "99 Problems",
            "21 Guns",
            "1999",
            "1-800-273-8255",
            "Track Star",
            "Song 2",
            "3-Way",
            "7",
            "Audio Slave",
            "Untitled (for Elise)",
            "Creep (Remastered 2009)",
            "Song (feat. Someone)",
            "Around the World - Radio Edit",
            "Bohemian Rhapsody",
        ] {
            assert!(!looks_like_filename(t), "{t:?} is a real title");
        }
    }

    /// A WAV has no tags: everything is flagged, and the track is listed under its file name.
    #[test]
    fn a_file_without_tags_is_flagged_and_not_guessed_at() {
        let dir = std::env::temp_dir().join(format!("wisp-tags-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("03 Some Song.wav");
        let pcm = vec![0u8; 48_000 * 4]; // 1 s of 16-bit stereo silence at 48 kHz
        let mut f = Vec::new();
        f.extend(b"RIFF");
        f.extend((36 + pcm.len() as u32).to_le_bytes());
        f.extend(b"WAVEfmt ");
        f.extend(16u32.to_le_bytes());
        f.extend(1u16.to_le_bytes());
        f.extend(2u16.to_le_bytes());
        f.extend(48_000u32.to_le_bytes());
        f.extend((48_000u32 * 4).to_le_bytes());
        f.extend(4u16.to_le_bytes());
        f.extend(16u16.to_le_bytes());
        f.extend(b"data");
        f.extend((pcm.len() as u32).to_le_bytes());
        f.extend(&pcm);
        std::fs::write(&path, f).unwrap();
        let m = read(&path).unwrap();
        assert_eq!(m.title, "03 Some Song", "the file name, as it is");
        assert_eq!((m.artist.as_str(), m.album.as_str(), m.album_artist.as_str()), ("Unknown Artist", "Unknown Album", "Unknown Artist"));
        assert_eq!(m.flags, flag::NO_ARTIST | flag::BAD_TITLE | flag::NO_COVER);
        assert_eq!(m.duration_ms, 1000);
        assert!(read(&dir.join("nope.wav")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A video file is listed by its soundtrack, with the file name for a title and everything flagged.
    #[test]
    fn a_video_files_audio_is_listed_under_its_file_name() {
        let dir = std::env::temp_dir().join(format!("wisp-tags-video-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Test Artist - Sine Video (Official Video).mp4");
        std::fs::write(&path, include_bytes!("../testdata/sine-262hz-with-video.mp4")).unwrap();
        let m = read(&path).unwrap();
        assert_eq!((m.artist.as_str(), m.title.as_str()), ("Unknown Artist", "Test Artist - Sine Video (Official Video)"));
        assert_eq!(m.flags, flag::NO_ARTIST | flag::BAD_TITLE | flag::NO_COVER);
        assert!((900..1200).contains(&m.duration_ms), "{} ms", m.duration_ms);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A Matroska file's own tags are read. Its duration is the whole file's (the container gives
    /// no other), and a recording that does not state one is listed with none.
    #[test]
    fn matroska_and_webm_files_are_listed_by_their_tags_and_the_files_duration() {
        let dir = std::env::temp_dir().join(format!("wisp-tags-mkv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, bytes: &[u8]| {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };

        let m = read(&write("unrelated name.mkv", include_bytes!("../testdata/sine-262hz-vorbis-video.mkv"))).unwrap();
        assert_eq!((m.artist.as_str(), m.title.as_str()), ("Fixture Artist", "Fixture Song"));
        assert_eq!(m.flags, flag::NO_COVER, "tagged, but no picture");
        assert!((900..1200).contains(&m.duration_ms), "{} ms", m.duration_ms);

        let m = read(&write("Some Band - Some Song.webm", include_bytes!("../testdata/sine-262hz-opus.webm"))).unwrap();
        assert_eq!((m.artist.as_str(), m.title.as_str()), ("Unknown Artist", "Some Band - Some Song"));
        assert!(m.flags & flag::NO_ARTIST != 0);
        assert!((900..1200).contains(&m.duration_ms), "{} ms", m.duration_ms);

        let m = read(&write("recording.webm", include_bytes!("../testdata/sine-262hz-opus-recorder.webm"))).unwrap();
        assert_eq!(m.duration_ms, 0, "a recording does not say how long it is");
        let _ = std::fs::remove_dir_all(dir);
    }
}
