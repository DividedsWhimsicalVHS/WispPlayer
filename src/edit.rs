//! Saving the track editor's result: what changes in the file and what in the library.
//!
//! An MP3's tags are written into the file itself (`lookup::id3`), the only place Wisp changes a
//! music file. Every other format keeps what the user entered in the library index (the title,
//! artist, album, year and track number in the track's record, the lyrics in a table of their own)
//! and a chosen cover as a file in Wisp's data folder, because Wisp does not rewrite those
//! containers. Either way the track's flags are worked out again from what it now has, so a song
//! leaves the unidentified list exactly when nothing is left to fix.

use std::path::{Path, PathBuf};

use crate::library::{self, Library, Track};
use crate::lookup::id3;

/// What the editor's form holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fields {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    /// 0 when blank.
    pub year: u16,
    /// 0 when blank.
    pub track_no: u16,
}

pub fn is_mp3(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp3"))
}

/// What Save does for this file, in words for the editor.
pub fn describe_target(path: &Path) -> String {
    if is_mp3(path) {
        "Save writes these tags into the MP3 file. The music itself is not touched.".to_string()
    } else {
        let kind = path.extension().map_or_else(|| "These".to_string(), |e| format!("{} files", e.to_string_lossy().to_uppercase()));
        format!("{kind} can't be tagged by Wisp, so Save keeps these details in Wisp's library. The file itself is not changed.")
    }
}

fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xFF, 0xD8, ..] => Some("jpg"),
        [0x89, b'P', b'N', b'G', ..] => Some("png"),
        _ => None,
    }
}

/// Keeps a chosen cover as `<dir>/<id>.jpg|png`; returns its path.
fn store_cover(dir: &Path, id: u32, bytes: &[u8]) -> Result<String, String> {
    let ext = image_extension(bytes).ok_or("that is not a JPEG or PNG image")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("couldn't create the covers folder: {e}"))?;
    let file = dir.join(format!("{id}.{ext}"));
    let tmp = dir.join(format!("{id}.{ext}.part"));
    std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, &file)).map_err(|e| format!("couldn't save the cover: {e}"))?;
    for other in ["jpg", "png"].into_iter().filter(|o| *o != ext) {
        let _ = std::fs::remove_file(dir.join(format!("{id}.{other}")));
    }
    Ok(file.to_string_lossy().into_owned())
}

