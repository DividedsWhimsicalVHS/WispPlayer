//! The controller: connects the player, library, background loader and UI. Everything here runs
//! on the UI thread; worker threads hand their results back through `invoke_from_event_loop`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering::*};
use std::time::Duration;

use slint::{ComponentHandle, Image, Model, ModelRc, Rgb8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel, Weak};

use crate::art::Rgb;
use crate::host::{self, MediaAction, NowPlaying};
use crate::library::{self, Library, Progress, Sort};
use crate::loader::{Done, Job, Loader, NowAssets};
use crate::lyrics::{self, Lyrics};
use crate::models::{AlbumList, QueueList, TrackList, fmt_count, fmt_time};
use crate::player::{self, AudioSettings, Event, Player, Repeat};
use crate::playlists::{Playlist, Playlists};
use crate::settings::{self, Settings};
use crate::themes::{self, Col, ThemeDef};
use crate::{App, ArtistData, DeviceRow, LyricLine, MainWindow, Theme};

mod editor;

const PAGE_SONGS: i32 = 0;
const PAGE_ALBUMS: i32 = 1;
const PAGE_ARTISTS: i32 = 2;
const PAGE_QUEUE: i32 = 3;
const PAGE_SETTINGS: i32 = 4;
const PAGE_DETAIL: i32 = 5;
const PAGE_PLAYLIST: i32 = 6;
const PAGE_FLAGGED: i32 = 7;

const DIALOG_NEW: i32 = 1;
const DIALOG_RENAME: i32 = 2;
const DIALOG_DELETE: i32 = 3;
const DIALOG_SAVE_QUEUE: i32 = 4;

const ACCENTS: [[u8; 3]; 8] = [
    [0xfa, 0x2d, 0x48],
    [0xff, 0x7a, 0x3d],
    [0xf2, 0xb6, 0x32],
    [0x2f, 0xc4, 0x67],
    [0x2f, 0xb5, 0xc8],
    [0x4f, 0x7c, 0xff],
    [0x9b, 0x5c, 0xf0],
    [0xff, 0x5c, 0x93],
];
#[cfg(desktop)]
const SCALES: [f32; 5] = [0.9, 1.0, 1.1, 1.25, 1.5];

struct State {
    ui: Weak<MainWindow>,
    lib: Arc<Library>,
    player: Player,
    settings: Settings,
    loader: Loader,
    progress: Arc<Progress>,

    songs: Rc<TrackList>,
    albums: Rc<AlbumList>,
    queue_page: Rc<QueueList>,
    upnext: Rc<QueueList>,

    query: String,
    sort: Sort,
    /// Artist filter on the albums page: (name, first album, album count).
    artist_filter: Option<(String, u32, u32)>,
    search_gen: Arc<AtomicU32>,
    /// Library revision the visible lists were built from.
    rev: u32,
    total_tracks: usize,

    cur_uid: u32,
    /// The song and play state last reported to the host (the phone's media notification).
    announced: (u32, bool),
    cur_duration_ms: u64,
    lyrics: Option<Lyrics>,
    last_ended: u32,
    backdrop_flip: bool,
    accent_hsv: Option<(f32, f32, f32)>,
    /// Brightness of the playing artwork (0 to 1), for the Album Chameleon theme.
    backdrop_luma: f32,
    /// The monitor's own scale factor; 0 until the native window exists and reports it.
    #[cfg_attr(mobile, allow(dead_code))]
    base_scale: f32,
    /// The scale factor (native x UI-size setting) this controller last put into effect.
    #[cfg_attr(mobile, allow(dead_code))]
    applied_scale: f32,
    /// `WISP_DEBUG` is set: print playback health to stderr.
    debug: bool,
    ticks: u32,

    playlists: Playlists,
    /// The playlists as listed in the sidebar.
    playlist_list: Vec<Playlist>,
    /// The playlist shown on the playlist page.
    open_playlist: Option<Playlist>,
    /// Files the playlist menu (and then perhaps the new-playlist dialog) was opened for.
    pending_add: Vec<String>,
    /// The unidentified songs; filled while their page (7) is open.
    flagged: Rc<TrackList>,
    editor: editor::Editor,

    toast_timer: Timer,
    search_timer: Timer,
    tick_timer: Timer,
    tick_ms: u64,
}

type Ctl = Rc<RefCell<State>>;

thread_local! {
    static CTL: RefCell<Option<Ctl>> = const { RefCell::new(None) };
}

/// Runs `f` on the UI thread with the controller (used by worker-thread results).
fn on_ui_thread(f: impl FnOnce(&Ctl) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || {
        let ctl = CTL.with(|c| c.borrow().clone());
        if let Some(ctl) = ctl {
            f(&ctl);
        }
    });
}

/// A media button was pressed outside the window (see [`host`]). Callable from any thread.
pub fn media_action(action: MediaAction) {
    on_ui_thread(move |ctl| match action {
        MediaAction::Play => {
            if !ctl.borrow().player.is_playing() {
                toggle_play(ctl);
            }
        }
        MediaAction::Pause => ctl.borrow().player.pause(),
        MediaAction::Next => ctl.borrow().player.next(),
        MediaAction::Prev => ctl.borrow().player.prev(),
    });
}

/// Looks for new and changed files, e.g. once the phone's storage permission has been granted.
/// Callable from any thread.
pub fn rescan() {
    on_ui_thread(start_scan);
}

fn to_image(a: &Rgb) -> Image {
    Image::from_rgb8(SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(&a.px, a.w, a.h))
}

fn ui_of(ctl: &Ctl) -> Option<MainWindow> {
    ctl.borrow().ui.upgrade()
}

fn strings_model(items: impl IntoIterator<Item = String>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(items.into_iter().map(SharedString::from).collect::<Vec<_>>()))
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

// ---------------------------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------------------------

/// What the window manager and the Flatpak know the program as: the window's Wayland `app_id` must
/// match the name of its `.desktop` file for the dock to show the right icon.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const APP_ID: &str = "io.github.DividedsWhimsicalVHS.WispPlayer";

/// A message for a failure that happens before there is a window to show it in.
#[cfg(any(windows, target_os = "macos"))]
fn show_error(message: &str) {
    let _ = rfd::MessageDialog::new().set_title("Wisp").set_description(message).show();
}

#[cfg(not(any(windows, target_os = "macos")))]
fn show_error(message: &str) {
    eprintln!("wisp: {message}");
}

