//! Writing ID3v2 tags into MP3 files: the one place Wisp changes a music file, and only when the
//! user presses Save in the track editor.
//!
//! The edit is a merge. The title, artist, album, album artist, year, track number, front cover and
//! lyrics are replaced; every other frame the file has (ReplayGain, genre, comments, ...) is kept
//! byte for byte, in the version the file already uses (2.3 stays 2.3; 2.4, no tag, and the rare
//! 2.2 get 2.4). The audio is never re-encoded: the new file is the new tag followed by the old
//! file from its first audio byte, so an ID3v1 trailer and everything else after the audio stay.
//! It is written beside the original and renamed into place, so an interrupted save leaves the
//! original untouched.

use std::fs::File;
#[cfg(test)]
use std::io::Cursor;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// What to write. An empty text (or a zero year or track number) removes that frame; `None` for
/// the cover or the lyrics leaves what the file has.
pub struct Edit<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    pub album_artist: &'a str,
    pub year: u16,
    pub track_no: u16,
    pub cover: Option<&'a [u8]>,
    pub lyrics: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Version {
    V3,
    V4,
}

struct Frame {
    id: [u8; 4],
    flags: [u8; 2],
    data: Vec<u8>,
}

struct Tag {
    version: Version,
    frames: Vec<Frame>,
}

/// Padding after the frames, so a later edit has room.
const PADDING: usize = 256;

fn syncsafe(n: usize) -> [u8; 4] {
    [(n >> 21) as u8 & 0x7f, (n >> 14) as u8 & 0x7f, (n >> 7) as u8 & 0x7f, n as u8 & 0x7f]
}

fn from_syncsafe(b: &[u8]) -> usize {
    ((b[0] as usize & 0x7f) << 21) | ((b[1] as usize & 0x7f) << 14) | ((b[2] as usize & 0x7f) << 7) | (b[3] as usize & 0x7f)
}

/// Undoes unsynchronisation: `FF 00` becomes `FF`.
fn deunsync(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        out.push(data[i]);
        if data[i] == 0xFF && data.get(i + 1) == Some(&0) {
            i += 1;
        }
        i += 1;
    }
    out
}

fn valid_id(id: &[u8]) -> bool {
    id.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Reads the frames of a tag body. `unsync` is the tag-level flag.
fn parse_frames(version: Version, body: &[u8], unsync: bool) -> Vec<Frame> {
    let mut frames = Vec::new();
    let mut pos = 0;
    while pos + 10 <= body.len() {
        let id: [u8; 4] = body[pos..pos + 4].try_into().expect("four bytes");
        if id[0] == 0 || !valid_id(&id) {
            break; // padding, or something that is not a frame
        }
        let size = match version {
            Version::V3 => u32::from_be_bytes(body[pos + 4..pos + 8].try_into().expect("four bytes")) as usize,
            Version::V4 => from_syncsafe(&body[pos + 4..pos + 8]),
        };
        let mut flags = [body[pos + 8], body[pos + 9]];
        let Some(data) = body.get(pos + 10..pos + 10 + size) else { break };
        // A tag-level unsynchronisation of 2.4 is carried over as each frame's own flag.
        if unsync && version == Version::V4 {
            flags[1] |= 0x02;
        }
        frames.push(Frame { id, flags, data: data.to_vec() });
        pos += 10 + size;
    }
    frames
}

/// Reads the ID3v2 tags at the start of a file. The first tag's frames are kept; any further
/// stacked tags are dropped. Returns them and the offset of the first byte after the tags.
fn read_tags<R: Read + Seek>(r: &mut R) -> std::io::Result<(Option<Tag>, u64)> {
    let mut first: Option<Tag> = None;
    let mut offset = 0u64;
    loop {
        r.seek(SeekFrom::Start(offset))?;
        let mut head = [0u8; 10];
        if r.read_exact(&mut head).is_err() || &head[..3] != b"ID3" || head[3] == 0xFF || head[6..10].iter().any(|b| b & 0x80 != 0) {
            return Ok((first, offset));
        }
        let (major, flags) = (head[3], head[5]);
        let size = from_syncsafe(&head[6..10]);
        if size > 64 << 20 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "the ID3 tag is larger than 64 MB"));
        }
        let mut body = vec![0u8; size];
        r.read_exact(&mut body)?;
        let footer = if major == 4 && flags & 0x10 != 0 { 10 } else { 0 };
        offset += 10 + size as u64 + footer;
        if first.is_some() {
            continue;
        }
        first = Some(match major {
            3 | 4 => {
                let version = if major == 3 { Version::V3 } else { Version::V4 };
                let unsync = flags & 0x80 != 0;
                let mut body = if unsync && version == Version::V3 { deunsync(&body) } else { body };
                if flags & 0x40 != 0 {
                    // An extended header: not carried over.
                    let skip = match version {
                        Version::V3 => body.get(..4).map(|b| 4 + u32::from_be_bytes(b.try_into().expect("four bytes")) as usize),
                        Version::V4 => body.get(..4).map(from_syncsafe),
                    };
                    body.drain(..skip.unwrap_or(0).min(body.len()));
                }
                Tag { version, frames: parse_frames(version, &body, unsync) }
            }
            // 2.2 has 3-letter frames, and later versions are unknown: nothing to carry over.
            _ => Tag { version: Version::V4, frames: Vec::new() },
        });
    }
}

