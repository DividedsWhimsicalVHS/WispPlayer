//! The track editor's controller: opens a song from the unidentified list, runs the lookup, fills
//! the form from the match the user picks and saves it.
//!
//! Nothing here decides anything for the user. The form starts from the song's own tags, a pick
//! only fills it in (every field stays editable), and only Save changes anything (`edit::save`).
//! The lookup and the save run on worker threads and report back through `on_ui_thread`.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use slint::{ComponentHandle, Image, Model, ModelRc, SharedString, VecModel};

use super::{Ctl, library_edited, on_ui_thread, to_image, toast, ui_of};
use crate::art::{self, Rgb};
use crate::edit::{self, Fields};
use crate::library::{self, Track, filename, flag};
use crate::lookup::{Event, Lookup, Lyrics as Found, Row};
use crate::models::fmt_time;
use crate::settings;
use crate::{App, ResultRow};

pub struct Editor {
    lookup: Lookup,
    open: Option<Open>,
}

impl Editor {
    pub fn new() -> Editor {
        Editor { lookup: Lookup::new(), open: None }
    }
}

/// A cover that Save will write: a match's from the Cover Art Archive, or an image the user chose.
struct NewCover {
    bytes: Vec<u8>,
    preview: Image,
    note: &'static str,
}

/// What the editor knows about the song it is open on, besides what the form itself holds.
struct Open {
    /// The song as the library had it when the editor opened.
    track: Track,
    /// The search whose events count; answers to older ones are dropped.
    search: u32,
    rows: Vec<Row>,
    lyrics: Vec<Option<Found>>,
    results: Rc<VecModel<ResultRow>>,
    picked: Option<usize>,
    /// The search has reported everything it will.
    done: bool,
    /// The song's own cover, shown until a match brings another.
    own_cover: Option<Image>,
    own_cover_loaded: bool,
    new_cover: Option<NewCover>,
    /// What the lyrics box was last filled with from a match: a box that still says this has not
    /// been typed in.
    lyrics_filled: String,
}

// -- what the editor starts with ------------------------------------------------------------