pub fn run() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let _ = slint::set_xdg_app_id(APP_ID);
    let dir = settings::data_dir();
    let mut settings = Settings::load(&dir);
    let lib = match Library::open(&dir.join("library.redb")) {
        Ok(l) => Arc::new(l),
        Err(e) => {
            show_error(&e);
            return Err(e);
        }
    };
    if settings.get("folders").is_none() {
        if let Some(music) = settings::default_music_dir() {
            settings.set_folders(&[music]);
        }
    }

    let audio = AudioSettings {
        device: Some(settings.string("device", "")).filter(|d| !d.is_empty()),
        volume: settings.f32("volume", 0.8).clamp(0.0, 1.0),
        muted: settings.bool("muted", false),
        normalization: settings.i32("normalization", 0).clamp(0, 2) as u8,
        preamp_db: settings.f32("preamp", 0.0).clamp(-12.0, 12.0),
        prevent_clipping: settings.bool("prevent_clip", true),
    };
    let resolver_lib = lib.clone();
    let player = Player::start(Arc::new(move |id| resolver_lib.path_of(id)), &audio);
    {
        let mut q = player.queue();
        q.set_repeat(Repeat::from_index(settings.i32("repeat", 0)));
    }

    let ui = MainWindow::new().map_err(|e| e.to_string())?;
    ui.global::<Theme>().set_touch_screen(cfg!(mobile));
    let loader = Loader::spawn(|done| on_ui_thread(move |ctl| handle_done(ctl, done)));
    let playlists = Playlists::new(dir.join("Playlists"));
    let songs = TrackList::new(lib.clone());
    let flagged = TrackList::new(lib.clone());
    let albums = AlbumList::new(lib.clone());
    let queue_page = QueueList::new(lib.clone(), player_queue(&player));
    let upnext = QueueList::new(lib.clone(), player_queue(&player));

    let state = State {
        ui: ui.as_weak(),
        lib: lib.clone(),
        player,
        settings,
        loader,
        progress: Arc::new(Progress::default()),
        songs: songs.clone(),
        albums: albums.clone(),
        queue_page: queue_page.clone(),
        upnext: upnext.clone(),
        query: String::new(),
        sort: Sort::Title,
        artist_filter: None,
        search_gen: Arc::new(AtomicU32::new(0)),
        rev: u32::MAX,
        total_tracks: 0,
        cur_uid: 0,
        announced: (0, false),
        cur_duration_ms: 0,
        lyrics: None,
        last_ended: 0,
        backdrop_flip: false,
        accent_hsv: None,
        backdrop_luma: 0.0,
        base_scale: 0.0,
        applied_scale: 0.0,
        debug: std::env::var_os("WISP_DEBUG").is_some(),
        ticks: 0,
        playlists,
        playlist_list: Vec::new(),
        open_playlist: None,
        pending_add: Vec::new(),
        flagged: flagged.clone(),
        editor: editor::Editor::new(),
        toast_timer: Timer::default(),
        search_timer: Timer::default(),
        tick_timer: Timer::default(),
        tick_ms: 0,
    };
    let ctl: Ctl = Rc::new(RefCell::new(state));
    CTL.with(|c| *c.borrow_mut() = Some(ctl.clone()));

    {
        let app = ui.global::<App>();
        app.set_tracks(songs.model());
        app.set_albums(albums.model());
        app.set_queue(queue_page.model());
        app.set_upnext(upnext.model());
        app.set_flagged(flagged.model());
        app.set_theme_groups(themes::groups());
        let weak = Rc::downgrade(&ctl);
        albums.on_want_art(move |index, rep_track| {
            if let Some(ctl) = weak.upgrade() {
                let c = ctl.borrow();
                if let Some(t) = c.lib.track(rep_track) {
                    c.loader.request(Job::Thumb { index, path: PathBuf::from(&t.path), cover: t.cover });
                }
            }
        });
    }
    wire_callbacks(&ui, &ctl);
    apply_all_settings(&ctl);

    // Window: restore size, wire close. (A phone's window is the screen.)
    #[cfg(desktop)]
    {
        // Logical size at the monitor's native scale; the window is fitted to the screen once it exists.
        let c = ctl.borrow();
        let (w, h) = (c.settings.f32("window_w", 1180.0), c.settings.f32("window_h", 760.0));
        ui.window().set_size(slint::LogicalSize::new(w.clamp(360.0, 6000.0), h.clamp(480.0, 4000.0)));
    }
    let close_ctl = ctl.clone();
    let close_ui = ui.as_weak();
    ui.window().on_close_requested(move || {
        if let Some(ui) = close_ui.upgrade() {
            save_on_exit(&close_ctl, &ui);
        }
        slint::CloseRequestResponse::HideWindow
    });

    ui.show().map_err(|e| e.to_string())?;
    // The native window is only created once the event loop runs; scale and placement wait for it.
    settle_window(&ctl, 0);

    load_playlists(&ctl);
    refresh_view(&ctl);
    start_tick(&ctl);
    {
        let scan = {
            let c = ctl.borrow();
            c.settings.bool("scan_on_start", true) || c.lib.track_count() == 0
        };
        if scan {
            start_scan(&ctl);
        }
    }
    load_devices(&ctl);

    slint::run_event_loop().map_err(|e| e.to_string())?;
    let _ = ui.hide();
    {
        let c = ctl.borrow();
        c.progress.cancel.store(true, Relaxed);
    }
    CTL.with(|c| *c.borrow_mut() = None);
    Ok(())
}

fn player_queue(player: &Player) -> Arc<std::sync::Mutex<player::queue::Queue>> {
    player.queue_arc()
}

fn save_on_exit(ctl: &Ctl, ui: &MainWindow) {
    let mut c = ctl.borrow_mut();
    #[cfg(desktop)]
    {
        let size = ui.window().size();
        // Native logical size, so the UI-size setting does not change the window between runs.
        let scale = if c.base_scale > 0.0 { c.base_scale } else { ui.window().scale_factor().max(0.1) };
        c.settings.set("window_w", (size.width as f32 / scale).round());
        c.settings.set("window_h", (size.height as f32 / scale).round());
    }
    let vol = ui.global::<App>().get_volume();
    c.settings.set("volume", vol);
    c.settings.save();
}

// ---------------------------------------------------------------------------------------------
// Callbacks
// ---------------------------------------------------------------------------------------------