// -- building frames ------------------------------------------------------------------------

fn text_payload(version: Version, text: &str) -> Vec<u8> {
    match version {
        Version::V4 => [&[3u8][..], text.as_bytes()].concat(),
        Version::V3 => {
            let mut p = vec![1u8, 0xFF, 0xFE];
            p.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            p
        }
    }
}

fn text_frame(version: Version, id: &[u8; 4], text: &str) -> Frame {
    Frame { id: *id, flags: [0, 0], data: text_payload(version, text) }
}

/// "eng", an empty description, then the text: unsynchronised lyrics.
fn lyrics_frame(version: Version, text: &str) -> Frame {
    let text = text.replace("\r\n", "\n");
    let mut data = match version {
        Version::V4 => vec![3u8, b'e', b'n', b'g', 0],
        Version::V3 => vec![1u8, b'e', b'n', b'g', 0xFF, 0xFE, 0, 0],
    };
    match version {
        Version::V4 => data.extend(text.as_bytes()),
        Version::V3 => {
            data.extend([0xFF, 0xFE]);
            data.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        }
    }
    Frame { id: *b"USLT", flags: [0, 0], data }
}

fn cover_frame(image: &[u8]) -> Frame {
    let mime: &[u8] = if image.starts_with(b"\x89PNG") { b"image/png" } else { b"image/jpeg" };
    let mut data = vec![0u8]; // ISO-8859-1 for the (empty) description
    data.extend(mime);
    data.extend([0, 3, 0]); // end of the MIME type, "front cover", empty description
    data.extend(image);
    Frame { id: *b"APIC", flags: [0, 0], data }
}

/// The picture type of an `APIC` frame, if it can be read.
fn picture_type(data: &[u8]) -> Option<u8> {
    let mime_end = data.get(1..)?.iter().position(|b| *b == 0)? + 1;
    data.get(mime_end + 1).copied()
}

fn encode_frames(version: Version, frames: &[Frame]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for f in frames {
        if version == Version::V4 && f.data.len() > 0x0FFF_FFFF {
            return Err("a picture or lyrics text is too large for an ID3 tag".into());
        }
        out.extend(f.id);
        match version {
            Version::V3 => out.extend((f.data.len() as u32).to_be_bytes()),
            Version::V4 => out.extend(syncsafe(f.data.len())),
        }
        out.extend(f.flags);
        out.extend(&f.data);
    }
    Ok(out)
}