/// What the search box starts with: the file name without track number and upload junk when the
/// title is only that, else the title, with the artist when there is one.
fn first_query(t: &Track) -> String {
    let stem = Path::new(&t.path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let named = filename::cleaned(&stem);
    if t.flags & flag::BAD_TITLE != 0 && !named.is_empty() {
        named
    } else if library::unknown_artist(&t.artist) {
        t.title.clone()
    } else {
        format!("{} - {}", t.artist, t.title)
    }
}

/// The form as the song's own tags fill it: "Unknown Artist" and "Unknown Album" are blanks, and a
/// title that is a file name is shown without its track number and upload tags.
fn first_fields(t: &Track) -> Fields {
    let known = |s: &str| if library::unknown_artist(s) { String::new() } else { s.to_string() };
    let title = match t.flags & flag::BAD_TITLE != 0 {
        true => Some(filename::cleaned(&t.title)).filter(|c| !c.is_empty()).unwrap_or_else(|| t.title.clone()),
        false => t.title.clone(),
    };
    Fields {
        title,
        artist: known(&t.artist),
        album: if t.album == "Unknown Album" { String::new() } else { t.album.clone() },
        album_artist: known(&t.album_artist),
        year: t.year,
        track_no: t.track_no,
    }
}

fn number_text(n: u16) -> SharedString {
    if n > 0 { n.to_string().into() } else { SharedString::new() }
}

/// "Graduation · 2007 · Track 3"
fn detail(r: &Row) -> String {
    let mut parts = Vec::new();
    if !r.album.is_empty() {
        parts.push(r.album.clone());
    }
    if r.year > 0 {
        parts.push(r.year.to_string());
    }
    if r.track_no > 0 {
        parts.push(format!("Track {}", r.track_no));
    }
    parts.join(" · ")
}

fn result_row(r: &Row) -> ResultRow {
    ResultRow {
        title: r.title.as_str().into(),
        artist: r.artist.as_str().into(),
        detail: detail(r).into(),
        thumb: Image::default(),
        has_thumb: false,
        lyrics: 0,
        same_length: r.length_match,
    }
}

const NO_MATCHES: &str = "No matches. Try fewer words, or just the title.";

fn matches_text(n: usize) -> String {
    if n == 1 { "1 match".to_string() } else { format!("{n} matches") }
}

// -- opening and closing ------------------------------------------------------------------------

/// Opens row `row` of the unidentified list.
pub fn open(ctl: &Ctl, row: usize) {
    let Some(ui) = ui_of(ctl) else { return };
    let track = {
        let c = ctl.borrow();
        c.flagged.id_at(row).and_then(|id| c.lib.track(id))
    };
    let Some(track) = track else { return };
    let app = ui.global::<App>();
    let path = PathBuf::from(&track.path);

    let mut file = path.file_name().map_or_else(|| track.path.clone(), |n| n.to_string_lossy().into_owned());
    if track.duration_ms > 0 {
        file = format!("{file}  ·  {}", fmt_time(track.duration_ms as u64));
    }
    let query = first_query(&track);
    let form = first_fields(&track);
    app.set_ed_file(file.into());
    app.set_ed_reason(flag::describe(track.flags).into());
    app.set_ed_where(edit::describe_target(&path).into());
    app.set_ed_query(query.clone().into());
    app.set_ed_title(form.title.into());
    app.set_ed_artist(form.artist.into());
    app.set_ed_album(form.album.into());
    app.set_ed_album_artist(form.album_artist.into());
    app.set_ed_year(number_text(form.year));
    app.set_ed_track(number_text(form.track_no));

    let results = Rc::new(VecModel::<ResultRow>::default());
    app.set_ed_results(ModelRc::from(results.clone()));
    app.set_ed_picked(-1);
    app.set_ed_status(SharedString::new());
    app.set_ed_problem(false);
    app.set_ed_busy(false);
    app.set_ed_cover(Image::default());
    app.set_ed_has_cover(false);
    app.set_ed_cover_busy(false);
    app.set_ed_cover_note(SharedString::new());
    app.set_ed_lyrics(SharedString::new());
    app.set_ed_lyrics_serial(app.get_ed_lyrics_serial() + 1);
    app.set_ed_lyrics_note(SharedString::new());
    app.set_ed_lyrics_avail(0);
    app.set_ed_lyrics_mode(0);
    app.set_ed_saving(false);
    app.set_ed_error(SharedString::new());
    app.set_ed_open(true);
    {
        let mut c = ctl.borrow_mut();
        c.editor.lookup.cancel();
        c.editor.open = Some(Open {
            track: track.clone(),
            search: 0,
            rows: Vec::new(),
            lyrics: Vec::new(),
            results,
            picked: None,
            done: false,
            own_cover: None,
            own_cover_loaded: false,
            new_cover: None,
            lyrics_filled: String::new(),
        });
    }
    load_own_cover(&track);
    search(ctl, &query);
}

pub fn close(ctl: &Ctl) {
    {
        let mut c = ctl.borrow_mut();
        c.editor.lookup.cancel();
        c.editor.open = None;
    }
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    app.set_ed_open(false);
    // Nothing of the song stays in memory once the editor is shut.
    app.set_ed_results(ModelRc::new(VecModel::<ResultRow>::default()));
    app.set_ed_picked(-1);
    app.set_ed_busy(false);
    app.set_ed_cover(Image::default());
    app.set_ed_lyrics(SharedString::new());
}

/// The cover the song has now, for the preview (the file's own, else one saved in Wisp, else its
/// folder's), read off the UI thread.
fn load_own_cover(track: &Track) {
    let (id, path, saved) = (track.id, PathBuf::from(&track.path), track.cover.clone());
    std::thread::spawn(move || {
        let embedded = art::read_embedded(&path);
        let rgb = art::cover_with_fallback(&path, embedded.cover, &saved).and_then(|bytes| art::decode(&bytes, 240));
        on_ui_thread(move |ctl| own_cover_loaded(ctl, id, rgb));
    });
}

fn own_cover_loaded(ctl: &Ctl, id: u32, rgb: Option<Rgb>) {
    let Some(ui) = ui_of(ctl) else { return };
    let mut c = ctl.borrow_mut();
    let Some(open) = c.editor.open.as_mut().filter(|o| o.track.id == id) else { return };
    open.own_cover = rgb.as_ref().map(to_image);
    open.own_cover_loaded = true;
    let app = ui.global::<App>();
    if !app.get_ed_cover_busy() {
        show_cover(&app, open);
    }
}

/// The cover box: the new cover if there is one, else the song's own.
fn show_cover(app: &App, open: &Open) {
    let (image, note) = match (&open.new_cover, &open.own_cover) {
        (Some(new), _) => (Some(new.preview.clone()), new.note),
        (None, Some(own)) => (Some(own.clone()), "Current cover"),
        (None, None) => (None, if open.own_cover_loaded { "No cover yet" } else { "" }),
    };
    app.set_ed_has_cover(image.is_some());
    app.set_ed_cover(image.unwrap_or_default());
    app.set_ed_cover_note(note.into());
}

// -- searching and picking -----------------------------------------------------------------

/// Starts a search for `query` (a new one replaces the one running).
pub fn search(ctl: &Ctl, query: &str) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let query = query.trim();
    if query.is_empty() {
        app.set_ed_problem(false);
        app.set_ed_status("Type the song's title, and the artist if you know it.".into());
        return;
    }
    let mut c = ctl.borrow_mut();
    let contact = c.settings.string("mb_contact", "");
    let Some(ms) = c.editor.open.as_ref().map(|o| o.track.duration_ms) else { return };
    let number = c.editor.lookup.search(query, ms, &contact, |number, event| on_ui_thread(move |ctl| arrived(ctl, number, event)));
    let Some(open) = c.editor.open.as_mut() else { return };
    open.search = number;
    open.rows.clear();
    open.lyrics.clear();
    open.picked = None;
    open.done = false;
    open.results.set_vec(Vec::new());
    app.set_ed_picked(-1);
    app.set_ed_busy(true);
    app.set_ed_problem(false);
    app.set_ed_status("Searching MusicBrainz…".into());
    app.set_ed_lyrics_avail(0);
    app.set_ed_lyrics_note(SharedString::new());
    // A cover still on its way belongs to the old results.
    app.set_ed_cover_busy(false);
    show_cover(&app, open);
}