fn wire_callbacks(ui: &MainWindow, ctl: &Ctl) {
    let app = ui.global::<App>();

    // Registers a callback that owns a clone of the controller; the body sees it as `ctl: &Ctl`.
    macro_rules! on {
        ($app:ident, $ctl:ident, $setter:ident, |$($a:ident),*| $body:expr) => {{
            let owned = $ctl.clone();
            $app.$setter(move |$($a),*| {
                #[allow(unused_variables)]
                let $ctl = &owned;
                $body
            });
        }};
    }

    // transport
    on!(app, ctl, on_toggle_play, | | toggle_play(ctl));
    on!(app, ctl, on_next, | | ctl.borrow().player.next());
    on!(app, ctl, on_prev, | | ctl.borrow().player.prev());
    on!(app, ctl, on_seek, |f| seek_frac(ctl, f));
    on!(app, ctl, on_seek_by, |s| seek_by(ctl, s));
    on!(app, ctl, on_volume_changed, |v| set_volume(ctl, v, false));
    on!(app, ctl, on_volume_committed, |v| set_volume(ctl, v, true));
    on!(app, ctl, on_volume_by, |d| {
        let v = ui_of(ctl).map_or(0.8, |u| u.global::<App>().get_volume());
        set_volume(ctl, (v + d).clamp(0.0, 1.0), true);
    });
    on!(app, ctl, on_toggle_mute, | | toggle_mute(ctl));
    on!(app, ctl, on_toggle_shuffle, | | toggle_shuffle(ctl));
    on!(app, ctl, on_cycle_repeat, | | cycle_repeat(ctl));

    // navigation and lists
    on!(app, ctl, on_show_page, |p| show_page(ctl, p));
    on!(app, ctl, on_leave, | | host::leave());
    on!(app, ctl, on_search, |t| search_changed(ctl, t.to_string()));
    on!(app, ctl, on_sort_changed, |i| {
        {
            let mut c = ctl.borrow_mut();
            c.sort = Sort::from_index(i);
            c.settings.set("sort", i);
        }
        if let Some(ui) = ui_of(ctl) {
            ui.global::<App>().set_sort_index(i);
        }
        refresh_view(ctl);
    });
    on!(app, ctl, on_open_album, |i| open_album(ctl, i as u32));
    on!(app, ctl, on_play_album, |i, shuffle| play_album(ctl, i as u32, shuffle));
    on!(app, ctl, on_open_artist, |i| open_artist(ctl, i as usize));
    on!(app, ctl, on_clear_filter, | | {
        ctl.borrow_mut().artist_filter = None;
        refresh_view(ctl);
    });

    // playing from lists
    on!(app, ctl, on_play_row, |i| play_row(ctl, i as usize));
    on!(app, ctl, on_play_next_row, |i| enqueue_row(ctl, i as usize, true));
    on!(app, ctl, on_enqueue_row, |i| enqueue_row(ctl, i as usize, false));
    on!(app, ctl, on_play_all, |shuffle| play_all(ctl, shuffle));
    on!(app, ctl, on_queue_play, |i| {
        let uid = ctl.borrow().player.queue().at(i as usize).map(|it| it.uid);
        if let Some(uid) = uid {
            let c = ctl.borrow();
            c.player.play(uid);
            drop(c);
            set_playing_now(ctl);
        }
    });
    on!(app, ctl, on_queue_remove, |i| queue_remove(ctl, i as usize));
    on!(app, ctl, on_queue_clear, | | queue_clear(ctl));
    on!(app, ctl, on_lyrics_seek, |ms| ctl.borrow().player.seek_ms(ms.max(0) as u64));

    // settings
    #[cfg(desktop)]
    on!(app, ctl, on_add_folder, | | add_folder(ctl));
    on!(app, ctl, on_remove_folder, |i| remove_folder(ctl, i as usize));
    on!(app, ctl, on_rescan, | | start_scan(ctl));
    on!(app, ctl, on_set_theme_id, |id| {
        ctl.borrow_mut().settings.set("theme_id", themes::find(&id).id.to_string());
        apply_all_settings(ctl);
    });
    on!(app, ctl, on_set_accent, |i| {
        ctl.borrow_mut().settings.set("accent", i);
        apply_all_settings(ctl);
    });
    on!(app, ctl, on_set_accent_custom, |s| {
        if parse_hex(&s).is_some() {
            let mut c = ctl.borrow_mut();
            c.settings.set("accent_custom", s.trim().to_string());
            c.settings.set("accent", 9);
            drop(c);
            apply_all_settings(ctl);
        }
    });
    on!(app, ctl, on_set_density, |i| {
        ctl.borrow_mut().settings.set("density", i);
        apply_all_settings(ctl);
    });
    on!(app, ctl, on_set_scale, |i| {
        ctl.borrow_mut().settings.set("scale", i);
        apply_scale(ctl);
        apply_all_settings(ctl);
    });
    on!(app, ctl, on_set_backdrop, |on| {
        ctl.borrow_mut().settings.set("backdrop", on);
        apply_all_settings(ctl);
    });
    on!(app, ctl, on_set_normalization, |i| {
        ctl.borrow_mut().settings.set("normalization", i);
        apply_audio_settings(ctl);
        apply_all_settings(ctl);
    });
    on!(app, ctl, on_set_preamp, |db| {
        ctl.borrow_mut().settings.set("preamp", (db * 2.0).round() / 2.0);
        apply_audio_settings(ctl);
        apply_all_settings(ctl);
    });
    on!(app, ctl, on_set_prevent_clip, |on| {
        ctl.borrow_mut().settings.set("prevent_clip", on);
        apply_audio_settings(ctl);
        apply_all_settings(ctl);
    });
    on!(app, ctl, on_set_device, |id| {
        {
            let mut c = ctl.borrow_mut();
            c.settings.set("device", id.to_string());
            c.player.set_device(Some(id.to_string()).filter(|d| !d.is_empty()));
        }
        apply_all_settings(ctl);
    });
    on!(app, ctl, on_set_scan_on_start, |on| {
        ctl.borrow_mut().settings.set("scan_on_start", on);
        apply_all_settings(ctl);
    });
    on!(app, ctl, on_open_playlist, |i| open_playlist(ctl, i.max(0) as usize));
    on!(app, ctl, on_playlist_menu, |row, x, y| playlist_menu(ctl, row.max(0) as usize, x, y));
    on!(app, ctl, on_add_to_playlist, |i| add_to_playlist(ctl, i));
    on!(app, ctl, on_playlist_remove_row, |row| playlist_remove_row(ctl, row.max(0) as usize));
    on!(app, ctl, on_open_dialog, |mode| {
        ctl.borrow_mut().pending_add.clear();
        open_dialog(ctl, mode);
    });
    on!(app, ctl, on_dialog_accept, |text| dialog_accept(ctl, text.to_string()));
    on!(app, ctl, on_set_mb_contact, |contact| {
        let contact = contact.trim().to_string();
        if contact != ctl.borrow().settings.string("mb_contact", "") {
            ctl.borrow_mut().settings.set("mb_contact", contact);
            apply_all_settings(ctl);
        }
    });
    on!(app, ctl, on_open_flagged, |row| editor::open(ctl, row.max(0) as usize));
    on!(app, ctl, on_ed_search, |query| editor::search(ctl, &query));
    on!(app, ctl, on_ed_pick, |i| editor::pick(ctl, i.max(0) as usize));
    on!(app, ctl, on_ed_lyrics_mode_picked, |mode| editor::lyrics_mode(ctl, mode));
    on!(app, ctl, on_ed_choose_cover, | | editor::choose_cover(ctl));
    on!(app, ctl, on_ed_save, | | editor::save(ctl));
    on!(app, ctl, on_ed_close, | | editor::close(ctl));
    on!(app, ctl, on_restart_app, | | {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).spawn();
        }
        let _ = slint::quit_event_loop();
    });
}

// ---------------------------------------------------------------------------------------------
// Settings -> UI and engine
// ---------------------------------------------------------------------------------------------

/// The chosen theme (settings written before named themes existed map to Wisp Dark or Light).
fn theme_id(s: &Settings) -> String {
    let id = s.string("theme_id", "");
    if id.is_empty() { themes::legacy_id(s.i32("theme", 0)).to_string() } else { id }
}

/// The theme in effect right now: Album Chameleon is derived from the playing artwork.
fn current_theme(c: &State) -> ThemeDef {
    let def = themes::find(&theme_id(&c.settings));
    if def.dynamic { themes::chameleon(c.accent_hsv, c.backdrop_luma) } else { def.clone() }
}

/// The accent and second accent in effect: the user's choice, else the theme's own (or the
/// artwork's, for themes that follow it).
fn accents(c: &State, def: &ThemeDef) -> (Col, Col) {
    let choice = c.settings.i32("accent", 0);
    let picked = match choice {
        1..=8 => Some(ACCENTS[(choice - 1) as usize]),
        9 => Some(parse_hex(&c.settings.string("accent_custom", "")).unwrap_or(ACCENTS[0])),
        _ => None,
    };
    if let Some([r, g, b]) = picked {
        let col = Col { r, g, b, a: 255 };
        return (col, col);
    }
    match (def.follows_art, c.accent_hsv) {
        (true, Some(hsv)) => {
            let col = themes::artwork_accent(def, hsv);
            (col, col)
        }
        _ => (def.accent, def.accent2),
    }
}

fn apply_theme(c: &State, ui: &MainWindow) {
    let def = current_theme(c);
    let (accent, accent2) = accents(c, &def);
    themes::write(&ui.global::<Theme>(), &def, accent, accent2, c.settings.bool("backdrop", true));
    let app = ui.global::<App>();
    app.set_theme_id(def.id.into());
    app.set_theme_name(def.name.into());
    app.set_backdrop_locked(!def.backdrop);
    app.set_accent_hint(if def.follows_art { "Auto follows the album artwork" } else { "Auto uses the theme's own accent" }.into());
}

/// Pushes every persisted setting into the UI state and saves.
fn apply_all_settings(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let mut c = ctl.borrow_mut();
    let app = ui.global::<App>();
    let theme = ui.global::<Theme>();

    theme.set_density(c.settings.i32("density", 1).clamp(0, 2));
    apply_theme(&c, &ui);

    app.set_accent_index(c.settings.i32("accent", 0));
    app.set_accent_custom(c.settings.string("accent_custom", "").into());
    app.set_density_index(c.settings.i32("density", 1));
    app.set_scale_index(c.settings.i32("scale", 1).clamp(0, 4));
    app.set_backdrop_setting(c.settings.bool("backdrop", true));
    app.set_norm_index(c.settings.i32("normalization", 0));
    app.set_preamp(c.settings.f32("preamp", 0.0));
    app.set_prevent_clip(c.settings.bool("prevent_clip", true));
    app.set_device_id(c.settings.string("device", "").into());
    app.set_scan_on_start(c.settings.bool("scan_on_start", true));
    app.set_mb_contact(c.settings.string("mb_contact", "").into());
    app.set_sort_index(c.settings.i32("sort", 0));
    app.set_volume(c.settings.f32("volume", 0.8));
    app.set_muted(c.settings.bool("muted", false));
    c.sort = Sort::from_index(c.settings.i32("sort", 0));

    let folders = c.settings.folders();
    app.set_folders(strings_model(folders.iter().map(|p| p.to_string_lossy().into_owned())));
    app.set_library_info(
        format!("{} songs · {}", fmt_count(c.total_tracks), if folders.len() == 1 { "1 folder".to_string() } else { format!("{} folders", folders.len()) })
            .into(),
    );
    {
        let q = c.player.queue();
        app.set_shuffle(q.shuffle());
        app.set_repeat(q.repeat().index());
    }
    c.settings.save();
}