/// The tag that results from applying `edit` to `old`.
fn apply(old: Option<Tag>, edit: &Edit) -> Result<Vec<u8>, String> {
    let (version, kept) = match old {
        Some(t) => (t.version, t.frames),
        None => (Version::V4, Vec::new()),
    };
    let replaced_text: [&[u8; 4]; 8] = [b"TIT2", b"TPE1", b"TALB", b"TPE2", b"TRCK", b"TYER", b"TDRC", b"TDAT"];
    let mut frames: Vec<Frame> = Vec::new();
    for (id, value) in [(b"TIT2", edit.title), (b"TPE1", edit.artist), (b"TALB", edit.album), (b"TPE2", edit.album_artist)] {
        if !value.trim().is_empty() {
            frames.push(text_frame(version, id, value.trim()));
        }
    }
    if edit.track_no > 0 {
        frames.push(text_frame(version, b"TRCK", &edit.track_no.to_string()));
    }
    if edit.year > 0 {
        frames.push(text_frame(version, if version == Version::V4 { b"TDRC" } else { b"TYER" }, &edit.year.to_string()));
    }
    if let Some(text) = edit.lyrics.filter(|t| !t.trim().is_empty()) {
        frames.push(lyrics_frame(version, text));
    }
    if let Some(image) = edit.cover {
        frames.push(cover_frame(image));
    }
    for f in kept {
        let id = &f.id;
        let dropped = replaced_text.contains(&id)
            || (id == b"TIME" && edit.year > 0)
            || (id == b"USLT" && edit.lyrics.is_some())
            || (id == b"APIC" && edit.cover.is_some() && picture_type(&f.data) == Some(3));
        if !dropped {
            frames.push(f);
        }
    }
    let body = encode_frames(version, &frames)?;
    let size = body.len() + PADDING;
    if size > 0x0FFF_FFFF {
        return Err("the tag would be larger than ID3 allows".into());
    }
    let mut out = Vec::with_capacity(10 + size);
    out.extend(b"ID3");
    out.extend([if version == Version::V4 { 4 } else { 3 }, 0, 0]);
    out.extend(syncsafe(size));
    out.extend(body);
    out.extend(std::iter::repeat_n(0u8, PADDING));
    Ok(out)
}

/// `file` (a whole MP3) with `edit` applied to its tag.
#[cfg(test)]
fn retag(file: &[u8], edit: &Edit) -> Result<Vec<u8>, String> {
    let mut cursor = Cursor::new(file);
    let (old, audio_start) = read_tags(&mut cursor).map_err(|e| e.to_string())?;
    let mut out = apply(old, edit)?;
    out.extend(&file[audio_start as usize..]);
    Ok(out)
}