/// What the lookup reported for search `number`.
fn arrived(ctl: &Ctl, number: u32, event: Event) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let mut c = ctl.borrow_mut();
    let Some(open) = c.editor.open.as_mut().filter(|o| o.search == number) else { return };
    match event {
        Event::Results(rows) => {
            open.results.set_vec(rows.iter().map(result_row).collect::<Vec<_>>());
            open.lyrics = vec![None; rows.len()];
            let n = rows.len();
            open.rows = rows;
            app.set_ed_problem(false);
            if n == 0 {
                app.set_ed_busy(false);
                app.set_ed_status(NO_MATCHES.into());
            } else {
                app.set_ed_status(format!("{} · looking for lyrics and covers…", matches_text(n)).into());
            }
        }
        Event::Lyrics { row, lyrics } => {
            let level = if lyrics.synced.trim().is_empty() { 1 } else { 2 };
            if let Some(slot) = open.lyrics.get_mut(row) {
                *slot = Some(lyrics);
            }
            if let Some(mut data) = open.results.row_data(row) {
                data.lyrics = level;
                open.results.set_row_data(row, data);
            }
            if open.picked == Some(row) {
                apply_lyrics(&app, open, row, false);
            }
        }
        Event::Thumb { row, bytes } => {
            if let (Some(rgb), Some(mut data)) = (art::decode(&bytes, 136), open.results.row_data(row)) {
                data.thumb = to_image(&rgb);
                data.has_thumb = true;
                open.results.set_row_data(row, data);
            }
        }
        Event::Cover { row, bytes } => {
            if open.picked != Some(row) {
                return;
            }
            app.set_ed_cover_busy(false);
            open.new_cover = art::decode(&bytes, 240).map(|rgb| NewCover { bytes, preview: to_image(&rgb), note: "Cover from the Cover Art Archive" });
            show_cover(&app, open);
        }
        Event::NoCover { row, why } => {
            if open.picked != Some(row) {
                return;
            }
            app.set_ed_cover_busy(false);
            open.new_cover = None;
            show_cover(&app, open);
            app.set_ed_problem(true);
            app.set_ed_status(why.into());
        }
        Event::Busy => {
            app.set_ed_problem(false);
            app.set_ed_status("MusicBrainz is busy, trying again… A contact in Settings gets you served sooner.".into());
        }
        Event::Done => {
            open.done = true;
            app.set_ed_busy(false);
            app.set_ed_problem(false);
            app.set_ed_status(if open.rows.is_empty() { NO_MATCHES.to_string() } else { matches_text(open.rows.len()) }.into());
            if let Some(row) = open.picked {
                apply_lyrics(&app, open, row, false);
            }
        }
        Event::Problem(message) => {
            app.set_ed_busy(false);
            app.set_ed_problem(true);
            app.set_ed_status(message.into());
        }
    }
}

