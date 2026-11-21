//! Demo content for headless screenshots (`--features screenshot`): fills the UI with realistic
//! data and synthesized artwork so the layout can be reviewed without audio files.

use slint::{ComponentHandle, Image, ModelRc, Rgb8Pixel, SharedPixelBuffer, SharedString, VecModel};

use crate::art::{self, Rgb};
use crate::themes;
use crate::{AlbumData, App, ArtistData, DeviceRow, LyricLine, MainWindow, QueueRow, ResultRow, Theme, TrackRow};

const PALETTES: [([u8; 3], [u8; 3]); 8] = [
    ([250, 45, 72], [90, 20, 60]),
    ([255, 122, 61], [120, 40, 30]),
    ([242, 182, 50], [120, 70, 20]),
    ([47, 196, 103], [15, 80, 70]),
    ([47, 181, 200], [20, 50, 110]),
    ([79, 124, 255], [40, 30, 120]),
    ([155, 92, 240], [60, 25, 100]),
    ([255, 92, 147], [100, 25, 80]),
];

/// A pleasant gradient cover with a soft disc, deterministic per seed.
pub fn make_art(seed: usize, size: u32) -> Rgb {
    let (a, b) = PALETTES[seed % PALETTES.len()];
    let mut px = Vec::with_capacity((size * size * 3) as usize);
    let s = size as f32;
    for y in 0..size {
        for x in 0..size {
            let t = (x as f32 + y as f32) / (2.0 * s);
            let mut c = [0f32; 3];
            for i in 0..3 {
                c[i] = a[i] as f32 * (1.0 - t) + b[i] as f32 * t;
            }
            // Soft disc.
            let (cx, cy) = (s * (0.35 + 0.1 * (seed % 3) as f32), s * 0.62);
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / (s * 0.34);
            let k = (1.0 - d).clamp(0.0, 1.0).powf(0.7) * 0.35;
            for i in 0..3 {
                c[i] = c[i] * (1.0 - k) + 255.0 * k;
            }
            px.extend(c.map(|v| v.clamp(0.0, 255.0) as u8));
        }
    }
    Rgb { w: size, h: size, px }
}

pub fn to_image(a: &Rgb) -> Image {
    let buf = SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(&a.px, a.w, a.h);
    Image::from_rgb8(buf)
}

const SONGS: [(&str, &str, &str, &str); 18] = [
    ("Starálfur", "Sigur Rós", "Ágætis byrjun", "10:07"),
    ("Hoppípolla", "Sigur Rós", "Takk…", "4:28"),
    ("Windowlicker", "Aphex Twin", "Windowlicker", "6:07"),
    ("Teardrop", "Massive Attack", "Mezzanine", "5:29"),
    ("Re: Stacks", "Bon Iver", "For Emma, Forever Ago", "6:41"),
    ("Pyramids", "Frank Ocean", "channel ORANGE", "9:52"),
    ("Nude", "Radiohead", "In Rainbows", "4:15"),
    ("Never Let Me Down Again", "Depeche Mode", "Black Celebration", "4:47"),
    ("The Less I Know the Better", "Tame Impala", "Currents", "3:36"),
    ("Dreams", "Fleetwood Mac", "Rumours", "4:14"),
    ("夜に駆ける", "YOASOBI", "THE BOOK", "4:21"),
    ("Голубые города", "Наутилус Помпилиус", "Разлука", "5:02"),
    ("Cherry-coloured Funk", "Cocteau Twins", "Heaven or Las Vegas", "3:12"),
    ("Midnight City", "M83", "Hurry Up, We're Dreaming", "4:03"),
    ("Bags", "Clairo", "Immunity", "4:21"),
    ("Strawberry Swing", "Coldplay", "Viva la Vida", "4:09"),
    ("Dog Days Are Over", "Florence + The Machine", "Lungs", "4:12"),
    ("Archangel", "Burial", "Untrue", "3:58"),
];