/// Applies `edit` to the tag of the MP3 at `path`, in place and atomically.
pub fn write(path: &Path, edit: &Edit) -> Result<(), String> {
    let name = path.display();
    let mut src = File::open(path).map_err(|e| format!("couldn't open {name}: {e}"))?;
    let (old, audio_start) = read_tags(&mut src).map_err(|e| format!("couldn't read the tag of {name}: {e}"))?;
    let tag = apply(old, edit)?;
    let tmp = path.with_extension("wisp-part");
    let _ = std::fs::remove_file(&tmp);
    let copied = (|| -> std::io::Result<()> {
        let mut out = File::create_new(&tmp)?;
        out.write_all(&tag)?;
        src.seek(SeekFrom::Start(audio_start))?;
        std::io::copy(&mut src, &mut out)?;
        out.sync_all()
    })();
    drop(src);
    match copied.and_then(|()| std::fs::rename(&tmp, path)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(format!("couldn't write {name}: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::flag;

    const FRAME_LEN: usize = 417; // MPEG-1 Layer III, 128 kbps, 44.1 kHz

    /// Valid silent MP3 audio of `frames` frames.
    fn audio(frames: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for _ in 0..frames {
            out.extend([0xFF, 0xFB, 0x90, 0x00]);
            out.extend(std::iter::repeat_n(0u8, FRAME_LEN - 4));
        }
        out
    }

    fn edit() -> Edit<'static> {
        Edit {
            title: "Flashing Lights",
            artist: "Kanye West feat. Dwele",
            album: "Graduation",
            album_artist: "Kanye West",
            year: 2007,
            track_no: 9,
            cover: None,
            lyrics: None,
        }
    }

    /// A v2.3 or v2.4 tag around `frames` (id, data), with some padding.
    fn tag(version: Version, frames: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let all: Vec<Frame> = frames.iter().map(|(id, data)| Frame { id: **id, flags: [0, 0], data: data.clone() }).collect();
        let mut body = encode_frames(version, &all).unwrap();
        body.extend([0u8; 32]);
        let mut out = b"ID3".to_vec();
        out.extend([if version == Version::V4 { 4 } else { 3 }, 0, 0]);
        out.extend(syncsafe(body.len()));
        out.extend(body);
        out
    }

    fn frames_of(file: &[u8]) -> (Version, Vec<Frame>, usize) {
        let (tag, start) = read_tags(&mut Cursor::new(file)).unwrap();
        let tag = tag.expect("a tag");
        (tag.version, tag.frames, start as usize)
    }

    fn find<'a>(frames: &'a [Frame], id: &[u8; 4]) -> Option<&'a Frame> {
        frames.iter().find(|f| &f.id == id)
    }

    /// The text of a frame written by this module (UTF-8 in 2.4, UTF-16 with a BOM in 2.3).
    fn text_of(f: &Frame) -> String {
        match f.data[0] {
            3 => String::from_utf8(f.data[1..].to_vec()).unwrap(),
            1 => String::from_utf16(&f.data[3..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()).unwrap(),
            e => panic!("encoding {e}"),
        }
    }

    #[test]
    fn a_file_without_a_tag_gets_a_2_4_tag_and_the_audio_is_untouched() {
        let file = audio(5);
        let out = retag(&file, &edit()).unwrap();
        let (version, frames, start) = frames_of(&out);
        assert_eq!(version, Version::V4);
        assert_eq!(&out[start..], &file[..], "the audio is the same bytes");
        let get = |id: &[u8; 4]| text_of(find(&frames, id).unwrap());
        assert_eq!((get(b"TIT2").as_str(), get(b"TPE1").as_str(), get(b"TALB").as_str()), ("Flashing Lights", "Kanye West feat. Dwele", "Graduation"));
        assert_eq!((get(b"TPE2").as_str(), get(b"TDRC").as_str(), get(b"TRCK").as_str()), ("Kanye West", "2007", "9"));
        assert!(find(&frames, b"APIC").is_none() && find(&frames, b"USLT").is_none());
    }

    #[test]
    fn other_frames_are_kept_byte_for_byte_and_the_version_stays() {
        let old = tag(
            Version::V3,
            &[
                (b"TIT2", [&[0u8][..], b"old title"].concat()),
                (b"TXXX", [&[0u8][..], b"replaygain_track_gain\0-7.5 dB"].concat()),
                (b"TCON", [&[0u8][..], b"Hip-Hop"].concat()),
                (b"COMM", [&[0u8][..], b"eng\0\0a comment"].concat()),
                (b"TYER", [&[0u8][..], b"1999"].concat()),
                (b"TDAT", [&[0u8][..], b"0105"].concat()),
            ],
        );
        let file = [old, audio(3)].concat();
        let out = retag(&file, &edit()).unwrap();
        let (version, frames, start) = frames_of(&out);
        assert_eq!(version, Version::V3, "a 2.3 tag stays 2.3, so the frames it has stay valid");
        assert_eq!(text_of(find(&frames, b"TIT2").unwrap()), "Flashing Lights");
        assert_eq!(text_of(find(&frames, b"TYER").unwrap()), "2007");
        assert!(find(&frames, b"TDAT").is_none(), "the old day and month no longer match the year");
        assert_eq!(frames.iter().filter(|f| &f.id == b"TIT2").count(), 1);
        for (id, data) in [(b"TXXX", [&[0u8][..], b"replaygain_track_gain\0-7.5 dB"].concat()), (b"TCON", [&[0u8][..], b"Hip-Hop"].concat()), (b"COMM", [&[0u8][..], b"eng\0\0a comment"].concat())] {
            assert_eq!(find(&frames, id).unwrap().data, data, "{}", String::from_utf8_lossy(id));
        }
        assert_eq!(&out[start..], &audio(3)[..]);
    }

    #[test]
    fn empty_fields_remove_their_frames() {
        let old = tag(Version::V4, &[(b"TALB", [&[3u8][..], b"Old Album"].concat()), (b"TDRC", [&[3u8][..], b"1999"].concat()), (b"TRCK", [&[3u8][..], b"4/12"].concat())]);
        let e = Edit { album: "", year: 0, track_no: 0, ..edit() };
        let (_, frames, _) = frames_of(&retag(&[old, audio(1)].concat(), &e).unwrap());
        assert!(find(&frames, b"TALB").is_none() && find(&frames, b"TDRC").is_none() && find(&frames, b"TRCK").is_none());
        assert!(find(&frames, b"TIT2").is_some());
    }

    #[test]
    fn the_front_cover_is_replaced_other_pictures_stay_and_lyrics_follow_the_edit() {
        let picture = |kind: u8, bytes: &[u8]| [&[0u8][..], b"image/jpeg\0", &[kind, 0], bytes].concat();
        let old = tag(
            Version::V4,
            &[(b"APIC", picture(3, b"old front")), (b"APIC", picture(4, b"the back")), (b"USLT", [&[3u8][..], b"eng\0\0old words"].concat())],
        );
        let file = [old, audio(2)].concat();
        // No cover and no lyrics in the edit: what the file has stays.
        let (_, frames, _) = frames_of(&retag(&file, &edit()).unwrap());
        assert_eq!(frames.iter().filter(|f| &f.id == b"APIC").count(), 2);
        assert!(find(&frames, b"USLT").is_some());
        // A new cover replaces the front one only; new lyrics replace the old.
        let png = [b"\x89PNG\r\n\x1a\n".as_slice(), b"data"].concat();
        let e = Edit { cover: Some(&png), lyrics: Some("[00:01.00] New words\r\nsecond"), ..edit() };
        let (_, frames, _) = frames_of(&retag(&file, &e).unwrap());
        let pictures: Vec<&Frame> = frames.iter().filter(|f| &f.id == b"APIC").collect();
        assert_eq!(pictures.len(), 2);
        assert!(pictures.iter().any(|f| picture_type(&f.data) == Some(3) && f.data.windows(9).any(|w| w == b"image/png")));
        assert!(pictures.iter().any(|f| picture_type(&f.data) == Some(4) && f.data.ends_with(b"the back")));
        let lyrics: Vec<&Frame> = frames.iter().filter(|f| &f.id == b"USLT").collect();
        assert_eq!(lyrics.len(), 1);
        assert!(lyrics[0].data.ends_with(b"[00:01.00] New words\nsecond"), "line ends are plain newlines");
        // Empty lyrics remove them.
        let e = Edit { lyrics: Some(""), ..edit() };
        assert!(find(&frames_of(&retag(&file, &e).unwrap()).1, b"USLT").is_none());
    }

    #[test]
    fn an_unsynchronised_2_3_tag_is_read_correctly_and_old_formats_are_replaced() {
        // A 2.3 tag with the unsynchronisation flag: every FF 00 in it stands for FF.
        let frame = (b"TXXX", [&[0u8][..], b"key\0value\xFF"].concat());
        let mut plain = tag(Version::V3, &[frame.clone()]);
        let body_len = plain.len() - 10;
        let mut synced = plain[10..].to_vec();
        let at = synced.iter().position(|b| *b == 0xFF).unwrap();
        synced.insert(at + 1, 0); // FF becomes FF 00
        plain.truncate(10);
        plain[5] = 0x80;
        plain[6..10].copy_from_slice(&syncsafe(synced.len()));
        let file = [plain, synced, audio(1)].concat();
        assert!(body_len > 0);
        let (version, frames, _) = frames_of(&retag(&file, &edit()).unwrap());
        assert_eq!(version, Version::V3);
        assert_eq!(find(&frames, b"TXXX").unwrap().data, frame.1, "the frame is back to its real bytes");
        // A 2.2 tag has nothing to carry over and becomes 2.4.
        let mut v22 = b"ID3\x02\x00\x00".to_vec();
        v22.extend(syncsafe(16));
        v22.extend([b'T', b'T', b'2', 0, 0, 6, 0, b'o', b'l', b'd', 0, 0, 0, 0, 0, 0]);
        let (version, frames, _) = frames_of(&retag(&[v22, audio(1)].concat(), &edit()).unwrap());
        assert_eq!(version, Version::V4);
        assert_eq!(text_of(find(&frames, b"TIT2").unwrap()), "Flashing Lights");
    }

    #[test]
    fn stacked_tags_collapse_and_an_id3v1_trailer_stays() {
        let first = tag(Version::V4, &[(b"TIT2", [&[3u8][..], b"first"].concat())]);
        let second = tag(Version::V4, &[(b"TPE1", [&[3u8][..], b"second"].concat())]);
        let mut trailer = b"TAG".to_vec();
        trailer.resize(128, b' ');
        let file = [first, second, audio(2), trailer.clone()].concat();
        let out = retag(&file, &edit()).unwrap();
        let (_, frames, start) = frames_of(&out);
        assert_eq!(text_of(find(&frames, b"TIT2").unwrap()), "Flashing Lights");
        assert_eq!(&out[start..], &[audio(2), trailer].concat()[..], "audio and the ID3v1 trailer are as they were");
    }

    /// Through the files and Symphonia: what the editor saves is what the library and the player read.
    #[test]
    fn a_saved_mp3_reads_back_with_its_tags_cover_and_lyrics() {
        let dir = std::env::temp_dir().join(format!("wisp-id3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("track_01.mp3");
        std::fs::write(&path, audio(40)).unwrap();
        let before = crate::library::read_tags(&path).unwrap();
        assert_eq!(before.flags, flag::NO_ARTIST | flag::BAD_TITLE | flag::NO_COVER);

        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0xFF, 0xD9];
        let e = Edit { cover: Some(&jpeg), lyrics: Some("[00:01.00] Words"), ..edit() };
        write(&path, &e).unwrap();
        assert!(!path.with_extension("wisp-part").exists(), "no temporary file is left");

        let after = crate::library::read_tags(&path).unwrap();
        assert_eq!((after.title.as_str(), after.artist.as_str(), after.album.as_str()), ("Flashing Lights", "Kanye West feat. Dwele", "Graduation"));
        assert_eq!((after.album_artist.as_str(), after.year, after.track_no), ("Kanye West", 2007, 9));
        assert_eq!(after.flags, 0, "nothing is left to fix");
        let embedded = crate::art::read_embedded(&path);
        assert_eq!(embedded.cover.as_deref(), Some(&jpeg[..]));
        assert_eq!(embedded.lyrics.as_deref(), Some("[00:01.00] Words"));
        // The audio is still all there.
        assert!(after.duration_ms.abs_diff(before.duration_ms) < 60, "{} ms before, {} ms after", before.duration_ms, after.duration_ms);

        // A second save keeps what the first wrote and changes only what is asked.
        let e = Edit { title: "Renamed", cover: None, lyrics: None, ..edit() };
        write(&path, &e).unwrap();
        let again = crate::library::read_tags(&path).unwrap();
        assert_eq!(again.title, "Renamed");
        assert_eq!(again.flags, 0);
        assert!(crate::art::read_embedded(&path).cover.is_some());
        assert!(write(&dir.join("missing.mp3"), &edit()).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