/// Puts a result's lyrics in the box (synced ones first). Unless `force`, a box the user has typed
/// in is left alone.
fn apply_lyrics(app: &App, open: &mut Open, row: usize, force: bool) {
    if !force && app.get_ed_lyrics().as_str() != open.lyrics_filled {
        return;
    }
    let (avail, mode, text, note) = match open.lyrics.get(row).and_then(Option::as_ref) {
        Some(l) if !l.synced.trim().is_empty() => (if l.plain.trim().is_empty() { 2 } else { 3 }, 0, l.synced.clone(), "Synced lyrics from LRCLIB"),
        Some(l) if !l.plain.trim().is_empty() => (1, 1, l.plain.clone(), "Lyrics from LRCLIB"),
        _ => (0, 0, String::new(), if open.done { "No lyrics found for this song" } else { "Looking for lyrics…" }),
    };
    app.set_ed_lyrics_avail(avail);
    app.set_ed_lyrics_mode(mode);
    app.set_ed_lyrics_note(note.into());
    set_lyrics_text(app, open, text);
}

fn set_lyrics_text(app: &App, open: &mut Open, text: String) {
    if app.get_ed_lyrics().as_str() != text {
        app.set_ed_lyrics(text.as_str().into());
        app.set_ed_lyrics_serial(app.get_ed_lyrics_serial() + 1);
    }
    open.lyrics_filled = text;
}

/// A match was chosen: its details fill the form, ready to be looked over and changed.
pub fn pick(ctl: &Ctl, index: usize) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let fetch = {
        let mut c = ctl.borrow_mut();
        let contact = c.settings.string("mb_contact", "");
        let Some(open) = c.editor.open.as_mut() else { return };
        let Some(row) = open.rows.get(index).cloned() else { return };
        open.picked = Some(index);
        app.set_ed_picked(index as i32);
        app.set_ed_error(SharedString::new());
        app.set_ed_title(row.title.as_str().into());
        app.set_ed_artist(row.artist.as_str().into());
        app.set_ed_album(row.album.as_str().into());
        app.set_ed_album_artist(row.album_artist.as_str().into());
        app.set_ed_year(number_text(row.year));
        app.set_ed_track(number_text(row.track_no));
        apply_lyrics(&app, open, index, true);

        open.new_cover = None;
        let has_cover = !(row.group_id.is_empty() && row.release_id.is_empty());
        if has_cover {
            // The small cover from the list is on screen already; the full one is on its way.
            match open.results.row_data(index).filter(|d| d.has_thumb) {
                Some(data) => {
                    app.set_ed_cover(data.thumb);
                    app.set_ed_has_cover(true);
                }
                None => show_cover(&app, open),
            }
            app.set_ed_cover_note("Loading the cover…".into());
            app.set_ed_cover_busy(true);
        } else {
            show_cover(&app, open);
        }
        has_cover.then(|| (row.group_id, row.release_id, contact))
    };
    if let Some((group, release, contact)) = fetch {
        ctl.borrow().editor.lookup.fetch_cover(index, group, release, &contact, |number, event| {
            on_ui_thread(move |ctl| arrived(ctl, number, event))
        });
    }
}