fn apply_audio_settings(ctl: &Ctl) {
    let c = ctl.borrow();
    c.player.set_normalization(
        c.settings.i32("normalization", 0).clamp(0, 2) as u8,
        c.settings.f32("preamp", 0.0).clamp(-12.0, 12.0),
        c.settings.bool("prevent_clip", true),
    );
}

// A phone's window is the screen: there is no UI-size setting to apply and nothing to fit.
#[cfg(mobile)]
fn apply_scale(_: &Ctl) {}
#[cfg(mobile)]
fn track_scale(_: &Ctl, _: &MainWindow) {}
#[cfg(mobile)]
fn settle_window(_: &Ctl, _: u32) {}

/// UI size: native scale times the chosen factor. Applied by telling the window its scale changed;
/// the window's pixel size stays, only how many logical pixels it holds changes.
#[cfg(desktop)]
fn apply_scale(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let target = {
        let c = ctl.borrow();
        if c.base_scale <= 0.0 {
            return; // native scale not known yet; settle_window() applies it
        }
        c.base_scale * SCALES[c.settings.i32("scale", 1).clamp(0, 4) as usize]
    };
    let window = ui.window();
    if (window.scale_factor() - target).abs() > 1e-3 {
        window.dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: target });
        // A scale change alone would keep the logical size and so grow the drawing area past the real
        // window; announce the (unchanged) pixel size in the new logical units.
        let px = window.size();
        let logical = slint::LogicalSize::new(px.width as f32 / target, px.height as f32 / target);
        window.dispatch_event(slint::platform::WindowEvent::Resized { size: logical });
    }
    ctl.borrow_mut().applied_scale = target;
}

/// The OS reports a scale factor of its own on first show and whenever the window moves to a
/// monitor with another DPI. Any value other than the one applied here is the new native scale,
/// so the UI-size setting is put on top of it again.
#[cfg(desktop)]
fn track_scale(ctl: &Ctl, ui: &MainWindow) {
    let seen = ui.window().scale_factor();
    if (seen - ctl.borrow().applied_scale).abs() > 1e-3 {
        ctl.borrow_mut().base_scale = seen;
        apply_scale(ctl);
    }
}

/// Runs once the native window exists (it is created lazily when the event loop starts): learns the
/// monitor's scale, applies the UI-size setting and fits the window to the screen.
#[cfg(desktop)]
fn settle_window(ctl: &Ctl, attempt: u32) {
    use slint::winit_030::WinitWindowAccessor;
    let Some(ui) = ui_of(ctl) else { return };
    if !ui.window().has_winit_window() {
        if attempt < 300 {
            let weak = Rc::downgrade(ctl);
            Timer::single_shot(Duration::from_millis(10), move || {
                if let Some(ctl) = weak.upgrade() {
                    settle_window(&ctl, attempt + 1);
                }
            });
        }
        return;
    }
    track_scale(ctl, &ui);
    fit_to_monitor(&ui);
}

/// Keeps the window on the screen it opened on. A default or saved size can be larger than a small
/// or heavily scaled display (1920x1080 at 150% is only 1280x720 logical), which would push the
/// player bar off-screen. Shrinks to fit and centres; the margins leave room for a taskbar or dock
/// on any edge.
#[cfg(desktop)]
fn fit_to_monitor(ui: &MainWindow) {
    use slint::winit_030::WinitWindowAccessor;
    let Some(Some((origin, screen, inner, outer))) = ui.window().with_winit_window(|w| {
        let monitor = w.current_monitor().or_else(|| w.primary_monitor())?;
        Some((monitor.position(), monitor.size(), w.inner_size(), w.outer_size()))
    }) else {
        return;
    };
    let frame_w = outer.width.saturating_sub(inner.width);
    let frame_h = outer.height.saturating_sub(inner.height);
    let width = inner.width.min(((screen.width as f32 * 0.94) as u32).saturating_sub(frame_w)).max(1);
    let height = inner.height.min(((screen.height as f32 * 0.88) as u32).saturating_sub(frame_h)).max(1);
    if (width, height) != (inner.width, inner.height) {
        ui.window().set_size(slint::PhysicalSize::new(width, height));
    }
    let x = origin.x + (screen.width as i32 - (width + frame_w) as i32).max(0) / 2;
    let y = origin.y + (screen.height as i32 - (height + frame_h) as i32).max(0) / 2;
    ui.window().set_position(slint::PhysicalPosition::new(x, y));
}

fn load_devices(ctl: &Ctl) {
    let _ = ctl;
    std::thread::spawn(|| {
        let devices = player::list_devices();
        on_ui_thread(move |ctl| {
            let Some(ui) = ui_of(ctl) else { return };
            let mut rows = vec![DeviceRow { id: "".into(), name: "System default".into() }];
            rows.extend(devices.into_iter().map(|d| DeviceRow { id: d.id.into(), name: d.name.into() }));
            ui.global::<App>().set_devices(ModelRc::new(VecModel::from(rows)));
        });
    });
}

// ---------------------------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------------------------

fn show_page(ctl: &Ctl, page: i32) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let leaving = app.get_page();
    app.set_page(page);
    app.set_np_open(false);
    if leaving == PAGE_ALBUMS && page != PAGE_ALBUMS && page != PAGE_DETAIL {
        ctl.borrow().albums.release();
    }
    if leaving == PAGE_FLAGGED && page != PAGE_FLAGGED {
        ctl.borrow().flagged.set(Rc::new(Vec::new()), false);
    }
    if page != PAGE_ALBUMS && page != PAGE_DETAIL {
        // The artist filter belongs to the albums page.
        ctl.borrow_mut().artist_filter = None;
    }
    if page == PAGE_SETTINGS {
        load_devices(ctl);
    }
    refresh_view(ctl);
}

fn search_changed(ctl: &Ctl, text: String) {
    let Some(ui) = ui_of(ctl) else { return };
    ui.global::<App>().set_search_text(text.clone().into());
    ctl.borrow_mut().query = text;
    // Debounce: wait for a pause in typing before scanning the library.
    let weak = Rc::downgrade(ctl);
    ctl.borrow().search_timer.start(TimerMode::SingleShot, Duration::from_millis(140), move || {
        if let Some(ctl) = weak.upgrade() {
            refresh_view(&ctl);
        }
    });
}

/// Rebuilds the list of the visible page from the current search / sort / filter.
fn refresh_view(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let page = app.get_page();
    {
        let mut c = ctl.borrow_mut();
        c.rev = c.progress.revision.load(Relaxed);
        c.total_tracks = c.lib.track_count();
        let total = c.total_tracks;
        app.set_albums_subtitle(format!("{} albums", fmt_count(c.lib.album_count())).into());
        if c.query.is_empty() {
            app.set_songs_subtitle(format!("{} songs", fmt_count(total)).into());
        }
        let folders = c.settings.folders().len();
        app.set_library_info(format!("{} songs · {}", fmt_count(total), if folders == 1 { "1 folder".to_string() } else { format!("{folders} folders") }).into());
    }
    match page {
        PAGE_SONGS => run_search(ctl),
        PAGE_ALBUMS => load_albums(ctl),
        PAGE_ARTISTS => load_artists(ctl),
        PAGE_QUEUE => {
            let c = ctl.borrow();
            c.queue_page.changed();
            let n = c.player.queue().len();
            app.set_queue_subtitle(format!("{} songs", fmt_count(n)).into());
        }
        PAGE_PLAYLIST => load_open_playlist(ctl),
        _ => {}
    }
    refresh_flagged(ctl);
}