const ALBUMS: [(&str, &str, &str); 14] = [
    ("Ágætis byrjun", "Sigur Rós", "1999"),
    ("Mezzanine", "Massive Attack", "1998"),
    ("In Rainbows", "Radiohead", "2007"),
    ("Currents", "Tame Impala", "2015"),
    ("Rumours", "Fleetwood Mac", "1977"),
    ("channel ORANGE", "Frank Ocean", "2012"),
    ("Heaven or Las Vegas", "Cocteau Twins", "1990"),
    ("Untrue", "Burial", "2007"),
    ("Lungs", "Florence + The Machine", "2009"),
    ("Hurry Up, We're Dreaming", "M83", "2011"),
    ("Immunity", "Clairo", "2019"),
    ("Viva la Vida", "Coldplay", "2008"),
    ("THE BOOK", "YOASOBI", "2021"),
    ("For Emma, Forever Ago", "Bon Iver", "2007"),
];

const LYRICS: [&str; 14] = [
    "Teardrop on the fire",
    "Fearless on my breath",
    "",
    "Nine night, black out",
    "Sunrise in the cold",
    "Hold me close, never let go",
    "Water is my eye",
    "Most faithful mirror",
    "",
    "Fleeting is the night",
    "Black flowers blossom",
    "Fearless on my breath",
    "Teardrop on the fire",
    "Of a confession, whispered low",
];

