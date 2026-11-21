//! The music library: a persistent index of every track on disk, stored in redb.
//!
//! Memory is the design constraint. Nothing but 4-byte track ids is ever held in RAM for a whole
//! view: the index tables keep tracks in sorted order, a view is a `Vec<u32>` of ids read from
//! them, and a row's text is fetched from the database only when the list asks for it. A
//! 100 000-track library costs ~400 KB for the "all songs" id list plus redb's small page cache.
//!
//! ```text
//! tracks    id -> record            by_title  sortkey -> id     (songs A-Z)
//! paths     path -> (id,mtime,size) by_album  sortkey -> id     (artist > album > disc > track)
//! albums    n -> album record       meta      counters / schema
//! flags     id -> reasons           lyrics    id -> text        (lyrics kept in Wisp, not in the file)
//! ```
//!
//! `flags` lists only the tracks whose tags leave something out (see [`flag`]), so the list of
//! unidentified tracks and its count never need the whole library to be read.

pub mod filename;
mod scan;
mod tags;

use std::path::PathBuf;

use redb::{Database, DatabaseError, Durability, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};

pub use scan::{Progress, spawn_scan, stamp};
pub use tags::{flags_of, unknown_artist};
#[cfg(test)]
pub(crate) use tags::read as read_tags;

const TRACKS: TableDefinition<u32, &[u8]> = TableDefinition::new("tracks");
const PATHS: TableDefinition<&str, (u32, u64, u64)> = TableDefinition::new("paths");
const BY_TITLE: TableDefinition<&[u8], u32> = TableDefinition::new("by_title");
const BY_ALBUM: TableDefinition<&[u8], u32> = TableDefinition::new("by_album");
const ALBUMS: TableDefinition<u32, &[u8]> = TableDefinition::new("albums");
const META: TableDefinition<&str, u64> = TableDefinition::new("meta");
const FLAGS: TableDefinition<u32, u8> = TableDefinition::new("flags");
/// Lyrics the user saved for a file that cannot hold them itself.
const LYRICS: TableDefinition<u32, &str> = TableDefinition::new("lyrics");
/// Tables of earlier layouts (online identification), dropped when an old index is opened.
const OLD_IDENT: TableDefinition<u32, u8> = TableDefinition::new("ident");
const OLD_LOOKUPS: TableDefinition<&str, &[u8]> = TableDefinition::new("lookups");

/// Bump when the on-disk layout changes; an old index is simply rebuilt by a rescan.
const SCHEMA: u64 = 3;
/// redb page cache. The index is read in small random reads, so a couple of MB is plenty.
const CACHE_BYTES: usize = 2 << 20;

/// Why a track is flagged as unidentified: what its own tags leave out.
pub mod flag {
    /// No artist, or "Unknown Artist".
    pub const NO_ARTIST: u8 = 1;
    /// The title is missing, or looks like a raw file name (`track_01`, `AUDIO_294`).
    pub const BAD_TITLE: u8 = 2;
    /// No cover picture inside the file.
    pub const NO_COVER: u8 = 4;