/// Songs for the visible query, computed off the UI thread; a newer request cancels an older one.
fn run_search(ctl: &Ctl) {
    let (lib, query, sort, counter, mine) = {
        let c = ctl.borrow();
        let mine = c.search_gen.fetch_add(1, AcqRel) + 1;
        (c.lib.clone(), c.query.clone(), c.sort, c.search_gen.clone(), mine)
    };
    std::thread::spawn(move || {
        let cancel = || counter.load(Acquire) != mine;
        let ids = lib.search_ids(&query, sort, &cancel);
        if cancel() {
            return;
        }
        on_ui_thread(move |ctl| {
            if ctl.borrow().search_gen.load(Acquire) != mine {
                return;
            }
            let Some(ui) = ui_of(ctl) else { return };
            let c = ctl.borrow();
            let app = ui.global::<App>();
            if !c.query.is_empty() {
                app.set_songs_subtitle(format!("{} results", fmt_count(ids.len())).into());
            }
            // Don't clobber an album's track list that is on screen.
            if app.get_page() == PAGE_SONGS {
                c.songs.set(Rc::new(ids), false);
            }
        });
    });
}

fn load_albums(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let (lib, query, filter) = {
        let c = ctl.borrow();
        (c.lib.clone(), c.query.clone(), c.artist_filter.clone())
    };
    app.set_filter_label(filter.as_ref().map_or_else(SharedString::new, |(n, _, _)| format!("Artist: {n}").into()));
    let mut ids: Vec<u32> = match &filter {
        Some((_, first, count)) => (*first..*first + *count).collect(),
        None => (0..lib.album_count() as u32).collect(),
    };
    if !query.is_empty() {
        let hits = lib.search_albums(&query, &|| false);
        ids.retain(|i| hits.binary_search(i).is_ok());
    }
    let c = ctl.borrow();
    app.set_albums_subtitle(
        if filter.is_some() || !query.is_empty() { format!("{} albums", fmt_count(ids.len())) } else { format!("{} albums", fmt_count(lib.album_count())) }.into(),
    );
    c.albums.set(Rc::new(ids));
}

fn load_artists(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let (artists, query) = {
        let c = ctl.borrow();
        (c.lib.artists(), c.query.clone())
    };
    let words: Vec<String> = library::fold(&query).split_whitespace().map(str::to_string).collect();
    let rows: Vec<ArtistData> = artists
        .iter()
        .filter(|a| {
            let name = library::fold(&a.name);
            words.iter().all(|w| name.contains(w.as_str()))
        })
        .map(|a| ArtistData {
            index: a.first_album as i32,
            name: a.name.as_str().into(),
            albums: if a.albums == 1 { "1 album".to_string() } else { format!("{} albums", a.albums) }.into(),
            initial: a.name.chars().next().map_or_else(|| "?".to_string(), |c| c.to_uppercase().to_string()).into(),
        })
        .collect();
    let app = ui.global::<App>();
    app.set_artists_subtitle(format!("{} artists", fmt_count(rows.len())).into());
    // The row's `index` is the artist's first album; `count` comes back through `open_artist`.
    app.set_artists(ModelRc::new(VecModel::from(rows)));
}

fn open_artist(ctl: &Ctl, row: usize) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let Some(data) = app.get_artists().row_data(row) else { return };
    let first = data.index as u32;
    let artists = ctl.borrow().lib.artists();
    let Some(a) = artists.iter().find(|a| a.first_album == first) else { return };
    {
        let mut c = ctl.borrow_mut();
        c.artist_filter = Some((a.name.clone(), a.first_album, a.albums));
    }
    app.set_page(PAGE_ALBUMS);
    refresh_view(ctl);
}

fn open_album(ctl: &Ctl, index: u32) {
    let Some(ui) = ui_of(ctl) else { return };
    let (lib, ids) = {
        let c = ctl.borrow();
        (c.lib.clone(), c.lib.album_tracks(index))
    };
    let Some(album) = lib.album(index) else { return };
    let total_ms: u64 = ids.iter().filter_map(|&i| lib.track(i)).map(|t| t.duration_ms as u64).sum();
    let app = ui.global::<App>();
    app.set_detail_title(album.name.as_str().into());
    app.set_detail_artist(album.artist.as_str().into());
    let mut meta = Vec::new();
    if album.year > 0 {
        meta.push(album.year.to_string());
    }
    meta.push(if ids.len() == 1 { "1 song".to_string() } else { format!("{} songs", ids.len()) });
    if total_ms > 0 {
        meta.push(format!("{} min", (total_ms / 60_000).max(1)));
    }
    app.set_detail_meta(meta.join(" · ").into());
    app.set_detail_seed(index as i32);
    app.set_detail_has_art(false);
    app.set_detail_art(Image::default());
    {
        let c = ctl.borrow();
        c.songs.set(Rc::new(ids), true);
        if let Some(t) = lib.track(album.rep_track) {
            c.loader.request(Job::Detail { index, path: PathBuf::from(&t.path), cover: t.cover });
        }
    }
    app.set_page(PAGE_DETAIL);
}

// ---------------------------------------------------------------------------------------------
// Playback actions
// ---------------------------------------------------------------------------------------------

/// Reflects "playing" in the UI right away (the position timer confirms it).
fn set_playing_now(ctl: &Ctl) {
    if let Some(ui) = ui_of(ctl) {
        ui.global::<App>().set_playing(true);
    }
}

fn start_queue(ctl: &Ctl, ids: &[u32], start: usize) {
    let uid = {
        let c = ctl.borrow();
        let uid = c.player.queue().set(ids, start);
        c.player.queue_changed();
        c.queue_page.changed();
        uid
    };
    if let Some(uid) = uid {
        ctl.borrow().player.play(uid);
        set_playing_now(ctl);
    }
}

fn play_row(ctl: &Ctl, row: usize) {
    let ids = ctl.borrow().songs.ids();
    if row < ids.len() {
        start_queue(ctl, &ids, row);
    }
}

fn play_all(ctl: &Ctl, shuffle: bool) {
    let ids = ctl.borrow().songs.ids();
    if ids.is_empty() {
        return;
    }
    if shuffle {
        ctl.borrow().player.queue().set_shuffle(true, None);
        if let Some(ui) = ui_of(ctl) {
            ui.global::<App>().set_shuffle(true);
        }
    }
    let start = if shuffle { fastrand::usize(..ids.len()) } else { 0 };
    start_queue(ctl, &ids, start);
}

fn play_album(ctl: &Ctl, index: u32, shuffle: bool) {
    let ids = ctl.borrow().lib.album_tracks(index);
    if ids.is_empty() {
        return;
    }
    if shuffle {
        ctl.borrow().player.queue().set_shuffle(true, None);
    }
    let start = if shuffle { fastrand::usize(..ids.len()) } else { 0 };
    start_queue(ctl, &ids, start);
}

fn enqueue_row(ctl: &Ctl, row: usize, next: bool) {
    let Some(id) = ctl.borrow().songs.id_at(row) else { return };
    {
        let c = ctl.borrow();
        let cur = c.player.now().uid;
        let mut q = c.player.queue();
        if next {
            q.play_next(Some(cur).filter(|u| *u != 0), id);
        } else {
            q.append(id);
        }
        drop(q);
        c.player.queue_changed();
        c.queue_page.changed();
        c.upnext.changed();
    }
    toast(ctl, if next { "Playing next" } else { "Added to queue" });
    refresh_queue_subtitle(ctl);
}

fn queue_remove(ctl: &Ctl, row: usize) {
    {
        let c = ctl.borrow();
        let uid = c.player.queue().at(row).map(|i| i.uid);
        if let Some(uid) = uid {
            c.player.queue().remove(uid);
            c.player.queue_changed();
        }
        c.queue_page.changed();
        c.upnext.changed();
    }
    refresh_queue_subtitle(ctl);
}

fn queue_clear(ctl: &Ctl) {
    {
        let c = ctl.borrow();
        let keep = Some(c.player.now().uid).filter(|u| *u != 0);
        c.player.queue().clear_except(keep);
        c.player.queue_changed();
        c.queue_page.changed();
        c.upnext.changed();
    }
    refresh_queue_subtitle(ctl);
}

fn refresh_queue_subtitle(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let n = ctl.borrow().player.queue().len();
    ui.global::<App>().set_queue_subtitle(format!("{} songs", fmt_count(n)).into());
}