/// Applies the editor's result to song `id`. `cover` and `lyrics` are `None` to leave what the song
/// has. Returns the song as it is now stored.
pub fn save(lib: &Library, covers_dir: &Path, id: u32, f: &Fields, cover: Option<&[u8]>, lyrics: Option<&str>) -> Result<Track, String> {
    // The record as it is now, not as it was when the editor opened: `save_edit` stores only if
    // nothing replaced it in between.
    let current = lib.track(id).ok_or("This song is no longer in the library.")?;
    let path = PathBuf::from(&current.path);
    let mut new = current.clone();
    let mut has_cover = current.flags & library::flag::NO_COVER == 0;
    let mut stored_lyrics = None;

    if is_mp3(&path) {
        let edit = id3::Edit {
            title: &f.title,
            artist: &f.artist,
            album: &f.album,
            album_artist: &f.album_artist,
            year: f.year,
            track_no: f.track_no,
            cover,
            lyrics,
        };
        id3::write(&path, &edit)?;
        let md = std::fs::metadata(&path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
        // The file is ours now: remembering its new size and time keeps a rescan from re-reading it.
        (new.mtime, new.size) = library::stamp(&md);
        has_cover |= cover.is_some();
    } else {
        if let Some(bytes) = cover {
            new.cover = store_cover(covers_dir, id, bytes)?;
        }
        has_cover |= !new.cover.is_empty();
        stored_lyrics = lyrics;
    }

    // The same fall-backs the scanner applies to a file with no such tag.
    new.title = f.title.clone();
    new.artist = match (library::unknown_artist(&f.artist), library::unknown_artist(&f.album_artist)) {
        (false, _) => f.artist.clone(),
        (true, false) => f.album_artist.clone(),
        (true, true) => "Unknown Artist".to_string(),
    };
    new.album = if f.album.is_empty() { "Unknown Album".to_string() } else { f.album.clone() };
    new.album_artist = if f.album_artist.is_empty() { new.artist.clone() } else { f.album_artist.clone() };
    new.year = f.year;
    new.track_no = f.track_no;
    new.flags = library::flags_of(&f.title, &f.artist, &f.album_artist, has_cover);

    if !lib.save_edit(&current, new.clone(), stored_lyrics)? {
        return Err("The library changed while saving. Rescan it in Settings, then open the song again.".to_string());
    }
    lib.rebuild_albums()?;
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{Sort, Upsert, flag};

    const JPEG: [u8; 22] = [0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0xFF, 0xD9];

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wisp-edit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Valid silent MPEG-1 Layer III audio: 40 frames, about a second.
    fn mp3_bytes() -> Vec<u8> {
        let mut out = Vec::new();
        for _ in 0..40 {
            out.extend([0xFF, 0xFB, 0x90, 0x00]);
            out.extend(std::iter::repeat_n(0u8, 413));
        }
        out
    }

    fn library_with(dir: &Path, path: &Path) -> (Library, u32) {
        let lib = Library::open(&dir.join("lib.redb")).unwrap();
        let (mtime, size) = library::stamp(&std::fs::metadata(path).unwrap_or_else(|_| std::fs::metadata(dir).unwrap()));
        let track = Track {
            path: path.to_string_lossy().into_owned(),
            title: "track_01".into(),
            artist: "Unknown Artist".into(),
            album: "Unknown Album".into(),
            album_artist: "Unknown Artist".into(),
            flags: flag::NO_ARTIST | flag::BAD_TITLE | flag::NO_COVER,
            mtime,
            size,
            duration_ms: 1000,
            ..Default::default()
        };
        let id = lib.apply(vec![Upsert { id: None, track }], true).unwrap()[0];
        lib.rebuild_albums().unwrap();
        (lib, id)
    }

    fn fields() -> Fields {
        Fields {
            title: "Flashing Lights".into(),
            artist: "Kanye West feat. Dwele".into(),
            album: "Graduation".into(),
            album_artist: "Kanye West".into(),
            year: 2007,
            track_no: 9,
        }
    }

    #[test]
    fn an_mp3_gets_its_tags_in_the_file_and_leaves_the_flagged_list() {
        let d = dir("mp3");
        let path = d.join("track_01.mp3");
        std::fs::write(&path, mp3_bytes()).unwrap();
        let (lib, id) = library_with(&d, &path);
        assert_eq!(lib.flagged_ids(), [id]);

        let saved = save(&lib, &d.join("Covers"), id, &fields(), Some(&JPEG), Some("[00:01.00] Words")).unwrap();
        assert_eq!(saved.flags, 0);
        assert_eq!(lib.flagged_count(), 0, "nothing is left to fix");

        let stored = lib.track(id).unwrap();
        assert_eq!((stored.title.as_str(), stored.artist.as_str(), stored.album.as_str()), ("Flashing Lights", "Kanye West feat. Dwele", "Graduation"));
        assert_eq!((stored.year, stored.track_no), (2007, 9));
        // The file says the same, and a rescan would see it as unchanged.
        let read = library::read_tags(&path).unwrap();
        assert_eq!((read.title.as_str(), read.artist.as_str(), read.flags), ("Flashing Lights", "Kanye West feat. Dwele", 0));
        assert_eq!(library::stamp(&std::fs::metadata(&path).unwrap()), (stored.mtime, stored.size));
        assert_eq!(lib.known(&[stored.path.clone()])[0], Some((id, stored.mtime, stored.size)));
        assert!(crate::art::read_embedded(&path).cover.is_some());
        assert_eq!(lib.lyrics(id), None, "an MP3 keeps its lyrics in the file");
        assert_eq!(crate::art::read_embedded(&path).lyrics.as_deref(), Some("[00:01.00] Words"));
        // The album list follows.
        assert_eq!(lib.album_count(), 1);
        assert_eq!(lib.album(0).unwrap().name, "Graduation");
        assert_eq!(lib.ordered_ids(Sort::Title), [id]);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn leaving_a_field_blank_keeps_the_song_flagged_for_it() {
        let d = dir("partial");
        let path = d.join("track_01.mp3");
        std::fs::write(&path, mp3_bytes()).unwrap();
        let (lib, id) = library_with(&d, &path);
        let only_title = Fields { title: "Flashing Lights".into(), ..Default::default() };
        let saved = save(&lib, &d.join("Covers"), id, &only_title, None, None).unwrap();
        assert_eq!(saved.flags, flag::NO_ARTIST | flag::NO_COVER);
        assert_eq!((saved.artist.as_str(), saved.album.as_str()), ("Unknown Artist", "Unknown Album"));
        assert_eq!(lib.flagged_ids(), [id]);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn other_formats_keep_the_details_in_the_library_and_the_file_untouched() {
        let d = dir("flac");
        let path = d.join("take 3.flac");
        std::fs::write(&path, b"not really audio").unwrap();
        let (lib, id) = library_with(&d, &path);
        let covers = d.join("Covers");

        let saved = save(&lib, &covers, id, &fields(), Some(&JPEG), Some("plain words")).unwrap();
        assert_eq!(saved.flags, 0);
        assert_eq!(std::fs::read(&path).unwrap(), b"not really audio", "the file is not changed");
        let stored = lib.track(id).unwrap();
        assert_eq!((stored.title.as_str(), stored.album.as_str(), stored.year), ("Flashing Lights", "Graduation", 2007));
        assert_eq!(std::fs::read(&stored.cover).unwrap(), JPEG);
        assert!(stored.cover.ends_with(&format!("{id}.jpg")));
        assert_eq!(lib.lyrics(id).as_deref(), Some("plain words"));

        // Saving again without a cover or lyrics keeps both; a PNG replaces the JPEG.
        let again = save(&lib, &covers, id, &Fields { title: "Renamed".into(), ..fields() }, None, None).unwrap();
        assert_eq!((again.flags, lib.track(id).unwrap().cover.as_str()), (0, stored.cover.as_str()));
        assert_eq!(lib.lyrics(id).as_deref(), Some("plain words"));
        let png = [0x89, b'P', b'N', b'G', 13, 10, 26, 10];
        let swapped = save(&lib, &covers, id, &fields(), Some(&png), None).unwrap();
        assert!(swapped.cover.ends_with(&format!("{id}.png")));
        assert!(!covers.join(format!("{id}.jpg")).exists(), "the old cover file is removed");
        assert!(save(&lib, &covers, id, &fields(), Some(b"GIF89a"), None).is_err(), "only JPEG and PNG covers are accepted");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_missing_song_and_a_missing_file_are_reported() {
        let d = dir("missing");
        let path = d.join("gone.mp3");
        let (lib, id) = library_with(&d, &path);
        assert!(save(&lib, &d, id + 7, &fields(), None, None).unwrap_err().contains("no longer"));
        let err = save(&lib, &d, id, &fields(), None, None).unwrap_err();
        assert!(err.contains("gone.mp3"), "{err}");
        assert_eq!(lib.track(id).unwrap().title, "track_01", "a failed save changes nothing");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn the_footer_says_what_save_does() {
        assert!(describe_target(Path::new("a.MP3")).contains("into the MP3 file"));
        assert!(describe_target(Path::new("a.flac")).starts_with("FLAC files can't be tagged"));
        assert!(describe_target(Path::new("a.mp4")).contains("not changed"));
    }
}