    /// "No artist · No real title · No cover"
    pub fn describe(flags: u8) -> String {
        let mut parts = Vec::new();
        if flags & NO_ARTIST != 0 {
            parts.push("No artist");
        }
        if flags & BAD_TITLE != 0 {
            parts.push("No real title");
        }
        if flags & NO_COVER != 0 {
            parts.push("No cover");
        }
        parts.join(" · ")
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub id: u32,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track_no: u16,
    pub disc_no: u16,
    pub year: u16,
    pub duration_ms: u32,
    pub mtime: u64,
    pub size: u64,
    /// What the tags leave out, as [`flag`] bits; 0 when the track is complete.
    pub flags: u8,
    /// A cover the user chose in Wisp for a file that cannot hold one itself, or empty.
    pub cover: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Album {
    pub index: u32,
    pub name: String,
    pub artist: String,
    pub year: u16,
    pub tracks: u32,
    /// A track of this album, used to find its cover.
    pub rep_track: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Artist {
    pub name: String,
    /// Albums are stored sorted by artist, so an artist's albums are one contiguous run.
    pub first_album: u32,
    pub albums: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Title,
    /// Artist, album, disc, track: the natural order for browsing.
    Album,
    /// Newest first.
    Recent,
}

impl Sort {
    pub fn from_index(i: i32) -> Sort {
        match i {
            1 => Sort::Album,
            2 => Sort::Recent,
            _ => Sort::Title,
        }
    }
}

// -- text folding ---------------------------------------------------------------------------

fn base_char(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' | 'đ' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' | 'ħ' => 'h',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' | 'ß' => 's',
        'ţ' | 'ť' | 'ŧ' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        c => c,
    }
}

/// Lower-case with accents removed: what search compares.
pub fn fold(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).map(base_char).collect()
}

/// Sort form of a name: folded, with a leading article dropped ("The Beatles" files under B).
fn sort_name(s: &str) -> String {
    let f = fold(s.trim());
    for article in ["the ", "an ", "a "] {
        if let Some(rest) = f.strip_prefix(article) {
            if !rest.trim().is_empty() {
                return rest.trim_start().to_string();
            }
        }
    }
    f
}

fn key_title(t: &Track) -> Vec<u8> {
    let mut k = sort_name(&t.title).into_bytes();
    k.push(0);
    k.extend(t.id.to_be_bytes());
    k
}

fn album_prefix(artist: &str, album: &str) -> Vec<u8> {
    let mut k = sort_name(artist).into_bytes();
    k.push(0);
    k.extend(sort_name(album).into_bytes());
    k.push(0);
    k
}

fn key_album(t: &Track) -> Vec<u8> {
    let mut k = album_prefix(&t.album_artist, &t.album);
    k.extend(t.disc_no.to_be_bytes());
    k.extend(t.track_no.to_be_bytes());
    k.extend(sort_name(&t.title).into_bytes());
    k.push(0);
    k.extend(t.id.to_be_bytes());
    k
}

/// The part of a `by_album` key that identifies the album: everything up to the second NUL.
fn album_of_key(key: &[u8]) -> &[u8] {
    let mut nuls = 0;
    for (i, b) in key.iter().enumerate() {
        if *b == 0 {
            nuls += 1;
            if nuls == 2 {
                return &key[..=i];
            }
        }
    }
    key
}

// -- record encoding ------------------------------------------------------------------------

fn put_str(buf: &mut Vec<u8>, s: &str) {
    let end = s.floor_char_boundary(s.len().min(u16::MAX as usize));
    buf.extend((end as u16).to_le_bytes());
    buf.extend(&s.as_bytes()[..end]);
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        self.take(n).map(|b| String::from_utf8_lossy(b).into_owned())
    }
}

impl Track {
    fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(64 + self.path.len());
        for s in [&self.path, &self.title, &self.artist, &self.album, &self.album_artist] {
            put_str(&mut b, s);
        }
        b.extend(self.track_no.to_le_bytes());
        b.extend(self.disc_no.to_le_bytes());
        b.extend(self.year.to_le_bytes());
        b.extend(self.duration_ms.to_le_bytes());
        b.extend(self.mtime.to_le_bytes());
        b.extend(self.size.to_le_bytes());
        b.push(self.flags);
        put_str(&mut b, &self.cover);
        b
    }

    fn decode(id: u32, bytes: &[u8]) -> Option<Track> {
        let mut r = Reader(bytes);
        Some(Track {
            id,
            path: r.str()?,
            title: r.str()?,
            artist: r.str()?,
            album: r.str()?,
            album_artist: r.str()?,
            track_no: r.u16()?,
            disc_no: r.u16()?,
            year: r.u16()?,
            duration_ms: r.u32()?,
            mtime: r.u64()?,
            size: r.u64()?,
            flags: r.u8()?,
            cover: r.str()?,
        })
    }
}

fn encode_album(a: &Album, prefix: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    put_str(&mut b, &a.name);
    put_str(&mut b, &a.artist);
    b.extend(a.year.to_le_bytes());
    b.extend(a.tracks.to_le_bytes());
    b.extend(a.rep_track.to_le_bytes());
    b.extend(prefix);
    b
}

fn decode_album(index: u32, bytes: &[u8]) -> Option<(Album, Vec<u8>)> {
    let mut r = Reader(bytes);
    let album = Album {
        index,
        name: r.str()?,
        artist: r.str()?,
        year: r.u16()?,
        tracks: r.u32()?,
        rep_track: r.u32()?,
    };
    Some((album, r.0.to_vec()))
}

// -- the library ----------------------------------------------------------------------------

pub struct Library {
    db: Database,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// The tables a track record is spread over, open in one write transaction.
struct Writer<'t> {
    tracks: redb::Table<'t, u32, &'static [u8]>,
    paths: redb::Table<'t, &'static str, (u32, u64, u64)>,
    by_title: redb::Table<'t, &'static [u8], u32>,
    by_album: redb::Table<'t, &'static [u8], u32>,
    flags: redb::Table<'t, u32, u8>,
}