fn toggle_play(ctl: &Ctl) {
    let has_current = ctl.borrow().cur_uid != 0;
    if has_current {
        ctl.borrow().player.toggle();
        return;
    }
    // Nothing loaded yet: play the queue, or start the library.
    let first = ctl.borrow().player.queue().at(0).map(|i| i.uid);
    if let Some(uid) = first {
        ctl.borrow().player.play(uid);
        set_playing_now(ctl);
    } else {
        let ids = ctl.borrow().songs.ids();
        if !ids.is_empty() {
            start_queue(ctl, &ids, 0);
        }
    }
}

fn seek_frac(ctl: &Ctl, f: f32) {
    let c = ctl.borrow();
    if c.cur_duration_ms > 0 {
        c.player.seek_ms((f.clamp(0.0, 1.0) as f64 * c.cur_duration_ms as f64) as u64);
    }
}

fn seek_by(ctl: &Ctl, seconds: i32) {
    let c = ctl.borrow();
    let now = c.player.now();
    if now.uid == 0 {
        return;
    }
    let target = (now.pos_ms as i64 + seconds as i64 * 1000).clamp(0, c.cur_duration_ms.saturating_sub(250) as i64);
    c.player.seek_ms(target as u64);
}

fn set_volume(ctl: &Ctl, v: f32, commit: bool) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let mut c = ctl.borrow_mut();
    c.player.set_volume(v);
    app.set_volume(v);
    if v > 0.0 && app.get_muted() {
        app.set_muted(false);
        c.player.set_muted(false);
        c.settings.set("muted", false);
    }
    if commit {
        c.settings.set("volume", v);
        c.settings.save();
    }
}

fn toggle_mute(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let mut c = ctl.borrow_mut();
    let muted = !app.get_muted();
    app.set_muted(muted);
    c.player.set_muted(muted);
    c.settings.set("muted", muted);
    c.settings.save();
}

fn toggle_shuffle(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let c = ctl.borrow();
    let on = !c.player.queue().shuffle();
    let cur = Some(c.player.now().uid).filter(|u| *u != 0);
    c.player.queue().set_shuffle(on, cur);
    c.player.queue_changed();
    app.set_shuffle(on);
    c.queue_page.changed();
    let start = cur.and_then(|u| c.player.queue().index_of(u)).unwrap_or(0);
    c.upnext.set_start(start);
    app.set_upnext_offset(start as i32);
    c.upnext.changed();
}

fn cycle_repeat(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let mut c = ctl.borrow_mut();
    let next = (c.player.queue().repeat().index() + 1) % 3;
    c.player.queue().set_repeat(Repeat::from_index(next));
    c.player.queue_changed();
    c.settings.set("repeat", next);
    c.settings.save();
    ui.global::<App>().set_repeat(next);
}

// ---------------------------------------------------------------------------------------------
// Library scanning
// ---------------------------------------------------------------------------------------------

fn start_scan(ctl: &Ctl) {
    let (lib, folders, progress) = {
        let c = ctl.borrow();
        (c.lib.clone(), c.settings.folders(), c.progress.clone())
    };
    let started = library::spawn_scan(
        lib,
        folders,
        progress,
        Box::new(|| on_ui_thread(|ctl| scan_progress(ctl))),
    );
    if started {
        if let Some(ui) = ui_of(ctl) {
            ui.global::<App>().set_scanning(true);
            ui.global::<App>().set_scan_text("Scanning…".into());
        }
    }
}

/// Called (throttled) while a scan runs and once when it ends.
fn scan_progress(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let (running, files, rev, seen_rev, error) = {
        let c = ctl.borrow();
        (
            c.progress.running.load(Acquire),
            c.progress.files.load(Relaxed),
            c.progress.revision.load(Relaxed),
            c.rev,
            c.progress.error.lock().ok().and_then(|e| e.clone()),
        )
    };
    app.set_scanning(running);
    if running {
        app.set_scan_text(format!("Scanning… {} files", fmt_count(files as usize)).into());
    }
    if rev != seen_rev {
        let c = ctl.borrow();
        c.songs.invalidate();
        c.albums.release(); // album numbers change when albums are regrouped
        drop(c);
        refresh_view(ctl);
    }
    if !running {
        if ctl.borrow().debug {
            eprintln!("[wisp] scan finished: {} songs", ctl.borrow().lib.track_count());
        }
        if let Some(e) = error {
            toast(ctl, &format!("Library scan problem: {e}"));
        }
    }
}

#[cfg(desktop)]
fn add_folder(ctl: &Ctl) {
    let _ = ctl;
    std::thread::spawn(|| {
        let picked = rfd::FileDialog::new().set_title("Add a music folder").pick_folder();
        let Some(path) = picked else { return };
        on_ui_thread(move |ctl| {
            {
                let mut c = ctl.borrow_mut();
                let mut folders = c.settings.folders();
                if !folders.contains(&path) {
                    folders.push(path);
                    c.settings.set_folders(&folders);
                }
            }
            apply_all_settings(ctl);
            start_scan(ctl);
        });
    });
}

fn remove_folder(ctl: &Ctl, index: usize) {
    {
        let mut c = ctl.borrow_mut();
        let mut folders: Vec<PathBuf> = c.settings.folders();
        if index < folders.len() {
            folders.remove(index);
            c.settings.set_folders(&folders);
        }
    }
    apply_all_settings(ctl);
    start_scan(ctl);
}

// ---------------------------------------------------------------------------------------------
// The tick: position, now-playing changes, events
// ---------------------------------------------------------------------------------------------

// ---------------------------------------------------------------------------------------------
// Playlists
// ---------------------------------------------------------------------------------------------

fn load_playlists(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let list = ctl.borrow().playlists.list();
    app.set_playlists(strings_model(list.iter().map(|p| p.name.clone())));
    let open = ctl.borrow().open_playlist.clone();
    let index = open.and_then(|o| list.iter().position(|p| p.file == o.file)).map_or(-1, |i| i as i32);
    app.set_playlist_index(index);
    ctl.borrow_mut().playlist_list = list;
}

fn open_playlist(ctl: &Ctl, index: usize) {
    let Some(p) = ctl.borrow().playlist_list.get(index).cloned() else { return };
    ctl.borrow_mut().open_playlist = Some(p);
    if let Some(ui) = ui_of(ctl) {
        ui.global::<App>().set_playlist_index(index as i32);
    }
    show_page(ctl, PAGE_PLAYLIST);
}

/// Shows the open playlist's songs that are in the library, in playlist order.
fn load_open_playlist(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let Some(p) = ctl.borrow().open_playlist.clone() else { return };
    let c = ctl.borrow();
    let paths = c.playlists.paths(&p);
    let ids: Vec<u32> = c.lib.known(&paths).into_iter().flatten().map(|(id, _, _)| id).collect();
    let total_ms: u64 = ids.iter().filter_map(|&i| c.lib.track(i)).map(|t| t.duration_ms as u64).sum();
    let mut subtitle = if ids.len() == 1 { "1 song".to_string() } else { format!("{} songs", fmt_count(ids.len())) };
    if total_ms > 0 {
        subtitle.push_str(&format!(" · {} min", (total_ms / 60_000).max(1)));
    }
    if paths.len() > ids.len() {
        subtitle.push_str(&format!(" · {} not in your library", paths.len() - ids.len()));
    }
    app.set_playlist_title(p.name.as_str().into());
    app.set_playlist_subtitle(subtitle.into());
    c.songs.set(Rc::new(ids), false);
}

fn is_open_playlist(ctl: &Ctl, p: &Playlist) -> bool {
    ctl.borrow().open_playlist.as_ref().is_some_and(|o| o.file == p.file)
}

/// The playlist button of a song row: remember the song, open the menu next to the button.
fn playlist_menu(ctl: &Ctl, row: usize, x: f32, y: f32) {
    let path = {
        let c = ctl.borrow();
        c.songs.id_at(row).and_then(|id| c.lib.track(id)).map(|t| t.path)
    };
    let (Some(path), Some(ui)) = (path, ui_of(ctl)) else { return };
    ctl.borrow_mut().pending_add = vec![path];
    let app = ui.global::<App>();
    app.set_menu_x(x);
    app.set_menu_y(y);
    app.set_menu_open(true);
}