/// Synced or plain lyrics, when the match has both.
pub fn lyrics_mode(ctl: &Ctl, mode: i32) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let mut c = ctl.borrow_mut();
    let Some(open) = c.editor.open.as_mut() else { return };
    let text = open
        .picked
        .and_then(|row| open.lyrics.get(row))
        .and_then(Option::as_ref)
        .map(|l| if mode == 0 { l.synced.clone() } else { l.plain.clone() })
        .unwrap_or_default();
    if text.trim().is_empty() {
        return;
    }
    app.set_ed_lyrics_mode(mode);
    set_lyrics_text(&app, open, text);
}

/// "Choose image…": a JPEG or PNG from disk for the cover.
pub fn choose_cover(ctl: &Ctl) {
    let _ = ctl;
    std::thread::spawn(|| {
        let picked = rfd::FileDialog::new().set_title("Choose a cover image").add_filter("Images", &["jpg", "jpeg", "png"]).pick_file();
        let Some(path) = picked else { return };
        let bytes = std::fs::metadata(&path).ok().filter(|m| m.len() <= 16 << 20).and_then(|_| std::fs::read(&path).ok());
        on_ui_thread(move |ctl| cover_chosen(ctl, bytes));
    });
}

fn cover_chosen(ctl: &Ctl, bytes: Option<Vec<u8>>) {
    let Some(ui) = ui_of(ctl) else { return };
    let Some(bytes) = bytes else { return toast(ctl, "Couldn't read that image") };
    let Some(rgb) = art::decode(&bytes, 240) else { return toast(ctl, "Only JPEG and PNG images can be used as a cover") };
    let mut c = ctl.borrow_mut();
    let Some(open) = c.editor.open.as_mut() else { return };
    open.new_cover = Some(NewCover { bytes, preview: to_image(&rgb), note: "The image you chose" });
    let app = ui.global::<App>();
    app.set_ed_cover_busy(false);
    show_cover(&app, open);
}

// -- saving ---------------------------------------------------------------------------------

/// The form as `Fields`, or what is wrong with it.
fn read_fields(app: &App) -> Result<Fields, String> {
    let (year, track) = (app.get_ed_year(), app.get_ed_track());
    let (year, track) = (year.trim(), track.trim());
    let fields = Fields {
        title: app.get_ed_title().trim().to_string(),
        artist: app.get_ed_artist().trim().to_string(),
        album: app.get_ed_album().trim().to_string(),
        album_artist: app.get_ed_album_artist().trim().to_string(),
        year: if year.is_empty() {
            0
        } else {
            year.parse::<u16>().ok().filter(|y| (1000..=2999).contains(y)).ok_or("The year should be four digits, like 2007.")?
        },
        track_no: if track.is_empty() {
            0
        } else {
            track.parse::<u16>().ok().filter(|t| (1..=999).contains(t)).ok_or("The track number should be between 1 and 999.")?
        },
    };
    if fields.title.is_empty() {
        return Err("Give the song a title first.".to_string());
    }
    Ok(fields)
}

pub fn save(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    if app.get_ed_saving() {
        return;
    }
    let fields = match read_fields(&app) {
        Ok(f) => f,
        Err(e) => return app.set_ed_error(e.into()),
    };
    let lyrics = app.get_ed_lyrics().to_string();
    let lyrics = (!lyrics.trim().is_empty()).then_some(lyrics);
    let (lib, id, cover) = {
        let c = ctl.borrow();
        let Some(open) = c.editor.open.as_ref() else { return };
        (c.lib.clone(), open.track.id, open.new_cover.as_ref().map(|n| n.bytes.clone()))
    };
    app.set_ed_error(SharedString::new());
    app.set_ed_saving(true);
    std::thread::spawn(move || {
        let covers = settings::data_dir().join("Covers");
        let result = edit::save(&lib, &covers, id, &fields, cover.as_deref(), lyrics.as_deref());
        on_ui_thread(move |ctl| saved(ctl, result));
    });
}