impl<'t> Writer<'t> {
    fn open(wtx: &'t redb::WriteTransaction) -> Result<Writer<'t>, String> {
        Ok(Writer {
            tracks: wtx.open_table(TRACKS).map_err(err)?,
            paths: wtx.open_table(PATHS).map_err(err)?,
            by_title: wtx.open_table(BY_TITLE).map_err(err)?,
            by_album: wtx.open_table(BY_ALBUM).map_err(err)?,
            flags: wtx.open_table(FLAGS).map_err(err)?,
        })
    }

    /// Writes a record under `id`, replacing every index entry of the previous version.
    fn store(&mut self, id: u32, mut t: Track) -> Result<(), String> {
        let old = self.tracks.get(id).map_err(err)?.and_then(|g| Track::decode(id, g.value()));
        if let Some(old) = old {
            self.by_title.remove(key_title(&old).as_slice()).map_err(err)?;
            self.by_album.remove(key_album(&old).as_slice()).map_err(err)?;
            if old.path != t.path {
                self.paths.remove(old.path.as_str()).map_err(err)?;
            }
        }
        t.id = id;
        self.tracks.insert(id, t.encode().as_slice()).map_err(err)?;
        self.paths.insert(t.path.as_str(), (id, t.mtime, t.size)).map_err(err)?;
        self.by_title.insert(key_title(&t).as_slice(), id).map_err(err)?;
        self.by_album.insert(key_album(&t).as_slice(), id).map_err(err)?;
        if t.flags == 0 {
            self.flags.remove(id).map_err(err)?;
        } else {
            self.flags.insert(id, t.flags).map_err(err)?;
        }
        Ok(())
    }
}

/// One file's worth of new or changed data for `Library::apply`.
pub(crate) struct Upsert {
    pub id: Option<u32>,
    pub track: Track,
}

impl Library {
    /// Opens (or creates) the index. A damaged or outdated index is discarded and rebuilt by the
    /// next scan; the music files themselves are never touched.
    pub fn open(path: &std::path::Path) -> Result<Library, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        let mut last = String::new();
        for attempt in 0..2 {
            // A corrupt file must never take the app down, whether redb reports it or panics.
            let created = std::panic::catch_unwind(|| Database::builder().set_cache_size(CACHE_BYTES).create(path));
            match created {
                Ok(Ok(db)) => {
                    let lib = Library { db };
                    match lib.check_schema() {
                        Ok(()) => return Ok(lib),
                        Err(e) => last = e,
                    }
                }
                Ok(Err(DatabaseError::DatabaseAlreadyOpen)) => {
                    return Err("The library is already open in another Wisp window.".into());
                }
                Ok(Err(e)) => last = e.to_string(),
                Err(_) => last = "the index file is damaged".into(),
            }
            if attempt == 0 {
                // Unreadable index: move it aside and start over.
                let _ = std::fs::rename(path, path.with_extension("redb.bad"));
            }
        }
        Err(format!("cannot open the library index: {last}"))
    }

    fn check_schema(&self) -> Result<(), String> {
        let wtx = self.db.begin_write().map_err(err)?;
        let version = {
            let meta = wtx.open_table(META).map_err(err)?;
            let v = meta.get("schema").map_err(err)?.map(|v| v.value());
            v
        };
        if version.is_some_and(|v| v != SCHEMA) {
            // An older layout: drop it all; the next scan rebuilds the index from the files.
            wtx.delete_table(TRACKS).map_err(err)?;
            wtx.delete_table(PATHS).map_err(err)?;
            wtx.delete_table(BY_TITLE).map_err(err)?;
            wtx.delete_table(BY_ALBUM).map_err(err)?;
            wtx.delete_table(ALBUMS).map_err(err)?;
            wtx.delete_table(FLAGS).map_err(err)?;
            wtx.delete_table(LYRICS).map_err(err)?;
            wtx.delete_table(OLD_IDENT).map_err(err)?;
            wtx.delete_table(OLD_LOOKUPS).map_err(err)?;
            wtx.delete_table(META).map_err(err)?;
        }
        if version != Some(SCHEMA) {
            let mut meta = wtx.open_table(META).map_err(err)?;
            meta.insert("schema", SCHEMA).map_err(err)?;
        }
        // Touch every table so readers can always open them.
        wtx.open_table(TRACKS).map_err(err)?;
        wtx.open_table(PATHS).map_err(err)?;
        wtx.open_table(BY_TITLE).map_err(err)?;
        wtx.open_table(BY_ALBUM).map_err(err)?;
        wtx.open_table(ALBUMS).map_err(err)?;
        wtx.open_table(FLAGS).map_err(err)?;
        wtx.open_table(LYRICS).map_err(err)?;
        wtx.commit().map_err(err)
    }

    // -- reading ----------------------------------------------------------------------------

    pub fn track(&self, id: u32) -> Option<Track> {
        let rtx = self.db.begin_read().ok()?;
        let table = rtx.open_table(TRACKS).ok()?;
        let guard = table.get(id).ok()??;
        Track::decode(id, guard.value())
    }

    pub fn path_of(&self, id: u32) -> Option<PathBuf> {
        self.track(id).map(|t| PathBuf::from(t.path))
    }

    pub fn track_count(&self) -> usize {
        let count = || -> Option<u64> { self.db.begin_read().ok()?.open_table(TRACKS).ok()?.len().ok() };
        count().unwrap_or(0) as usize
    }