/// A menu choice: a playlist, or -1 for a new one (which then gets the song).
fn add_to_playlist(ctl: &Ctl, index: i32) {
    if let Some(ui) = ui_of(ctl) {
        ui.global::<App>().set_menu_open(false);
    }
    if index < 0 {
        open_dialog(ctl, DIALOG_NEW);
        return;
    }
    let Some(p) = ctl.borrow().playlist_list.get(index as usize).cloned() else { return };
    add_pending_to(ctl, &p);
}

fn add_pending_to(ctl: &Ctl, p: &Playlist) {
    let paths = std::mem::take(&mut ctl.borrow_mut().pending_add);
    let added = ctl.borrow().playlists.add(p, &paths);
    match added {
        Ok(0) => toast(ctl, &format!("Already in {}", p.name)),
        Ok(_) => toast(ctl, &format!("Added to {}", p.name)),
        Err(e) => toast(ctl, &e),
    }
    if is_open_playlist(ctl, p) {
        refresh_view(ctl);
    }
}

fn playlist_remove_row(ctl: &Ctl, row: usize) {
    let (open, path) = {
        let c = ctl.borrow();
        (c.open_playlist.clone(), c.songs.id_at(row).and_then(|id| c.lib.track(id)).map(|t| t.path))
    };
    let (Some(p), Some(path)) = (open, path) else { return };
    let removed = ctl.borrow().playlists.remove(&p, &path);
    if let Err(e) = removed {
        toast(ctl, &e);
    }
    refresh_view(ctl);
}

fn open_dialog(ctl: &Ctl, mode: i32) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let text = match mode {
        DIALOG_RENAME | DIALOG_DELETE => ctl.borrow().open_playlist.as_ref().map(|p| p.name.clone()).unwrap_or_default(),
        DIALOG_SAVE_QUEUE => "Saved queue".to_string(),
        _ => String::new(),
    };
    app.set_dialog_text(text.into());
    app.set_dialog_error(SharedString::new());
    app.set_dialog(mode);
}

/// OK in the dialog. Errors (a duplicate name, say) stay in the dialog.
fn dialog_accept(ctl: &Ctl, text: String) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let open = ctl.borrow().open_playlist.clone();
    let result: Result<(), String> = match app.get_dialog() {
        DIALOG_NEW => {
            let created = ctl.borrow().playlists.create(&text);
            created.map(|p| {
                load_playlists(ctl);
                if ctl.borrow().pending_add.is_empty() {
                    toast(ctl, &format!("Created {}", p.name));
                } else {
                    add_pending_to(ctl, &p);
                }
            })
        }
        DIALOG_RENAME => match open {
            Some(p) => {
                let renamed = ctl.borrow().playlists.rename(&p, &text);
                renamed.map(|np| {
                    ctl.borrow_mut().open_playlist = Some(np);
                    load_playlists(ctl);
                    refresh_view(ctl);
                })
            }
            None => Ok(()),
        },
        DIALOG_DELETE => match open {
            Some(p) => {
                let deleted = ctl.borrow().playlists.delete(&p);
                deleted.map(|()| {
                    ctl.borrow_mut().open_playlist = None;
                    load_playlists(ctl);
                    show_page(ctl, PAGE_SONGS);
                    toast(ctl, &format!("Deleted {}", p.name));
                })
            }
            None => Ok(()),
        },
        DIALOG_SAVE_QUEUE => {
            let paths: Vec<String> = {
                let c = ctl.borrow();
                let q = c.player.queue();
                (0..q.len()).filter_map(|i| q.at(i)).filter_map(|item| c.lib.track(item.track)).map(|t| t.path).collect()
            };
            let created = ctl.borrow().playlists.create(&text);
            created.and_then(|p| {
                let added = ctl.borrow().playlists.add(&p, &paths)?;
                load_playlists(ctl);
                toast(ctl, &format!("Saved {} songs to {}", added, p.name));
                Ok(())
            })
        }
        _ => Ok(()),
    };
    match result {
        Ok(()) => app.set_dialog(0),
        Err(e) => app.set_dialog_error(e.into()),
    }
}

// ---------------------------------------------------------------------------------------------
// Unidentified songs
// ---------------------------------------------------------------------------------------------

/// The count for the Settings badge and, while their page is open, the list of songs itself.
fn refresh_flagged(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let c = ctl.borrow();
    let count = if app.get_page() == PAGE_FLAGGED {
        let ids = c.lib.flagged_ids();
        let count = ids.len();
        c.flagged.set(Rc::new(ids), false);
        count
    } else {
        c.lib.flagged_count()
    };
    app.set_flagged_count(count as i32);
    app.set_flagged_text(if count == 0 { SharedString::new() } else { format!("{} Unidentified", fmt_count(count)).into() });
}

/// The track editor changed song `id`: every list that shows it catches up, and so does the
/// player's own display if it is the song that is playing.
fn library_edited(ctl: &Ctl, id: u32) {
    {
        let c = ctl.borrow();
        c.songs.invalidate();
        c.albums.release();
        c.queue_page.changed();
        c.upnext.changed();
    }
    refresh_view(ctl);
    let uid = ctl.borrow().cur_uid;
    if uid == 0 {
        return;
    }
    let track = {
        let c = ctl.borrow();
        let item = c.player.queue().get(uid);
        item.and_then(|it| c.lib.track(it.track)).filter(|t| t.id == id)
    };
    let (Some(t), Some(ui)) = (track, ui_of(ctl)) else { return };
    let app = ui.global::<App>();
    app.set_title(t.title.as_str().into());
    app.set_artist(t.artist.as_str().into());
    app.set_album(t.album.as_str().into());
    let c = ctl.borrow();
    c.loader.request(Job::Now { uid, path: PathBuf::from(&t.path), cover: t.cover.clone(), lyrics: c.lib.lyrics(t.id) });
}

fn start_tick(ctl: &Ctl) {
    set_tick_interval(ctl, 100);
}

fn set_tick_interval(ctl: &Ctl, ms: u64) {
    let mut c = ctl.borrow_mut();
    if c.tick_ms == ms {
        return;
    }
    c.tick_ms = ms;
    let weak = Rc::downgrade(ctl);
    c.tick_timer.start(TimerMode::Repeated, Duration::from_millis(ms), move || {
        if let Some(ctl) = weak.upgrade() {
            on_tick(&ctl);
        }
    });
}

fn on_tick(ctl: &Ctl) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    let (now, playing, ended, events) = {
        let c = ctl.borrow();
        (c.player.now(), c.player.is_playing(), c.player.ended_count(), c.player.poll_events())
    };
    for ev in events {
        match ev {
            Event::Error(msg) => toast(ctl, &msg),
            Event::Output { device, rate, channels } => {
                let layout = match channels {
                    1 => "mono".to_string(),
                    2 => "stereo".to_string(),
                    n => format!("{n} channels"),
                };
                app.set_output_info(format!("{device} · {:.1} kHz · {layout}", rate as f32 / 1000.0).into());
            }
        }
    }
    if now.uid != ctl.borrow().cur_uid {
        on_track_change(ctl, now.uid);
    }
    app.set_playing(playing);

    let (dur, last_ended) = {
        let c = ctl.borrow();
        (c.cur_duration_ms, c.last_ended)
    };
    if ended != last_ended {
        ctl.borrow_mut().last_ended = ended;
        app.set_playing(false);
    }
    if now.uid != 0 && dur > 0 {
        let pos = (now.pos_ms as u64).min(dur);
        app.set_progress((pos as f64 / dur as f64) as f32);
        app.set_pos_text(fmt_time(pos).into());
    } else if now.uid != 0 {
        app.set_pos_text(fmt_time(now.pos_ms as u64).into());
    }

    // Synchronized lyrics: highlight the line that has started.
    let active = {
        let c = ctl.borrow();
        c.lyrics.as_ref().filter(|l| l.synced).map_or(-1, |l| lyrics::active_line(&l.lines, now.pos_ms).map_or(-1, |i| i as i32))
    };
    if active != app.get_lyrics_active() {
        app.set_lyrics_active(active);
    }

    // The phone's media notification follows the song and whether it plays.
    {
        let state = (ctl.borrow().cur_uid, app.get_playing());
        if state != ctl.borrow().announced {
            ctl.borrow_mut().announced = state;
            let (title, artist) = if state.0 == 0 { Default::default() } else { (app.get_title(), app.get_artist()) };
            host::now_playing(&NowPlaying { title: &title, artist: &artist, playing: state.1 });
        }
    }

    // A scan (or anything else) changed the library: refresh lists.
    let (rev, seen) = {
        let c = ctl.borrow();
        (c.progress.revision.load(Relaxed), c.rev)
    };
    if rev != seen && !ctl.borrow().progress.running.load(Acquire) {
        scan_progress(ctl);
    }

    track_scale(ctl, &ui);

    if ctl.borrow().debug {
        // WISP_DEBUG=1: playback health on stderr, every two seconds while playing.
        let mut c = ctl.borrow_mut();
        c.ticks = c.ticks.wrapping_add(1);
        if playing && c.ticks % 20 == 0 {
            eprintln!("[wisp] track {} at {} ms, underruns {}", now.uid, now.pos_ms, c.player.underruns());
        }
    }

    // Tick quickly only while something moves.
    let animating = playing || app.get_np_open();
    set_tick_interval(ctl, if animating { 100 } else { 400 });
}