fn saved(ctl: &Ctl, result: Result<Track, String>) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    app.set_ed_saving(false);
    match result {
        Err(e) if app.get_ed_open() => app.set_ed_error(e.into()),
        Err(e) => toast(ctl, &e),
        Ok(track) => {
            if app.get_ed_open() {
                close(ctl);
            }
            let message = match track.flags {
                0 => format!("Saved “{}”", track.title),
                left => format!("Saved “{}”. Still unidentified: {}", track.title, flag::describe(left).to_lowercase()),
            };
            toast(ctl, &message);
            library_edited(ctl, track.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(path: &str, title: &str, artist: &str, flags: u8) -> Track {
        Track {
            path: path.into(),
            title: title.into(),
            artist: artist.into(),
            album: "Unknown Album".into(),
            album_artist: artist.into(),
            flags,
            ..Default::default()
        }
    }

    #[test]
    fn the_search_starts_from_the_file_name_when_the_title_is_only_that() {
        let raw = flag::NO_ARTIST | flag::BAD_TITLE | flag::NO_COVER;
        assert_eq!(first_query(&song(r"C:\Music\04. Daft Punk - Around the World (Official Audio) [320kbps].mp3", "04. Daft Punk - Around the World (Official Audio) [320kbps]", "Unknown Artist", raw)), "Daft Punk - Around the World");
        assert_eq!(first_query(&song(r"C:\Music\Kanye_West_-_Flashing_Lights.mp3", "Kanye_West_-_Flashing_Lights", "Unknown Artist", raw)), "Kanye West - Flashing Lights");
        // A real title with no artist, and a real title with an artist.
        assert_eq!(first_query(&song(r"C:\Music\x.mp3", "Flashing Lights", "Unknown Artist", flag::NO_ARTIST)), "Flashing Lights");
        assert_eq!(first_query(&song(r"C:\Music\x.mp3", "Flashing Lights", "Kanye West", flag::NO_COVER)), "Kanye West - Flashing Lights");
    }

    #[test]
    fn the_form_starts_from_the_tags_without_placeholders() {
        let mut t = song(r"C:\Music\09 Flashing Lights.mp4", "AUDIO_294", "Unknown Artist", flag::NO_ARTIST | flag::BAD_TITLE | flag::NO_COVER);
        t.album_artist = "Unknown Artist".into();
        let f = first_fields(&t);
        assert_eq!((f.title.as_str(), f.artist.as_str(), f.album.as_str(), f.album_artist.as_str()), ("AUDIO 294", "", "", ""));
        let mut t = song(r"C:\Music\x.mp3", "Intro", "Björk", flag::NO_COVER);
        (t.album, t.year, t.track_no) = ("Homogenic".into(), 1997, 1);
        let f = first_fields(&t);
        assert_eq!((f.title.as_str(), f.artist.as_str(), f.album.as_str(), f.year, f.track_no), ("Intro", "Björk", "Homogenic", 1997, 1));
    }

    #[test]
    fn a_match_reads_as_album_year_and_track() {
        let row = Row {
            title: "Flashing Lights".into(),
            artist: "Kanye West feat. Dwele".into(),
            album: "Graduation".into(),
            album_artist: "Kanye West".into(),
            year: 2007,
            track_no: 9,
            disc_no: 1,
            length_ms: Some(237_000),
            release_id: String::new(),
            group_id: String::new(),
            length_match: true,
        };
        assert_eq!(detail(&row), "Graduation · 2007 · Track 9");
        assert_eq!(detail(&Row { album: String::new(), year: 0, track_no: 0, ..row.clone() }), "");
        assert_eq!(detail(&Row { year: 0, ..row }), "Graduation · Track 9");
        assert_eq!((matches_text(1), matches_text(14)), ("1 match".to_string(), "14 matches".to_string()));
    }
}