    pub fn album_count(&self) -> usize {
        let count = || -> Option<u64> { self.db.begin_read().ok()?.open_table(ALBUMS).ok()?.len().ok() };
        count().unwrap_or(0) as usize
    }

    /// Every track id in the requested order.
    pub fn ordered_ids(&self, sort: Sort) -> Vec<u32> {
        let mut ids = Vec::new();
        let Ok(rtx) = self.db.begin_read() else { return ids };
        match sort {
            Sort::Recent => {
                if let Ok(t) = rtx.open_table(TRACKS) {
                    if let Ok(iter) = t.iter() {
                        ids.extend(iter.rev().filter_map(|r| r.ok()).map(|(k, _)| k.value()));
                    }
                }
            }
            Sort::Title | Sort::Album => {
                let table = rtx.open_table(if sort == Sort::Title { BY_TITLE } else { BY_ALBUM });
                if let Ok(t) = table {
                    ids.reserve(t.len().unwrap_or(0) as usize);
                    if let Ok(iter) = t.iter() {
                        ids.extend(iter.filter_map(|r| r.ok()).map(|(_, v)| v.value()));
                    }
                }
            }
        }
        ids
    }

    /// Ids of tracks whose title, artist or album contains every word of `query`, in `sort`
    /// order. `cancel` is polled so a newer keystroke can abandon a long scan.
    pub fn search_ids(&self, query: &str, sort: Sort, cancel: &dyn Fn() -> bool) -> Vec<u32> {
        let words: Vec<String> = fold(query).split_whitespace().map(str::to_string).collect();
        if words.is_empty() {
            return self.ordered_ids(sort);
        }
        let mut hits = Vec::new();
        let Ok(rtx) = self.db.begin_read() else { return hits };
        let Ok(tracks) = rtx.open_table(TRACKS) else { return hits };
        for (n, id) in self.ordered_ids(sort).into_iter().enumerate() {
            if n % 512 == 0 && cancel() {
                return Vec::new();
            }
            let Ok(Some(guard)) = tracks.get(id) else { continue };
            let Some(t) = Track::decode(id, guard.value()) else { continue };
            let hay = fold(&format!("{} {} {} {}", t.title, t.artist, t.album, t.album_artist));
            if words.iter().all(|w| hay.contains(w.as_str())) {
                hits.push(id);
            }
        }
        hits
    }

    pub fn album(&self, index: u32) -> Option<Album> {
        let rtx = self.db.begin_read().ok()?;
        let table = rtx.open_table(ALBUMS).ok()?;
        let guard = table.get(index).ok()??;
        decode_album(index, guard.value()).map(|(a, _)| a)
    }

    /// Track ids of an album in disc/track order.
    pub fn album_tracks(&self, index: u32) -> Vec<u32> {
        let mut ids = Vec::new();
        let Ok(rtx) = self.db.begin_read() else { return ids };
        let Ok(albums) = rtx.open_table(ALBUMS) else { return ids };
        let Ok(Some(guard)) = albums.get(index) else { return ids };
        let Some((_, prefix)) = decode_album(index, guard.value()) else { return ids };
        let mut end = prefix.clone();
        if let Some(last) = end.last_mut() {
            *last = 1; // the prefix ends with NUL; NUL+1 is the first key past the album
        }
        let Ok(by_album) = rtx.open_table(BY_ALBUM) else { return ids };
        if let Ok(range) = by_album.range(prefix.as_slice()..end.as_slice()) {
            ids.extend(range.filter_map(|r| r.ok()).map(|(_, v)| v.value()));
        }
        ids
    }

    /// Albums whose name or artist matches every word of `query`.
    pub fn search_albums(&self, query: &str, cancel: &dyn Fn() -> bool) -> Vec<u32> {
        let words: Vec<String> = fold(query).split_whitespace().map(str::to_string).collect();
        let count = self.album_count() as u32;
        if words.is_empty() {
            return (0..count).collect();
        }
        let mut hits = Vec::new();
        let Ok(rtx) = self.db.begin_read() else { return hits };
        let Ok(table) = rtx.open_table(ALBUMS) else { return hits };
        for index in 0..count {
            if index % 512 == 0 && cancel() {
                return Vec::new();
            }
            let Ok(Some(guard)) = table.get(index) else { continue };
            let Some((a, _)) = decode_album(index, guard.value()) else { continue };
            let hay = fold(&format!("{} {}", a.name, a.artist));
            if words.iter().all(|w| hay.contains(w.as_str())) {
                hits.push(index);
            }
        }
        hits
    }