fn on_track_change(ctl: &Ctl, uid: u32) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    ctl.borrow_mut().cur_uid = uid;
    app.set_now_uid(uid as i32);
    if uid == 0 {
        app.set_has_track(false);
        app.set_now_track(-1);
        return;
    }
    let (track, index) = {
        let c = ctl.borrow();
        let q = c.player.queue();
        (q.get(uid).and_then(|it| c.lib.track(it.track)), q.index_of(uid))
    };
    if let Some(index) = index {
        ctl.borrow().upnext.set_start(index);
        app.set_upnext_offset(index as i32);
    }
    let Some(track) = track else { return };
    app.set_has_track(true);
    app.set_title(track.title.as_str().into());
    app.set_artist(track.artist.as_str().into());
    app.set_album(track.album.as_str().into());
    app.set_now_track(track.id as i32);
    app.set_progress(0.0);
    app.set_pos_text("0:00".into());
    app.set_dur_text(if track.duration_ms > 0 { fmt_time(track.duration_ms as u64) } else { "--:--".to_string() }.into());
    app.set_lyrics(ModelRc::new(VecModel::from(Vec::<LyricLine>::new())));
    app.set_lyrics_synced(false);
    app.set_lyrics_active(-1);
    app.set_lyrics_loading(true);
    let mut c = ctl.borrow_mut();
    c.cur_duration_ms = track.duration_ms as u64;
    c.lyrics = None;
    c.loader.request(Job::Now { uid, path: PathBuf::from(&track.path), cover: track.cover.clone(), lyrics: c.lib.lyrics(track.id) });
}

fn handle_done(ctl: &Ctl, done: Done) {
    let Some(ui) = ui_of(ctl) else { return };
    let app = ui.global::<App>();
    match done {
        Done::Thumb { index, art } => {
            let image = art.as_ref().map(to_image);
            ctl.borrow().albums.art_loaded(index, image);
        }
        Done::Detail { index, art } => {
            // The cover of an album the user has already left is dropped.
            if index as i32 == app.get_detail_seed() {
                if let Some(a) = art {
                    app.set_detail_art(to_image(&a));
                    app.set_detail_has_art(true);
                }
            }
        }
        Done::Now { uid, assets } => {
            if uid != ctl.borrow().cur_uid {
                return; // the track changed again while this was loading
            }
            apply_now_assets(ctl, &ui, assets);
        }
    }
}

fn apply_now_assets(ctl: &Ctl, ui: &MainWindow, assets: NowAssets) {
    let app = ui.global::<App>();
    match &assets.art {
        Some(a) => {
            app.set_art(to_image(a));
            app.set_has_art(true);
        }
        None => {
            app.set_art(Image::default());
            app.set_has_art(false);
        }
    }
    match &assets.backdrop {
        Some(b) => {
            let flip = ctl.borrow().backdrop_flip;
            // Load into the hidden slot, then flip: the UI cross-fades between the two.
            if flip {
                app.set_backdrop_a(to_image(&upscale_backdrop(b)));
                app.set_backdrop_flip(false);
            } else {
                app.set_backdrop_b(to_image(&upscale_backdrop(b)));
                app.set_backdrop_flip(true);
            }
            ctl.borrow_mut().backdrop_flip = !flip;
            app.set_has_backdrop(true);
        }
        None => app.set_has_backdrop(false),
    }
    {
        let mut c = ctl.borrow_mut();
        c.accent_hsv = assets.hsv;
        c.backdrop_luma = assets.backdrop.as_ref().map_or(0.0, themes::mean_luma);
        apply_theme(&c, ui);
        c.lyrics = assets.lyrics.clone();
    }
    match &assets.lyrics {
        Some(l) => {
            let lines: Vec<LyricLine> = l.lines.iter().map(|x| LyricLine { text: x.text.as_str().into(), time: x.time_ms as i32 }).collect();
            app.set_lyrics(ModelRc::new(VecModel::from(lines)));
            app.set_lyrics_synced(l.synced);
        }
        None => {
            app.set_lyrics(ModelRc::new(VecModel::from(Vec::<LyricLine>::new())));
            app.set_lyrics_synced(false);
        }
    }
    app.set_lyrics_loading(false);
}

/// The 40x40 backdrop is stretched to the window by the renderer; bilinear scaling of such a
/// small image shows faint blockiness, so smooth it up to 160x160 once (100 KB) with bicubic-ish
/// interpolation (two linear passes over an already very soft image).
fn upscale_backdrop(src: &Rgb) -> Rgb {
    let (w, h) = (160u32, 160u32);
    let mut px = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        let fy = (y as f32 + 0.5) * src.h as f32 / h as f32 - 0.5;
        let (y0, ty) = (fy.floor().clamp(0.0, src.h as f32 - 1.0) as u32, fy - fy.floor());
        let y1 = (y0 + 1).min(src.h - 1);
        for x in 0..w {
            let fx = (x as f32 + 0.5) * src.w as f32 / w as f32 - 0.5;
            let (x0, tx) = (fx.floor().clamp(0.0, src.w as f32 - 1.0) as u32, fx - fx.floor());
            let x1 = (x0 + 1).min(src.w - 1);
            for c in 0..3 {
                let p = |xx: u32, yy: u32| src.px[((yy * src.w + xx) * 3) as usize + c] as f32;
                let top = p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx;
                let bottom = p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx;
                px.push((top * (1.0 - ty) + bottom * ty).round() as u8);
            }
        }
    }
    Rgb { w, h, px }
}

fn toast(ctl: &Ctl, message: &str) {
    let Some(ui) = ui_of(ctl) else { return };
    ui.global::<App>().set_toast(message.into());
    let weak = ctl.borrow().ui.clone();
    ctl.borrow().toast_timer.start(TimerMode::SingleShot, Duration::from_millis(3200), move || {
        if let Some(ui) = weak.upgrade() {
            ui.global::<App>().set_toast(SharedString::new());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colors_parse_strictly() {
        assert_eq!(parse_hex("#ff3b8d"), Some([0xff, 0x3b, 0x8d]));
        assert_eq!(parse_hex("FF3B8D"), Some([0xff, 0x3b, 0x8d]));
        assert_eq!(parse_hex(" #000000 "), Some([0, 0, 0]));
        assert_eq!(parse_hex("#fff"), None);
        assert_eq!(parse_hex("#gg0000"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn backdrop_upscale_keeps_size_and_average_color() {
        let src = Rgb { w: 40, h: 40, px: [200u8, 100, 50].iter().cycle().take(40 * 40 * 3).copied().collect() };
        let up = upscale_backdrop(&src);
        assert_eq!((up.w, up.h, up.px.len()), (160, 160, 160 * 160 * 3));
        assert!(up.px.chunks(3).all(|p| p == [200, 100, 50]), "a flat color stays flat");
    }
}