pub fn populate(ui: &MainWindow, scene: &str, theme_id: &str, phone: bool, width: u32) {
    let app = ui.global::<App>();
    let theme = ui.global::<Theme>();
    // The window sets `compact` itself once it has its size; set here too so the first frame is right.
    let compact = width < 840;
    theme.set_touch_screen(phone);
    theme.set_compact(compact);

    // Now playing.
    let art = make_art(5, 512);
    app.set_has_track(true);
    app.set_playing(true);
    app.set_title("Teardrop".into());
    app.set_artist("Massive Attack".into());
    app.set_album("Mezzanine".into());
    app.set_art(to_image(&art));
    app.set_has_art(true);
    app.set_now_track(3);
    app.set_now_uid(3);
    app.set_progress(0.38);
    app.set_pos_text("2:04".into());
    app.set_dur_text("5:29".into());
    app.set_volume(0.72);
    app.set_repeat(1);
    app.set_backdrop_a(to_image(&art::backdrop(&art)));
    app.set_has_backdrop(true);
    let hsv = art::dominant_hsv(&art);
    let base = themes::find(theme_id);
    let def = if base.dynamic { themes::chameleon(hsv, themes::mean_luma(&art::backdrop(&art))) } else { base.clone() };
    let accent = match (def.follows_art, hsv) {
        (true, Some(hsv)) => themes::artwork_accent(&def, hsv),
        _ => def.accent,
    };
    themes::write(&theme, &def, accent, if def.follows_art { accent } else { def.accent2 }, true);
    app.set_theme_id(def.id.into());
    app.set_theme_name(def.name.into());
    app.set_theme_groups(themes::groups());
    app.set_backdrop_locked(!def.backdrop);

    // Lists.
    let rows: Vec<TrackRow> = (0..60)
        .map(|i| {
            let (t, a, al, d) = SONGS[i % SONGS.len()];
            TrackRow {
                id: i as i32,
                title: t.into(),
                artist: a.into(),
                album: al.into(),
                duration: d.into(),
                number: if scene == "detail" { format!("{}", i + 1).into() } else { SharedString::new() },
                flags: 0,
            }
        })
        .collect();
    app.set_tracks(ModelRc::new(VecModel::from(rows)));
    let albums: Vec<AlbumData> = ALBUMS
        .iter()
        .enumerate()
        .cycle()
        .take(44)
        .enumerate()
        .map(|(n, (i, (t, a, y)))| AlbumData {
            index: n as i32,
            title: (*t).into(),
            artist: (*a).into(),
            year: (*y).into(),
            art: if i % 3 == 2 { Image::default() } else { to_image(&make_art(i, 220)) },
            has_art: i % 3 != 2,
        })
        .collect();
    app.set_albums(ModelRc::new(VecModel::from(albums)));
    let artists: Vec<ArtistData> = ALBUMS
        .iter()
        .enumerate()
        .map(|(i, (_, a, _))| ArtistData {
            index: i as i32,
            name: (*a).into(),
            albums: format!("{} album{}", 1 + i % 3, if i % 3 == 0 { "" } else { "s" }).into(),
            initial: a.chars().next().unwrap_or('?').to_string().into(),
        })
        .collect();
    app.set_artists(ModelRc::new(VecModel::from(artists)));
    let queue: Vec<QueueRow> = (0..12)
        .map(|i| {
            let (t, a, _, d) = SONGS[(i + 3) % SONGS.len()];
            QueueRow { uid: i as i32 + 3, track: i as i32, title: t.into(), artist: a.into(), duration: d.into() }
        })
        .collect();
    app.set_upnext(ModelRc::new(VecModel::from(queue.clone())));
    app.set_queue(ModelRc::new(VecModel::from(queue)));
    app.set_songs_subtitle("12,431 songs".into());
    app.set_albums_subtitle("1,024 albums".into());
    app.set_artists_subtitle("386 artists".into());
    app.set_queue_subtitle("12 songs · 52 min".into());

    // Lyrics.
    let lines: Vec<LyricLine> =
        LYRICS.iter().enumerate().map(|(i, t)| LyricLine { text: (*t).into(), time: i as i32 * 4000 }).collect();
    app.set_lyrics(ModelRc::new(VecModel::from(lines)));
    app.set_lyrics_synced(true);
    app.set_lyrics_active(4);

    // Album detail.
    app.set_detail_title("Mezzanine".into());
    app.set_detail_artist("Massive Attack".into());
    app.set_detail_meta("1998 · 11 songs · 62 min".into());
    app.set_detail_art(to_image(&make_art(1, 400)));
    app.set_detail_has_art(true);
    app.set_detail_seed(1);

    // Settings.
    app.set_devices(ModelRc::new(VecModel::from(vec![
        DeviceRow { id: "".into(), name: "System default".into() },
        DeviceRow { id: "a".into(), name: "Speakers (Realtek High Definition Audio)".into() },
        DeviceRow { id: "b".into(), name: "Headphones (WH-1000XM5)".into() },
    ])));
    app.set_output_info("48 kHz · 2 channels · System default".into());
    app.set_folders(ModelRc::new(VecModel::from(vec![
        SharedString::from(r"C:\Users\you\Music"),
        SharedString::from(r"D:\Lossless Collection"),
    ])));
    app.set_library_info("12,431 songs in 2 folders".into());
    app.set_norm_index(1);
    app.set_preamp(1.5);

    // Playlists.
    let names = ["Road trip", "Late night", "Gym", "Focus", "Sunday morning"];
    app.set_playlists(ModelRc::new(VecModel::from(names.iter().map(|n| SharedString::from(*n)).collect::<Vec<_>>())));
    app.set_playlist_index(0);
    app.set_playlist_title("Road trip".into());
    app.set_playlist_subtitle("18 songs · 1 hr 12 min".into());

    // Unidentified songs, and the track editor open on the first of them.
    let unidentified: [(&str, &str, i32); 9] = [
        ("AUDIO_294", "Unknown Artist", 7),
        ("track_01", "Unknown Artist", 7),
        ("Flashing Lights", "Unknown Artist", 5),
        ("04. Daft Punk - Around the World (Official Audio) [320kbps]", "Unknown Artist", 7),
        ("New Recording 12", "Unknown Artist", 7),
        ("Windowlicker", "Aphex Twin", 4),
        ("Untitled 3", "Unknown Artist", 3),
        ("Strawberry Swing", "Coldplay", 4),
        ("07 Archangel", "Burial", 2),
    ];
    let flagged: Vec<TrackRow> = unidentified
        .iter()
        .enumerate()
        .map(|(i, (t, a, flags))| TrackRow {
            id: i as i32,
            title: (*t).into(),
            artist: (*a).into(),
            album: "Unknown Album".into(),
            duration: "3:57".into(),
            number: SharedString::new(),
            flags: *flags,
        })
        .collect();
    app.set_flagged(ModelRc::new(VecModel::from(flagged)));
    app.set_flagged_count(14);
    app.set_flagged_text("14 Unidentified".into());
    if scene == "editor" || scene == "editor-fields" {
        let matches: [(&str, &str, &str, &str, bool, i32); 5] = [
            ("Flashing Lights", "Kanye West feat. Dwele", "Graduation · 2007 · Track 9", "", true, 2),
            ("Flashing Lights", "Kanye West feat. Dwele", "Graduation (Deluxe) · 2007 · Track 9", "", true, 2),
            ("Flashing Lights", "Kanye West feat. Dwele", "Late Orchestration · 2006 · Track 4", "", false, 1),
            ("Flashing Lights", "Kanye West", "Kanye West Live 2008 · Track 6", "", false, 0),
            ("Flashing Light", "Hospital Records", "Hospitality · 2012 · Track 13", "", false, 0),
        ];
        let rows: Vec<ResultRow> = matches
            .iter()
            .enumerate()
            .map(|(i, (title, artist, detail, _, same, lyrics))| ResultRow {
                title: (*title).into(),
                artist: (*artist).into(),
                detail: (*detail).into(),
                thumb: if i == 3 { Image::default() } else { to_image(&make_art(i + 2, 136)) },
                has_thumb: i != 3,
                lyrics: *lyrics,
                same_length: *same,
            })
            .collect();
        app.set_ed_results(ModelRc::new(VecModel::from(rows)));
        app.set_ed_open(true);
        app.set_ed_file("09 Flashing Lights.mp3  ·  3:57".into());
        app.set_ed_reason("No artist · No cover".into());
        app.set_ed_where("Save writes these tags into the MP3 file. The music itself is not touched.".into());
        app.set_ed_query("Flashing Lights".into());
        app.set_ed_status("5 matches".into());
        app.set_ed_picked(0);
        app.set_ed_title("Flashing Lights".into());
        app.set_ed_artist("Kanye West feat. Dwele".into());
        app.set_ed_album("Graduation".into());
        app.set_ed_album_artist("Kanye West".into());
        app.set_ed_year("2007".into());
        app.set_ed_track("9".into());
        app.set_ed_cover(to_image(&make_art(2, 240)));
        app.set_ed_has_cover(true);
        app.set_ed_cover_note("Cover from the Cover Art Archive".into());
        app.set_ed_lyrics("[00:12.30] Something in the way you move\n[00:16.10] Makes me feel like I can't live without you\n[00:20.00] It takes me all the way\n[00:23.50] I want you to stay".into());
        app.set_ed_lyrics_note("Synced lyrics from LRCLIB".into());
        app.set_ed_lyrics_avail(3);
        app.set_ed_step(if scene == "editor-fields" { 1 } else { 0 });
    }

    let page = match scene {
        "albums" => 1,
        "artists" => 2,
        "queue" => 3,
        "settings" => 4,
        "detail" => 5,
        "playlist" => 6,
        "unidentified" | "editor" | "editor-fields" => 7,
        "playlists" => 8,
        _ => 0,
    };
    app.set_page(page);
    app.set_np_open(matches!(scene, "np" | "np-lyrics" | "np-queue"));
    // A narrow window's "np" is the player itself (-1); a wide one has no such view and shows the lyrics.
    app.set_np_tab(match scene {
        "np-queue" => 1,
        "np" if compact => -1,
        _ => 0,
    });
    if scene == "sheet" {
        app.set_sheet_row(2);
    }
    if scene == "sheet-playlist" {
        app.set_page(6);
        app.set_sheet_row(2);
        app.set_sheet_removable(true);
    }
    if scene == "menu" {
        app.set_menu_open(true);
    }
    if scene == "dialog" {
        app.set_dialog(1);
    }
    if scene == "toast" {
        app.set_toast("Added to queue".into());
    }
    if scene == "empty" {
        app.set_tracks(ModelRc::new(VecModel::from(Vec::<TrackRow>::new())));
        app.set_has_track(false);
        app.set_title("".into());
        app.set_artist("".into());
        app.set_playing(false);
        app.set_has_art(false);
        app.set_has_backdrop(false);
        app.set_progress(0.0);
        app.set_pos_text("0:00".into());
        app.set_dur_text("0:00".into());
    }
}