    /// Artists with their (contiguous) run of albums.
    pub fn artists(&self) -> Vec<Artist> {
        let mut out: Vec<Artist> = Vec::new();
        let Ok(rtx) = self.db.begin_read() else { return out };
        let Ok(table) = rtx.open_table(ALBUMS) else { return out };
        let Ok(iter) = table.iter() else { return out };
        for item in iter.filter_map(|r| r.ok()) {
            let index = item.0.value();
            let Some((a, _)) = decode_album(index, item.1.value()) else { continue };
            match out.last_mut() {
                Some(last) if fold(&last.name) == fold(&a.artist) => last.albums += 1,
                _ => out.push(Artist { name: a.artist, first_album: index, albums: 1 }),
            }
        }
        out
    }

    // -- writing (used by the scanner) ------------------------------------------------------

    /// Known `(id, mtime, size)` for each path, in one snapshot.
    pub(crate) fn known(&self, paths: &[String]) -> Vec<Option<(u32, u64, u64)>> {
        let Ok(rtx) = self.db.begin_read() else { return vec![None; paths.len()] };
        let Ok(table) = rtx.open_table(PATHS) else { return vec![None; paths.len()] };
        paths.iter().map(|p| table.get(p.as_str()).ok().flatten().map(|g| g.value())).collect()
    }

    /// The id the next new track will get.
    pub(crate) fn next_id(&self) -> u32 {
        let read = || -> Option<u64> { self.db.begin_read().ok()?.open_table(META).ok()?.get("next_id").ok()?.map(|v| v.value()) };
        read().unwrap_or(1) as u32
    }

    /// Ids that were not seen by a scan (`seen[id]`), excluding paths under `keep_prefixes`
    /// (folders that could not be read and must not be treated as deleted).
    pub(crate) fn stale_ids(&self, seen: &[bool], keep_prefixes: &[String]) -> Vec<u32> {
        let mut out = Vec::new();
        let Ok(rtx) = self.db.begin_read() else { return out };
        let Ok(table) = rtx.open_table(PATHS) else { return out };
        let Ok(iter) = table.iter() else { return out };
        for item in iter.filter_map(|r| r.ok()) {
            let id = item.1.value().0;
            if seen.get(id as usize).copied().unwrap_or(false) {
                continue;
            }
            if keep_prefixes.iter().any(|p| item.0.value().starts_with(p.as_str())) {
                continue;
            }
            out.push(id);
        }
        out
    }

    /// Inserts or replaces tracks in one transaction. Returns the ids used, in order.
    pub(crate) fn apply(&self, batch: Vec<Upsert>, durable: bool) -> Result<Vec<u32>, String> {
        let mut wtx = self.db.begin_write().map_err(err)?;
        wtx.set_durability(if durable { Durability::Immediate } else { Durability::None }).map_err(err)?;
        let mut used = Vec::with_capacity(batch.len());
        {
            let mut w = Writer::open(&wtx)?;
            let mut meta = wtx.open_table(META).map_err(err)?;
            let mut next = meta.get("next_id").map_err(err)?.map_or(1, |v| v.value()) as u32;
            for up in batch {
                let id = match up.id {
                    Some(id) => id,
                    None => {
                        let id = next;
                        next += 1;
                        id
                    }
                };
                w.store(id, up.track)?;
                used.push(id);
            }
            meta.insert("next_id", next as u64).map_err(err)?;
        }
        wtx.commit().map_err(err)?;
        Ok(used)
    }

    /// Replaces a track's record (and, when given, its saved lyrics) only if its file is still the
    /// one `expected` describes (same path, size and modification time): a rescan that ran while
    /// the user was editing must not be overwritten. Returns whether it was stored.
    pub fn save_edit(&self, expected: &Track, new: Track, lyrics: Option<&str>) -> Result<bool, String> {
        let wtx = self.db.begin_write().map_err(err)?;
        let stored = {
            let mut w = Writer::open(&wtx)?;
            let current = w.tracks.get(expected.id).map_err(err)?.and_then(|g| Track::decode(expected.id, g.value()));
            let same = current.is_some_and(|c| c.path == expected.path && c.mtime == expected.mtime && c.size == expected.size);
            if same {
                w.store(expected.id, new)?;
                if let Some(text) = lyrics {
                    let mut table = wtx.open_table(LYRICS).map_err(err)?;
                    if text.trim().is_empty() {
                        table.remove(expected.id).map_err(err)?;
                    } else {
                        table.insert(expected.id, text).map_err(err)?;
                    }
                }
            }
            same
        };
        wtx.commit().map_err(err)?;
        Ok(stored)
    }

    /// The flagged tracks (see [`flag`]), in id order.
    pub fn flagged_ids(&self) -> Vec<u32> {
        let mut out = Vec::new();
        let Ok(rtx) = self.db.begin_read() else { return out };
        let Ok(table) = rtx.open_table(FLAGS) else { return out };
        let Ok(iter) = table.iter() else { return out };
        out.extend(iter.filter_map(|r| r.ok()).map(|(k, _)| k.value()));
        out
    }

    pub fn flagged_count(&self) -> usize {
        let count = || -> Option<u64> { self.db.begin_read().ok()?.open_table(FLAGS).ok()?.len().ok() };
        count().unwrap_or(0) as usize
    }

    /// Lyrics saved in Wisp for this track (not in its file).
    pub fn lyrics(&self, id: u32) -> Option<String> {
        let rtx = self.db.begin_read().ok()?;
        let table = rtx.open_table(LYRICS).ok()?;
        let guard = table.get(id).ok()??;
        Some(guard.value().to_string())
    }

    /// Removes tracks by id.
    pub(crate) fn remove(&self, ids: &[u32]) -> Result<(), String> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut wtx = self.db.begin_write().map_err(err)?;
        wtx.set_durability(Durability::None).map_err(err)?;
        {
            let mut tracks = wtx.open_table(TRACKS).map_err(err)?;
            let mut paths = wtx.open_table(PATHS).map_err(err)?;
            let mut by_title = wtx.open_table(BY_TITLE).map_err(err)?;
            let mut by_album = wtx.open_table(BY_ALBUM).map_err(err)?;
            let mut flags = wtx.open_table(FLAGS).map_err(err)?;
            let mut lyrics = wtx.open_table(LYRICS).map_err(err)?;
            for &id in ids {
                flags.remove(id).map_err(err)?;
                lyrics.remove(id).map_err(err)?;
                let old = tracks.remove(id).map_err(err)?.and_then(|g| Track::decode(id, g.value()));
                if let Some(old) = old {
                    paths.remove(old.path.as_str()).map_err(err)?;
                    by_title.remove(key_title(&old).as_slice()).map_err(err)?;
                    by_album.remove(key_album(&old).as_slice()).map_err(err)?;
                }
            }
        }
        wtx.commit().map_err(err)
    }

    /// Rebuilds the album list from the album-ordered index and makes the scan durable.
    pub(crate) fn rebuild_albums(&self) -> Result<(), String> {
        // Group consecutive keys that share an album prefix.
        struct Group {
            prefix: Vec<u8>,
            first_id: u32,
            count: u32,
        }
        let mut groups: Vec<Group> = Vec::new();
        {
            let rtx = self.db.begin_read().map_err(err)?;
            let by_album = rtx.open_table(BY_ALBUM).map_err(err)?;
            for item in by_album.iter().map_err(err)?.filter_map(|r| r.ok()) {
                let prefix = album_of_key(item.0.value());
                match groups.last_mut() {
                    Some(g) if g.prefix == prefix => g.count += 1,
                    _ => groups.push(Group { prefix: prefix.to_vec(), first_id: item.1.value(), count: 1 }),
                }
            }
        }
        let mut wtx = self.db.begin_write().map_err(err)?;
        wtx.set_durability(Durability::Immediate).map_err(err)?;
        wtx.delete_table(ALBUMS).map_err(err)?;
        {
            let tracks = wtx.open_table(TRACKS).map_err(err)?;
            let mut albums = wtx.open_table(ALBUMS).map_err(err)?;
            for (index, g) in groups.iter().enumerate() {
                let Some(first) = tracks.get(g.first_id).map_err(err)?.and_then(|t| Track::decode(g.first_id, t.value())) else {
                    continue;
                };
                let album = Album {
                    index: index as u32,
                    name: first.album.clone(),
                    artist: first.album_artist.clone(),
                    year: first.year,
                    tracks: g.count,
                    rep_track: g.first_id,
                };
                albums.insert(index as u32, encode_album(&album, &g.prefix).as_slice()).map_err(err)?;
            }
        }
        wtx.commit().map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wisp-lib-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn track(path: &str, title: &str, artist: &str, album: &str, disc: u16, no: u16) -> Track {
        Track {
            path: path.into(),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            album_artist: artist.into(),
            track_no: no,
            disc_no: disc,
            duration_ms: 180_000,
            mtime: 1,
            size: 2,
            ..Default::default()
        }
    }

    fn lib_with(name: &str, tracks: Vec<Track>) -> (Library, PathBuf) {
        let dir = temp(name);
        let lib = Library::open(&dir.join("lib.redb")).unwrap();
        lib.apply(tracks.into_iter().map(|track| Upsert { id: None, track }).collect(), true).unwrap();
        lib.rebuild_albums().unwrap();
        (lib, dir)
    }

    fn titles(lib: &Library, ids: &[u32]) -> Vec<String> {
        ids.iter().map(|&i| lib.track(i).unwrap().title).collect()
    }

    #[test]
    fn records_round_trip_including_unicode_and_long_text() {
        let mut t = track("C:\\Música\\Sigur Rós\\Ágætis byrjun\\01 Intro.flac", "Starálfur ♪ 日本語", "Sigur Rós", "Ágætis byrjun", 1, 1);
        t.year = 1999;
        t.title.push_str(&"x".repeat(70_000));
        let back = Track::decode(5, &t.encode()).unwrap();
        assert!(back.title.len() <= 65_535 && back.title.starts_with("Starálfur ♪ 日本語"));
        assert_eq!((back.artist.as_str(), back.year, back.id), ("Sigur Rós", 1999, 5));
        assert!(Track::decode(1, &[1, 2, 3]).is_none(), "truncated records are rejected, not panicked on");
    }

    #[test]
    fn sorting_ignores_case_accents_and_leading_articles() {
        let (lib, dir) = lib_with(
            "sort",
            vec![
                track("a", "The Zebra", "X", "A1", 1, 1),
                track("b", "apple", "X", "A1", 1, 2),
                track("c", "Éclair", "X", "A1", 1, 3),
                track("d", "Banana", "X", "A1", 1, 4),
                track("e", "10 Things", "X", "A1", 1, 5),
            ],
        );
        let ids = lib.ordered_ids(Sort::Title);
        assert_eq!(titles(&lib, &ids), ["10 Things", "apple", "Banana", "Éclair", "The Zebra"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn albums_group_and_list_their_tracks_in_disc_track_order() {
        let (lib, dir) = lib_with(
            "albums",
            vec![
                track("1", "Two", "The Band", "Record", 1, 2),
                track("2", "Three (disc 2)", "The Band", "Record", 2, 1),
                track("3", "One", "The Band", "Record", 1, 1),
                track("4", "Solo", "Other", "Single", 1, 1),
            ],
        );
        assert_eq!(lib.album_count(), 2);
        let band = (0..2).filter_map(|i| lib.album(i)).find(|a| a.name == "Record").unwrap();
        assert_eq!((band.tracks, band.artist.as_str()), (3, "The Band"));
        assert_eq!(titles(&lib, &lib.album_tracks(band.index)), ["One", "Two", "Three (disc 2)"]);
        // "The Band" files under B, before "Other"? No: B < O.
        assert_eq!(lib.album(0).unwrap().name, "Record");
        let artists = lib.artists();
        assert_eq!(artists.iter().map(|a| (a.name.as_str(), a.albums)).collect::<Vec<_>>(), [("The Band", 1), ("Other", 1)]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn search_matches_every_word_across_fields_ignoring_accents() {
        let (lib, dir) = lib_with(
            "search",
            vec![
                track("1", "Jóga", "Björk", "Homogenic", 1, 1),
                track("2", "Army of Me", "Björk", "Post", 1, 1),
                track("3", "Joga Bonito", "Someone", "Football", 1, 1),
            ],
        );
        let never = || false;
        assert_eq!(titles(&lib, &lib.search_ids("bjork", Sort::Title, &never)).len(), 2);
        assert_eq!(titles(&lib, &lib.search_ids("joga", Sort::Title, &never)), ["Jóga", "Joga Bonito"]);
        assert_eq!(titles(&lib, &lib.search_ids("bjork homo", Sort::Title, &never)), ["Jóga"]);
        assert!(lib.search_ids("nothing like this", Sort::Title, &never).is_empty());
        assert_eq!(lib.search_ids("", Sort::Title, &never).len(), 3);
        assert_eq!(lib.search_albums("post", &never).len(), 1);
        assert!(lib.search_ids("bjork", Sort::Title, &|| true).is_empty(), "cancelled searches return nothing");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn updating_and_removing_keeps_every_index_consistent() {
        let (lib, dir) = lib_with(
            "update",
            vec![track("1", "Old Title", "A", "Alb", 1, 1), track("2", "Keep", "A", "Alb", 1, 2)],
        );
        let id = lib.known(&["1".to_string()])[0].unwrap().0;
        let mut renamed = track("1", "New Title", "A", "Alb", 1, 1);
        renamed.mtime = 9;
        lib.apply(vec![Upsert { id: Some(id), track: renamed }], true).unwrap();
        assert_eq!(lib.track_count(), 2);
        assert_eq!(titles(&lib, &lib.ordered_ids(Sort::Title)), ["Keep", "New Title"]);
        assert_eq!(titles(&lib, &lib.ordered_ids(Sort::Album)), ["New Title", "Keep"]);
        lib.remove(&[id]).unwrap();
        lib.rebuild_albums().unwrap();
        assert_eq!(lib.track_count(), 1);
        assert_eq!(lib.ordered_ids(Sort::Title).len(), 1);
        assert_eq!(lib.album(0).unwrap().tracks, 1);
        assert!(lib.known(&["1".to_string()])[0].is_none());
        // Newest first.
        lib.apply(vec![Upsert { id: None, track: track("3", "Newest", "A", "Alb", 1, 3) }], true).unwrap();
        assert_eq!(titles(&lib, &lib.ordered_ids(Sort::Recent))[0], "Newest");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn flagged(path: &str, title: &str, flags: u8) -> Track {
        Track { flags, ..track(path, title, "A", "Alb", 1, 1) }
    }

    #[test]
    fn flagged_songs_are_listed_until_their_record_says_otherwise() {
        let (lib, dir) = lib_with(
            "flags",
            vec![
                flagged("1", "track_01", flag::NO_ARTIST | flag::BAD_TITLE | flag::NO_COVER),
                flagged("2", "Complete", 0),
                flagged("3", "Only a cover missing", flag::NO_COVER),
            ],
        );
        assert_eq!((lib.flagged_count(), lib.flagged_ids().len()), (2, 2));
        let first = lib.known(&["1".to_string()])[0].unwrap().0;
        assert_eq!(lib.track(first).unwrap().flags, 7, "flags are part of the record");
        assert_eq!(flag::describe(7), "No artist · No real title · No cover");
        assert_eq!(flag::describe(0), "");

        // A new record without flags takes the song off the list; removing a song does too.
        lib.apply(vec![Upsert { id: Some(first), track: flagged("1", "Fixed", 0) }], true).unwrap();
        assert_eq!(lib.flagged_count(), 1);
        let third = lib.known(&["3".to_string()])[0].unwrap().0;
        lib.remove(&[third]).unwrap();
        assert_eq!((lib.flagged_count(), lib.flagged_ids()), (0, vec![]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_edit_is_stored_only_while_the_file_is_the_one_that_was_edited() {
        let (lib, dir) = lib_with("edit", vec![flagged("1", "track_01", flag::BAD_TITLE)]);
        let id = lib.known(&["1".to_string()])[0].unwrap().0;
        let current = lib.track(id).unwrap();

        let renamed = Track { title: "Real Title".into(), flags: 0, ..current.clone() };
        assert!(lib.save_edit(&current, renamed.clone(), Some("la la la")).unwrap());
        assert_eq!((lib.track(id).unwrap().title.as_str(), lib.flagged_count()), ("Real Title", 0));
        assert_eq!(lib.lyrics(id).as_deref(), Some("la la la"));
        assert_eq!(titles(&lib, &lib.ordered_ids(Sort::Title)), ["Real Title"], "the sort index follows");

        // `current` is out of date now (a rescan or another save replaced the record): refused.
        let stale = Track { mtime: 99, ..current.clone() };
        assert!(!lib.save_edit(&stale, Track { title: "Lost".into(), ..renamed.clone() }, None).unwrap());
        assert_eq!(lib.track(id).unwrap().title, "Real Title");
        // Lyrics are kept when none are given, and removed by blank ones.
        assert!(lib.save_edit(&current, Track { title: "Again".into(), ..renamed.clone() }, None).unwrap());
        assert_eq!(lib.lyrics(id).as_deref(), Some("la la la"));
        assert!(lib.save_edit(&current, renamed, Some("  ")).unwrap());
        assert_eq!(lib.lyrics(id), None);
        lib.remove(&[id]).unwrap();
        assert_eq!(lib.lyrics(id), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_index_of_an_older_layout_is_discarded_and_the_old_tables_with_it() {
        let dir = temp("schema");
        let path = dir.join("lib.redb");
        {
            let lib = Library::open(&path).unwrap();
            lib.apply(vec![Upsert { id: None, track: track("1", "Song", "A", "Alb", 1, 1) }], true).unwrap();
            // Pretend it was written by the version that identified songs online.
            let wtx = lib.db.begin_write().unwrap();
            wtx.open_table(META).unwrap().insert("schema", 2u64).unwrap();
            wtx.open_table(OLD_IDENT).unwrap().insert(1u32, 1u8).unwrap();
            wtx.open_table(OLD_LOOKUPS).unwrap().insert("a - b", &[1u8, 2][..]).unwrap();
            wtx.commit().unwrap();
            assert_eq!(lib.track_count(), 1);
        }
        let lib = Library::open(&path).unwrap();
        assert_eq!(lib.track_count(), 0, "the next scan reads the files again");
        let rtx = lib.db.begin_read().unwrap();
        assert!(rtx.open_table(OLD_IDENT).is_err() && rtx.open_table(OLD_LOOKUPS).is_err(), "nothing of the old layout is kept");
        assert_eq!(rtx.open_table(META).unwrap().get("schema").unwrap().unwrap().value(), SCHEMA);
        drop(rtx);
        drop(lib);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_second_open_is_refused_and_a_damaged_file_is_rebuilt() {
        let dir = temp("open");
        let path = dir.join("lib.redb");
        let first = Library::open(&path).unwrap();
        let second = Library::open(&path);
        assert!(second.is_err(), "the index belongs to one running instance");
        drop(first);
        std::fs::write(&path, b"this is not a database at all").unwrap();
        let lib = Library::open(&path).expect("a damaged index is moved aside and recreated");
        assert_eq!(lib.track_count(), 0);
        assert!(path.with_extension("redb.bad").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
