//! Themes: named palettes plus the handful of style traits the interface honors.
//!
//! A theme is plain data. [`write`] copies one into the `Theme` global of the UI, and the UI
//! components read their colors and style from that global and nothing else, so adding a theme
//! is adding a row to [`build`]. Palettes are written out explicitly where the palette defines
//! them; everything a palette leaves open (hover states, tracks, scrollbars, ...) is derived by
//! [`base`] from the three colors that matter: background, text and accent.
//!
//! The style traits are the parts of a "look" that cost nothing to draw: corner rounding, border
//! width, fonts, an accent glow, a glossy highlight, beveled edges and the paired shadows of soft
//! neumorphic surfaces. Effects that need textures, animation or audio analysis (visualizers,
//! film grain, perspective grids, ...) are deliberately not part of this.
//!
//! One theme, Album Chameleon, is not fixed: [`chameleon`] derives its palette from the artwork
//! of the track that is playing and picks light or dark text for it.

use std::sync::OnceLock;

use slint::{ModelRc, VecModel};

use crate::art;

/// An RGBA color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Col {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Opaque `0xRRGGBB`.
pub const fn c(v: u32) -> Col {
    Col { r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8, a: 255 }
}

/// `0xRRGGBB` at `pct` percent opacity.
pub const fn ca(v: u32, pct: u32) -> Col {
    Col { r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8, a: ((pct * 255 + 50) / 100) as u8 }
}

/// `0xRRGGBBAA`.
pub const fn ch(v: u32) -> Col {
    Col { r: (v >> 24) as u8, g: (v >> 16) as u8, b: (v >> 8) as u8, a: v as u8 }
}

const CLEAR: Col = Col { r: 0, g: 0, b: 0, a: 0 };
const WHITE: Col = c(0xffffff);
const BLACK: Col = c(0x000000);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Wisp,
    Developer,
    Retro,
    Modern,
    Mood,
}

impl Group {
    pub const ALL: [Group; 5] = [Group::Wisp, Group::Developer, Group::Retro, Group::Modern, Group::Mood];

    pub fn title(self) -> &'static str {
        match self {
            Group::Wisp => "Wisp",
            Group::Developer => "Developer favorites",
            Group::Retro => "Retro & nostalgic",
            Group::Modern => "Modern glass & light",
            Group::Mood => "Moods & atmospheres",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ThemeDef {
    pub id: &'static str,
    pub name: &'static str,
    pub group: Group,
    /// Light text on a dark background (decides how artwork-derived accents are tuned).
    pub dark: bool,
    /// With the accent setting on Auto, the accent follows the playing album's artwork.
    pub follows_art: bool,
    /// The palette is derived from the artwork (see [`chameleon`]).
    pub dynamic: bool,
    /// The blurred album-art backdrop may show (the Ambient backdrop setting decides whether it does).
    pub backdrop: bool,

    // -- palette
    pub bg: Col,
    /// Bottom of a vertical gradient window background.
    pub bg2: Option<Col>,
    pub sidebar: Col,
    pub surface: Col,
    pub surface_hi: Col,
    pub bar: Col,
    pub overlay: Col,
    /// Dims the artwork backdrop so text stays readable on any cover.
    pub scrim: Col,
    pub text: Col,
    pub text2: Col,
    pub text3: Col,
    pub line: Col,
    pub hover: Col,
    pub press: Col,
    pub track: Col,
    pub thumb: Col,
    /// Selected segment of a segmented control.
    pub seg: Col,
    /// Slider thumbs and toggle knobs.
    pub knob: Col,
    pub shadow: Col,
    pub danger: Col,
    /// Colored film over the artwork backdrop (a sepia or lamp-light cast).
    pub tint: Col,
    pub accent: Col,
    pub accent2: Col,
    /// Text on accent-filled controls; derived from the accent when `None`.
    pub on_accent: Option<Col>,

    // -- style
    /// Corner rounding of cards, rows, buttons and inputs, relative to the default look (1.0).
    pub rounding: f32,
    /// How round the round things are (play button, icon buttons, toggles, slider thumbs): 1 circle, 0 square.
    pub circle: f32,
    pub border: f32,
    pub font: &'static str,
    pub title_font: &'static str,
    pub weight: i32,
    pub glow: f32,
    pub gloss: f32,
    pub bevel: bool,
    pub bevel_light: Col,
    pub bevel_dark: Col,
    pub neu: f32,
    pub neu_light: Col,
    pub neu_dark: Col,
    /// Strength of the accent-colored wash in the window's top corner.
    pub wash: f32,
    /// Size of icon buttons relative to the default.
    pub hit: f32,
    /// Seek/volume bar: 0 text color, 1 accent, 2 gradient from accent to accent2.
    pub bar_style: i32,
}

pub const DEFAULT: &str = "wisp-dark";

// -- color math -----------------------------------------------------------------------------

fn lin(v: u8) -> f32 {
    let s = v as f32 / 255.0;
    if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
}

/// WCAG relative luminance.
pub fn luminance(c: Col) -> f32 {
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio (1 to 21).
pub fn contrast(a: Col, b: Col) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `top` composited over an opaque `base`.
pub fn over(top: Col, base: Col) -> Col {
    let k = top.a as f32 / 255.0;
    let mix = |t: u8, b: u8| (t as f32 * k + b as f32 * (1.0 - k)).round() as u8;
    Col { r: mix(top.r, base.r), g: mix(top.g, base.g), b: mix(top.b, base.b), a: 255 }
}

fn mix(a: Col, b: Col, t: f32) -> Col {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Col { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b), a: 255 }
}

fn with_alpha(c: Col, pct: u32) -> Col {
    Col { a: ((pct * 255 + 50) / 100) as u8, ..c }
}

/// The most readable of a few candidates on `bg`.
fn best_on(bg: Col, candidates: &[Col]) -> Col {
    *candidates.iter().max_by(|x, y| contrast(**x, bg).total_cmp(&contrast(**y, bg))).unwrap_or(&WHITE)
}

// -- the table ------------------------------------------------------------------------------

/// Defaults for a theme: structure derived from background, text and accent, and the standard look.
fn base(id: &'static str, name: &'static str, group: Group, dark: bool, bg: u32, text: u32, accent: u32) -> ThemeDef {
    let (bgc, txt, acc) = (c(bg), c(text), c(accent));
    let a = |pct| with_alpha(txt, pct);
    ThemeDef {
        id,
        name,
        group,
        dark,
        follows_art: false,
        dynamic: false,
        backdrop: true,
        bg: bgc,
        bg2: None,
        sidebar: if dark { ca(0x000000, 28) } else { ca(0xffffff, 55) },
        surface: if dark { ca(0xffffff, 7) } else { ca(0xffffff, 70) },
        surface_hi: if dark { ca(0xffffff, 12) } else { ca(0xffffff, 93) },
        bar: with_alpha(if dark { mix(bgc, txt, 0.07) } else { mix(bgc, WHITE, 0.7) }, 94),
        overlay: if dark { mix(bgc, txt, 0.10) } else { mix(bgc, WHITE, 0.85) },
        scrim: with_alpha(bgc, if dark { 78 } else { 86 }),
        text: txt,
        text2: a(70),
        text3: a(44),
        line: a(if dark { 10 } else { 13 }),
        hover: a(if dark { 7 } else { 6 }),
        press: a(if dark { 12 } else { 10 }),
        track: a(if dark { 18 } else { 15 }),
        thumb: a(40),
        seg: if dark { a(18) } else { WHITE },
        knob: WHITE,
        shadow: if dark { ca(0x000000, 40) } else { ca(0x1c2030, 22) },
        danger: if dark { c(0xff6b7a) } else { c(0xc62f3b) },
        tint: CLEAR,
        accent: acc,
        accent2: acc,
        on_accent: None,
        rounding: 1.0,
        circle: 1.0,
        border: 1.0,
        font: "",
        title_font: "",
        weight: 400,
        glow: 0.0,
        gloss: 0.0,
        bevel: false,
        bevel_light: WHITE,
        bevel_dark: c(0x808080),
        neu: 0.0,
        neu_light: WHITE,
        neu_dark: c(0xa3b1c6),
        wash: 0.0,
        hit: 1.0,
        bar_style: 0,
    }
}

fn build() -> Vec<ThemeDef> {
    use Group::*;
    vec![
        // ---- Wisp: the original look, exactly -------------------------------------------------
        ThemeDef {
            follows_art: true,
            wash: 1.0,
            sidebar: ch(0x00000046),
            surface: ch(0xffffff12),
            surface_hi: ch(0xffffff20),
            bar: ch(0x1b1b22ea),
            overlay: ch(0x26262ef5),
            scrim: ch(0x0a0a0ebf),
            text2: ch(0xffffffb2),
            text3: ch(0xffffff68),
            line: ch(0xffffff1a),
            hover: ch(0xffffff14),
            press: ch(0xffffff24),
            track: ch(0xffffff2e),
            thumb: ch(0xffffff66),
            seg: ch(0xffffff2e),
            shadow: ch(0x00000066),
            danger: c(0xff5c6c),
            ..base("wisp-dark", "Wisp Dark", Wisp, true, 0x0a0a0e, 0xffffff, 0xfa2d48)
        },
        ThemeDef {
            follows_art: true,
            wash: 1.0,
            sidebar: ch(0xffffff8c),
            surface: ch(0xffffffb4),
            surface_hi: ch(0xffffffee),
            bar: ch(0xfffffff0),
            overlay: ch(0xfffffff7),
            scrim: ch(0xeeeef3cc),
            text2: ch(0x16161baa),
            text3: ch(0x16161b6a),
            line: ch(0x00000018),
            hover: ch(0x0000000d),
            press: ch(0x0000001a),
            track: ch(0x00000026),
            thumb: ch(0x00000055),
            seg: c(0xffffff),
            shadow: ch(0x00000066),
            ..base("wisp-light", "Wisp Light", Wisp, false, 0xeeeef3, 0x16161b, 0xfa2d48)
        },
        // ---- Developer favorites ---------------------------------------------------------------
        ThemeDef {
            sidebar: c(0x181825),
            surface: c(0x313244),
            surface_hi: c(0x45475a),
            bar: ch(0x181825f5),
            overlay: c(0x313244),
            scrim: with_alpha(c(0x1e1e2e), 82),
            text2: c(0xbac2de),
            text3: c(0x9399b2),
            line: c(0x45475a),
            seg: c(0x45475a),
            accent2: c(0xf5c2e7),
            on_accent: Some(c(0x11111b)),
            rounding: 1.2,
            bar_style: 2,
            wash: 0.35,
            ..base("catppuccin-mocha", "Catppuccin Mocha", Developer, true, 0x1e1e2e, 0xcdd6f4, 0xcba6f7)
        },
        ThemeDef {
            sidebar: c(0xe6e9ef),
            surface: c(0xe6e9ef),
            surface_hi: c(0xdce0e8),
            bar: ch(0xe6e9eff5),
            overlay: c(0xeff1f5),
            scrim: with_alpha(c(0xeff1f5), 88),
            text2: c(0x5c5f77),
            text3: c(0x7c7f93),
            line: c(0xccd0da),
            seg: c(0xffffff),
            accent2: c(0xea76cb),
            on_accent: Some(c(0xeff1f5)),
            rounding: 1.2,
            bar_style: 2,
            wash: 0.2,
            ..base("catppuccin-latte", "Catppuccin Latte", Developer, false, 0xeff1f5, 0x4c4f69, 0x8839ef)
        },
        ThemeDef {
            sidebar: c(0x292e39),
            surface: c(0x3b4252),
            surface_hi: c(0x434c5e),
            bar: ch(0x2b303bf5),
            overlay: c(0x3b4252),
            text2: c(0xd8dee9),
            text3: c(0x98a4ba),
            line: c(0x434c5e),
            seg: c(0x4c566a),
            accent2: c(0x81a1c1),
            on_accent: Some(c(0x2e3440)),
            rounding: 0.5,
            circle: 0.6,
            backdrop: false,
            bar_style: 2,
            shadow: CLEAR,
            ..base("nord", "Nord", Developer, true, 0x2e3440, 0xeceff4, 0x88c0d0)
        },
        ThemeDef {
            sidebar: c(0x1d2021),
            surface: c(0x3c3836),
            surface_hi: c(0x504945),
            bar: ch(0x32302ff5),
            overlay: c(0x3c3836),
            scrim: with_alpha(c(0x282828), 84),
            text2: c(0xd5c4a1),
            text3: c(0xa89984),
            line: c(0x504945),
            seg: c(0x504945),
            tint: ca(0xb57614, 14),
            accent2: c(0xfe8019),
            on_accent: Some(c(0x282828)),
            rounding: 0.4,
            circle: 0.5,
            bar_style: 2,
            ..base("gruvbox-dark", "Gruvbox Dark", Developer, true, 0x282828, 0xebdbb2, 0xd79921)
        },
        ThemeDef {
            sidebar: c(0xebdbb2),
            surface: c(0xf2e5bc),
            surface_hi: c(0xebdbb2),
            bar: ch(0xf2e5bcf5),
            overlay: c(0xfbf1c7),
            scrim: with_alpha(c(0xfbf1c7), 90),
            text2: c(0x504945),
            text3: c(0x7c6f64),
            line: c(0xd5c4a1),
            seg: c(0xfbf1c7),
            tint: ca(0xd79921, 12),
            accent2: c(0xaf3a03),
            rounding: 0.4,
            circle: 0.5,
            bar_style: 2,
            ..base("gruvbox-light", "Gruvbox Light", Developer, false, 0xfbf1c7, 0x3c3836, 0xb57614)
        },
        ThemeDef {
            sidebar: c(0x16161e),
            surface: c(0x24283b),
            surface_hi: c(0x2f3549),
            bar: ch(0x16161ef2),
            overlay: c(0x24283b),
            scrim: with_alpha(c(0x1a1b26), 82),
            text2: c(0xa9b1d6),
            text3: c(0x737aa2),
            line: c(0x2f3549),
            seg: c(0x3b4261),
            accent2: c(0xbb9af7),
            on_accent: Some(c(0x1a1b26)),
            rounding: 1.0,
            glow: 0.9,
            bar_style: 2,
            wash: 0.45,
            ..base("tokyo-night", "Tokyo Night", Developer, true, 0x1a1b26, 0xc0caf5, 0x7aa2f7)
        },
        ThemeDef {
            sidebar: c(0x16141f),
            surface: c(0x1f1d2e),
            surface_hi: c(0x26233a),
            bar: ch(0x1f1d2ef5),
            overlay: c(0x26233a),
            scrim: with_alpha(c(0x191724), 84),
            text2: c(0x908caa),
            text3: c(0x78748f),
            line: ch(0x00000000),
            seg: c(0x403d52),
            accent2: c(0xf6c177),
            on_accent: Some(c(0x191724)),
            rounding: 1.1,
            border: 0.0,
            title_font: "Georgia",
            bar_style: 2,
            wash: 0.3,
            ..base("rose-pine", "Rosé Pine", Developer, true, 0x191724, 0xe0def4, 0xebbcba)
        },
        ThemeDef {
            sidebar: c(0xf2e9e1),
            surface: c(0xfffaf3),
            surface_hi: c(0xf4ede8),
            bar: ch(0xfffaf3f5),
            overlay: c(0xfffaf3),
            scrim: with_alpha(c(0xfaf4ed), 88),
            text2: c(0x625e7a),
            text3: c(0x76728c),
            line: ch(0x00000000),
            seg: c(0xffffff),
            accent: c(0xd7827e),
            accent2: c(0xea9d34),
            rounding: 1.1,
            border: 0.0,
            title_font: "Georgia",
            bar_style: 2,
            wash: 0.2,
            ..base("rose-pine-dawn", "Rosé Pine Dawn", Developer, false, 0xfaf4ed, 0x575279, 0xd7827e)
        },
        ThemeDef {
            sidebar: c(0x21222c),
            surface: c(0x343746),
            surface_hi: c(0x44475a),
            bar: ch(0x21222cf5),
            overlay: c(0x343746),
            scrim: with_alpha(c(0x282a36), 82),
            text2: c(0xc8c8c2),
            text3: c(0x8d9bc4),
            line: c(0x44475a),
            seg: c(0x44475a),
            accent2: c(0xff79c6),
            on_accent: Some(c(0x282a36)),
            bar_style: 2,
            wash: 0.35,
            ..base("dracula", "Dracula", Developer, true, 0x282a36, 0xf8f8f2, 0xbd93f9)
        },
        ThemeDef {
            sidebar: c(0x232a2e),
            surface: c(0x343f44),
            surface_hi: c(0x3d484d),
            bar: ch(0x2d353bf5),
            overlay: c(0x343f44),
            scrim: with_alpha(c(0x2d353b), 90),
            text2: c(0xaeb9a8),
            text3: c(0x859289),
            line: c(0x475258),
            seg: c(0x475258),
            accent2: c(0x83c092),
            on_accent: Some(c(0x2d353b)),
            rounding: 0.9,
            bar_style: 2,
            ..base("everforest", "Everforest", Developer, true, 0x2d353b, 0xd3c6aa, 0xa7c080)
        },
        ThemeDef {
            sidebar: c(0x00212b),
            surface: c(0x073642),
            surface_hi: c(0x0a3a46),
            bar: ch(0x073642f5),
            overlay: c(0x073642),
            text2: c(0x93a1a1),
            text3: c(0x839496),
            line: c(0x174b58),
            seg: c(0x174b58),
            accent2: c(0x2aa198),
            rounding: 0.5,
            circle: 0.6,
            backdrop: false,
            bar_style: 2,
            shadow: CLEAR,
            ..base("solarized-dark", "Solarized Dark", Developer, true, 0x002b36, 0xeee8d5, 0x268bd2)
        },
        ThemeDef {
            sidebar: c(0xeee8d5),
            surface: c(0xf5eedb),
            surface_hi: c(0xeee8d5),
            bar: ch(0xeee8d5f5),
            overlay: c(0xfdf6e3),
            text2: c(0x4f646b),
            text3: c(0x657b83),
            line: c(0xd9d2bd),
            seg: c(0xfdf6e3),
            accent: c(0x268bd2),
            accent2: c(0x2aa198),
            rounding: 0.5,
            circle: 0.6,
            backdrop: false,
            bar_style: 2,
            shadow: CLEAR,
            ..base("solarized-light", "Solarized Light", Developer, false, 0xfdf6e3, 0x073642, 0x268bd2)
        },
        // ---- Retro & nostalgic -----------------------------------------------------------------
        ThemeDef {
            bg2: Some(c(0xe9f8ff)),
            sidebar: ca(0xffffff, 38),
            surface: ca(0xffffff, 46),
            surface_hi: ca(0xffffff, 68),
            bar: ca(0xffffff, 58),
            overlay: c(0xf2fbff),
            text: c(0x0b2e4a),
            text2: c(0x1f4d73),
            text3: c(0x3d6a8c),
            line: ca(0xffffff, 85),
            seg: c(0xffffff),
            shadow: ca(0x0b4f80, 28),
            accent2: c(0x7ed957),
            on_accent: Some(c(0xffffff)),
            rounding: 1.3,
            gloss: 0.6,
            backdrop: false,
            bar_style: 2,
            ..base("frutiger-aero", "Frutiger Aero", Retro, false, 0x5ec3ff, 0x0b2e4a, 0x0a6fc2)
        },
        ThemeDef {
            sidebar: c(0x2b2d30),
            surface: c(0x16201a),
            surface_hi: c(0x1f2e24),
            bar: c(0x2b2d30),
            overlay: c(0x2b2d30),
            text2: c(0x00cc34),
            text3: c(0x0a9a38),
            line: c(0x2d4a35),
            hover: ca(0x00ff41, 10),
            press: ca(0x00ff41, 18),
            track: c(0x050806),
            thumb: c(0x5b5f66),
            seg: c(0x3a3e44),
            knob: c(0xd0d4d0),
            shadow: CLEAR,
            on_accent: Some(c(0x0a0f0d)),
            rounding: 0.0,
            circle: 0.0,
            font: "Consolas",
            backdrop: false,
            bevel: true,
            bevel_light: c(0x5b5f66),
            bevel_dark: c(0x0b0c0d),
            bar_style: 1,
            ..base("winamp-classic", "Winamp Classic", Retro, true, 0x0a0f0d, 0x00ff41, 0x00ff41)
        },
        ThemeDef {
            sidebar: c(0x2b2d30),
            surface: c(0x211a0c),
            surface_hi: c(0x2e2411),
            bar: c(0x2b2d30),
            overlay: c(0x2b2d30),
            text2: c(0xcc8d00),
            text3: c(0x9a6c00),
            line: c(0x4a3a18),
            hover: ca(0xffb000, 10),
            press: ca(0xffb000, 18),
            track: c(0x080603),
            thumb: c(0x5b5f66),
            seg: c(0x3a3e44),
            knob: c(0xd0d4d0),
            shadow: CLEAR,
            on_accent: Some(c(0x0f0b05)),
            rounding: 0.0,
            circle: 0.0,
            font: "Consolas",
            backdrop: false,
            bevel: true,
            bevel_light: c(0x5b5f66),
            bevel_dark: c(0x0b0c0d),
            bar_style: 1,
            ..base("winamp-amber", "Winamp Amber", Retro, true, 0x0f0b05, 0xffb000, 0xffb000)
        },
        ThemeDef {
            bg2: Some(c(0x4a0d67)),
            sidebar: ca(0x0d0340, 55),
            surface: ca(0xff2a85, 10),
            surface_hi: ca(0xff2a85, 18),
            bar: ch(0x1a0a60dd),
            overlay: c(0x24106e),
            text: c(0xf4ecff),
            text2: c(0xcdb8ff),
            text3: c(0x9d85e0),
            line: ca(0x05d9e8, 35),
            seg: ca(0xff2a85, 30),
            accent2: c(0x05d9e8),
            rounding: 1.0,
            glow: 0.9,
            title_font: "Segoe UI Black",
            backdrop: false,
            bar_style: 2,
            ..base("vaporwave", "Vaporwave", Retro, true, 0x120458, 0xf4ecff, 0xff2a85)
        },
        ThemeDef {
            sidebar: c(0xb5c3b5),
            surface: c(0xb5c3b5),
            surface_hi: c(0xa6b6a6),
            bar: c(0xb5c3b5),
            overlay: c(0xc5d1c5),
            text2: c(0x2c3a2c),
            text3: c(0x4a5c4a),
            line: ca(0x1b241b, 45),
            hover: ca(0x1b241b, 10),
            press: ca(0x1b241b, 18),
            track: ca(0x1b241b, 25),
            thumb: ca(0x1b241b, 55),
            seg: c(0xa6b6a6),
            knob: c(0x1b241b),
            shadow: CLEAR,
            accent: c(0x1b241b),
            accent2: c(0x1b241b),
            on_accent: Some(c(0xc5d1c5)),
            rounding: 0.0,
            circle: 0.0,
            font: "Arial",
            backdrop: false,
            bar_style: 1,
            ..base("ipod-lcd", "iPod Classic LCD", Retro, false, 0xc5d1c5, 0x1b241b, 0x1b241b)
        },
        ThemeDef {
            sidebar: c(0x141416),
            surface: c(0x2c2c2e),
            surface_hi: c(0x3a3a3c),
            bar: ch(0x2c2c2ef5),
            overlay: c(0x2c2c2e),
            text: c(0xf2f2f7),
            text2: c(0xb0b0b6),
            text3: c(0x8e8e93),
            line: c(0x3a3a3c),
            seg: c(0x48484a),
            accent2: c(0x4da3ff),
            on_accent: Some(c(0xffffff)),
            rounding: 0.7,
            gloss: 0.3,
            font: "Arial",
            backdrop: false,
            bar_style: 2,
            ..base("ipod-black", "iPod Classic Black", Retro, true, 0x1c1c1e, 0xf2f2f7, 0x1f6fd8)
        },
        ThemeDef {
            bg2: Some(c(0xe6e9ec)),
            sidebar: c(0xc9ced3),
            surface: c(0xeceff1),
            surface_hi: c(0xf6f7f9),
            bar: ch(0xe4e7eaf5),
            overlay: c(0xf1f3f5),
            text2: c(0x464a51),
            text3: c(0x6a7078),
            line: c(0xb3b9c0),
            seg: c(0xffffff),
            accent: c(0x1f6fd8),
            accent2: c(0x1f6fd8),
            on_accent: Some(c(0xffffff)),
            rounding: 0.7,
            gloss: 0.3,
            font: "Arial",
            backdrop: false,
            bar_style: 1,
            shadow: ca(0x303844, 22),
            ..base("ipod-silver", "iPod Classic Silver", Retro, false, 0xd8dce0, 0x1c1c1e, 0x1f6fd8)
        },
        ThemeDef {
            sidebar: c(0xdcdcda),
            surface: c(0xeeeeec),
            surface_hi: c(0xf6f6f4),
            bar: c(0xe6e6e4),
            overlay: c(0xf6f6f4),
            text2: c(0x484846),
            text3: c(0x72726c),
            line: ca(0x222222, 32),
            seg: c(0xffffff),
            shadow: CLEAR,
            rounding: 0.3,
            font: "Consolas",
            backdrop: false,
            bar_style: 1,
            ..base("braun-te", "Teenage Engineering", Retro, false, 0xe6e6e4, 0x222222, 0xff4d00)
        },
        ThemeDef {
            sidebar: c(0xc0c0c0),
            surface: c(0xffffff),
            surface_hi: c(0xe8e8e8),
            bar: c(0xc0c0c0),
            overlay: c(0xc0c0c0),
            text2: c(0x303030),
            text3: c(0x606060),
            line: c(0x808080),
            hover: ca(0x000080, 12),
            press: ca(0x000080, 22),
            track: c(0xffffff),
            thumb: c(0x808080),
            seg: c(0xffffff),
            knob: c(0xc0c0c0),
            shadow: CLEAR,
            accent2: c(0x008080),
            on_accent: Some(c(0xffffff)),
            rounding: 0.0,
            circle: 0.0,
            border: 2.0,
            font: "Microsoft Sans Serif",
            backdrop: false,
            bevel: true,
            bevel_light: c(0xffffff),
            bevel_dark: c(0x808080),
            bar_style: 1,
            ..base("windows-95", "Windows 95", Retro, false, 0xc0c0c0, 0x000000, 0x000080)
        },
        // ---- Modern glass & light --------------------------------------------------------------
        ThemeDef {
            dynamic: true,
            follows_art: true,
            wash: 0.0,
            ..base("chameleon", "Album Chameleon", Modern, true, 0x14141a, 0xffffff, 0xfa2d48)
        },
        ThemeDef {
            bg2: Some(c(0x1c2028)),
            sidebar: ca(0xffffff, 8),
            surface: ca(0xffffff, 12),
            surface_hi: ca(0xffffff, 20),
            bar: ca(0xffffff, 14),
            overlay: c(0x2a2d36),
            scrim: with_alpha(c(0x101216), 62),
            text2: ca(0xffffff, 78),
            text3: ca(0xffffff, 52),
            line: ca(0xffffff, 30),
            seg: ca(0xffffff, 24),
            shadow: ca(0x000000, 45),
            follows_art: true,
            rounding: 1.6,
            glow: 0.25,
            ..base("liquid-glass", "Liquid Glass", Modern, true, 0x101216, 0xffffff, 0x8ab4ff)
        },
        ThemeDef {
            bg2: Some(c(0xf7f9fc)),
            sidebar: ca(0xffffff, 40),
            surface: ca(0xffffff, 52),
            surface_hi: ca(0xffffff, 80),
            bar: ca(0xffffff, 60),
            overlay: c(0xf4f6fa),
            scrim: with_alpha(c(0xe9edf3), 70),
            text: c(0x14161a),
            text2: ca(0x14161a, 76),
            text3: ca(0x14161a, 56),
            line: ca(0xffffff, 90),
            seg: c(0xffffff),
            shadow: ca(0x303a50, 24),
            follows_art: true,
            rounding: 1.6,
            ..base("liquid-glass-light", "Liquid Glass Light", Modern, false, 0xe9edf3, 0x14161a, 0x0a84ff)
        },
        ThemeDef {
            sidebar: c(0x000000),
            surface: c(0x0d0d0d),
            surface_hi: c(0x161616),
            bar: c(0x0d0d0d),
            overlay: c(0x121212),
            text2: c(0xb3b3b3),
            text3: c(0x7a7a7a),
            line: c(0x1f1f1f),
            hover: c(0x141414),
            press: c(0x1f1f1f),
            track: c(0x262626),
            thumb: c(0x5a5a5a),
            seg: c(0x242424),
            shadow: CLEAR,
            on_accent: Some(c(0x000000)),
            backdrop: false,
            ..base("oled-black", "OLED Pitch Black", Modern, true, 0x000000, 0xffffff, 0xffffff)
        },
        ThemeDef {
            sidebar: c(0xf7f5f0),
            surface: c(0xefede6),
            surface_hi: c(0xe6e3da),
            bar: c(0xf7f5f0),
            overlay: c(0xf7f5f0),
            text2: c(0x3d3d3a),
            text3: c(0x6e6c66),
            line: ca(0x111111, 28),
            seg: c(0xffffff),
            shadow: CLEAR,
            on_accent: Some(c(0xffffff)),
            rounding: 0.1,
            circle: 0.0,
            font: "Arial",
            title_font: "Arial Black",
            backdrop: false,
            bar_style: 1,
            ..base("swiss-paper", "Swiss Editorial", Modern, false, 0xf7f5f0, 0x111111, 0xd62839)
        },
        ThemeDef {
            sidebar: c(0xe0e5ec),
            surface: c(0xe0e5ec),
            surface_hi: c(0xd3d9e2),
            bar: c(0xe0e5ec),
            overlay: c(0xe0e5ec),
            text2: c(0x4d586f),
            text3: c(0x687489),
            line: CLEAR,
            seg: c(0xe0e5ec),
            shadow: CLEAR,
            accent: c(0x4a67ec),
            on_accent: Some(c(0xffffff)),
            rounding: 1.8,
            border: 0.0,
            backdrop: false,
            neu: 1.0,
            neu_light: c(0xffffff),
            neu_dark: c(0xa3b1c6),
            bar_style: 1,
            ..base("neumorphism-light", "Neumorphism Light", Modern, false, 0xe0e5ec, 0x3f4a5c, 0x4a67ec)
        },
        ThemeDef {
            sidebar: c(0x26292e),
            surface: c(0x26292e),
            surface_hi: c(0x30343a),
            bar: c(0x26292e),
            overlay: c(0x2c3037),
            text2: c(0xa9b1bd),
            text3: c(0x8b94a1),
            line: CLEAR,
            seg: c(0x26292e),
            shadow: CLEAR,
            on_accent: Some(c(0x15171a)),
            rounding: 1.8,
            border: 0.0,
            backdrop: false,
            neu: 1.0,
            neu_light: c(0x32363d),
            neu_dark: c(0x16181b),
            bar_style: 1,
            ..base("neumorphism-dark", "Neumorphism Dark", Modern, true, 0x26292e, 0xdde2ea, 0x7f9bff)
        },
        // ---- Moods & atmospheres ---------------------------------------------------------------
        ThemeDef {
            sidebar: c(0x1a1411),
            surface: c(0x2a211c),
            surface_hi: c(0x352a24),
            bar: ch(0x2a211cf5),
            overlay: c(0x2a211c),
            scrim: with_alpha(c(0x1f1916), 80),
            text2: c(0xc8b6a6),
            text3: c(0x9d8a79),
            line: c(0x46372e),
            seg: c(0x46372e),
            tint: ca(0xf4a261, 10),
            accent2: c(0xf4a261),
            on_accent: Some(c(0x1f1916)),
            rounding: 1.3,
            bar_style: 2,
            wash: 0.4,
            ..base("lofi-cafe", "Lo-Fi Cafe", Mood, true, 0x1f1916, 0xede0d4, 0xc87d55)
        },
        ThemeDef {
            sidebar: c(0x08090c),
            surface: c(0x14161d),
            surface_hi: c(0x1d2029),
            bar: ch(0x0e1015f5),
            overlay: c(0x14161d),
            text: c(0xf0f0f0),
            text2: c(0xaeb3bf),
            text3: c(0x7a8090),
            line: ca(0xfcee0a, 24),
            seg: c(0x2a2d38),
            danger: c(0xff003c),
            accent2: c(0x00f0ff),
            on_accent: Some(c(0x0b0c10)),
            rounding: 0.0,
            circle: 0.0,
            glow: 0.5,
            font: "Bahnschrift",
            backdrop: false,
            bar_style: 2,
            shadow: CLEAR,
            ..base("cyberpunk", "Cyberpunk", Mood, true, 0x0b0c10, 0xf0f0f0, 0xfcee0a)
        },
        ThemeDef {
            bg2: Some(c(0x3a0f4f)),
            sidebar: ca(0x0a0414, 50),
            surface: ca(0xff2e93, 9),
            surface_hi: ca(0xff2e93, 17),
            bar: ch(0x1a0d2cdd),
            overlay: c(0x2a1440),
            text: c(0xf6ecff),
            text2: c(0xd2b8ec),
            text3: c(0x9a7dba),
            line: ca(0xff2e93, 32),
            seg: ca(0xff2e93, 30),
            accent2: c(0x38b6ff),
            rounding: 0.8,
            glow: 0.9,
            backdrop: false,
            bar_style: 2,
            ..base("synthwave", "Synthwave Midnight", Mood, true, 0x13091f, 0xf6ecff, 0xff2e93)
        },
        ThemeDef {
            sidebar: c(0x101812),
            surface: c(0x1c2a21),
            surface_hi: c(0x25362a),
            bar: ch(0x1c2a21f5),
            overlay: c(0x1c2a21),
            scrim: with_alpha(c(0x141e18), 80),
            text2: c(0xc4cfa6),
            text3: c(0x8e9c78),
            line: c(0x2f4535),
            seg: c(0x2f4535),
            accent2: c(0xecf39e),
            on_accent: Some(c(0x141e18)),
            rounding: 1.5,
            bar_style: 2,
            wash: 0.3,
            ..base("forest-moss", "Forest Moss", Mood, true, 0x141e18, 0xeef2d2, 0x90a955)
        },
        ThemeDef {
            sidebar: c(0x000000),
            surface: c(0x000000),
            surface_hi: c(0x1a1a1a),
            bar: c(0x000000),
            overlay: c(0x000000),
            scrim: c(0x000000),
            text2: c(0xf2f2f2),
            text3: c(0xcccccc),
            line: c(0xffffff),
            hover: c(0x2b2b2b),
            press: c(0x404040),
            track: c(0x666666),
            thumb: c(0xffffff),
            seg: c(0x333333),
            shadow: CLEAR,
            danger: c(0xff6b6b),
            on_accent: Some(c(0x000000)),
            rounding: 0.5,
            circle: 0.5,
            border: 2.0,
            weight: 600,
            hit: 1.2,
            backdrop: false,
            bar_style: 1,
            ..base("high-contrast", "High Contrast", Mood, true, 0x000000, 0xffffff, 0xffd60a)
        },
    ]
}

fn table() -> &'static [ThemeDef] {
    static TABLE: OnceLock<Vec<ThemeDef>> = OnceLock::new();
    TABLE.get_or_init(build)
}

pub fn all() -> &'static [ThemeDef] {
    table()
}

/// The theme with this id; the default for an unknown (or empty) id.
pub fn find(id: &str) -> &'static ThemeDef {
    let t = table();
    t.iter().find(|d| d.id == id).unwrap_or_else(|| t.iter().find(|d| d.id == DEFAULT).expect("the default theme exists"))
}

/// The theme id for settings written before named themes existed (`theme=0` dark, `1` light).
pub fn legacy_id(old: i32) -> &'static str {
    if old == 1 { "wisp-light" } else { DEFAULT }
}

// -- Album Chameleon ------------------------------------------------------------------------

/// Average brightness (0 to 1) of an image, for choosing between a light and a dark scheme.
pub fn mean_luma(img: &art::Rgb) -> f32 {
    let n = (img.px.len() / 3).max(1);
    let sum: f32 = img.px.chunks_exact(3).map(|p| luminance(Col { r: p[0], g: p[1], b: p[2], a: 255 })).sum();
    sum / n as f32
}

/// The Album Chameleon palette for artwork with this dominant color and brightness: the
/// artwork's hue tints the background, and the text is light or dark by the artwork's brightness.
/// The scrim over the blurred artwork is strong enough for either text to stay readable on any cover.
pub fn chameleon(hsv: Option<(f32, f32, f32)>, luma: f32) -> ThemeDef {
    let proto = find("chameleon");
    let Some((h, s, _)) = hsv else { return proto.clone() };
    let light = luma > 0.30;
    let tinted = |sat: f32, val: f32| {
        let [r, g, b] = art::from_hsv(h, sat * s.max(0.5), val);
        (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
    };
    let (bg, text, accent) = if light { (tinted(0.22, 0.97), 0x15151c, 0) } else { (tinted(0.9, 0.16), 0xffffff, 0) };
    let mut d = base("chameleon", "Album Chameleon", Group::Modern, !light, bg, text, accent);
    d.dynamic = true;
    d.follows_art = true;
    d.accent = proto.accent;
    d.accent2 = proto.accent;
    d.scrim = with_alpha(c(bg), 66);
    d.sidebar = if light { ca(0xffffff, 42) } else { ca(0x000000, 30) };
    d.surface = if light { ca(0xffffff, 55) } else { ca(0xffffff, 10) };
    d.surface_hi = if light { ca(0xffffff, 82) } else { ca(0xffffff, 17) };
    d.bar = if light { ca(0xffffff, 62) } else { ca(0x000000, 42) };
    d.line = if light { ca(0xffffff, 75) } else { ca(0xffffff, 20) };
    d.text2 = with_alpha(d.text, 76);
    d.text3 = with_alpha(d.text, 56);
    d.rounding = 1.4;
    d
}

// -- applying -------------------------------------------------------------------------------

/// The accent a theme takes from the playing artwork: vivid enough to read, light on dark
/// themes and deep on light ones (Album Chameleon goes deeper still, as its background is
/// tinted rather than neutral). A hue that is dark by nature, like pure blue on a dark window,
/// is then lightened (or deepened, on a light theme) until it stands out from the surfaces.
pub fn artwork_accent(d: &ThemeDef, hsv: (f32, f32, f32)) -> Col {
    let (h, s, v) = hsv;
    let [r, g, b] = if d.dark {
        art::from_hsv(h, s.max(0.6), v.max(0.85))
    } else if d.dynamic {
        art::from_hsv(h, s.max(0.6), v.clamp(0.35, 0.6))
    } else {
        art::from_hsv(h, s.max(0.6), v.clamp(0.5, 0.78))
    };
    legible(Col { r, g, b, a: 255 }, d, &grounds(d), 3.0)
}

/// The surfaces the accent is drawn on: the window (both ends of a gradient) and the cards on it.
fn grounds(d: &ThemeDef) -> Vec<Col> {
    [c_of(d.bg), c_of(d.bg2.unwrap_or(d.bg))].into_iter().flat_map(|w| [w, over(d.surface, w)]).collect()
}

/// `accent` moved toward white (on a dark theme) or black until it has at least `min` contrast on
/// every one of `grounds`; unchanged when it already does.
fn legible(accent: Col, d: &ThemeDef, grounds: &[Col], min: f32) -> Col {
    let toward = if d.dark { WHITE } else { BLACK };
    (0..=10)
        .map(|i| mix(accent, toward, i as f32 / 10.0))
        .find(|c| grounds.iter().all(|g| contrast(*c, *g) >= min))
        .unwrap_or(toward)
}

/// The accent as the color of small text (the playing track, an artist's name): the accent itself
/// when it reads well on the window, the cards and a selected row, else nudged toward the text.
pub fn accent_text(d: &ThemeDef, accent: Col) -> Col {
    let mut on = grounds(d);
    on.extend(on.clone().into_iter().map(|g| over(with_alpha(accent, 18), g)));
    legible(accent, d, &on, 4.5)
}

fn color(c: Col) -> slint::Color {
    slint::Color::from_argb_u8(c.a, c.r, c.g, c.b)
}

/// Text on an accent-filled control: readable on the accent whatever the accent is.
pub fn on_accent(d: &ThemeDef, accent: Col) -> Col {
    d.on_accent.filter(|o| contrast(*o, accent) >= 4.5).unwrap_or_else(|| best_on(accent, &[c_of(d.bg), d.text, BLACK, WHITE]))
}

fn c_of(c: Col) -> Col {
    Col { a: 255, ..c }
}

/// Copies `d` into the UI. `accent` and `accent2` are the colors in effect (the theme's own, the
/// artwork's, or the user's choice); `backdrop` is the user's Ambient backdrop setting.
pub fn write(t: &crate::Theme<'_>, d: &ThemeDef, accent: Col, accent2: Col, backdrop: bool) {
    t.set_backdrop_on(backdrop && d.backdrop);
    t.set_accent(color(accent));
    t.set_accent2(color(accent2));
    t.set_on_accent(color(on_accent(d, accent)));
    t.set_accent_text(color(accent_text(d, accent)));
    t.set_bg(color(d.bg));
    t.set_bg2(color(d.bg2.unwrap_or(d.bg)));
    t.set_bg_gradient(d.bg2.is_some());
    t.set_sidebar(color(d.sidebar));
    t.set_surface(color(d.surface));
    t.set_surface_hi(color(d.surface_hi));
    t.set_bar(color(d.bar));
    t.set_overlay(color(d.overlay));
    t.set_scrim(color(d.scrim));
    t.set_text(color(d.text));
    t.set_text_2(color(d.text2));
    t.set_text_3(color(d.text3));
    t.set_line(color(d.line));
    t.set_hover(color(d.hover));
    t.set_press(color(d.press));
    t.set_track(color(d.track));
    t.set_thumb(color(d.thumb));
    t.set_seg(color(d.seg));
    t.set_knob(color(d.knob));
    t.set_shadow(color(d.shadow));
    t.set_danger(color(d.danger));
    t.set_tint(color(d.tint));
    t.set_rounding(d.rounding);
    t.set_circle(d.circle);
    t.set_border_w(d.border);
    t.set_font(d.font.into());
    t.set_title_font(if d.title_font.is_empty() { d.font } else { d.title_font }.into());
    t.set_weight(d.weight);
    t.set_glow(d.glow);
    t.set_gloss(d.gloss);
    t.set_bevel(d.bevel);
    t.set_bevel_light(color(d.bevel_light));
    t.set_bevel_dark(color(d.bevel_dark));
    t.set_neu(d.neu);
    t.set_neu_light(color(d.neu_light));
    t.set_neu_dark(color(d.neu_dark));
    t.set_wash(d.wash);
    t.set_hit(d.hit);
    t.set_bar_style(d.bar_style);
}

/// What a theme looks like in the picker: window, sidebar, card, text and accent, all opaque.
pub struct Preview {
    pub bg: Col,
    pub panel: Col,
    pub card: Col,
    pub text: Col,
    pub accent: Col,
}

pub fn preview(d: &ThemeDef) -> Preview {
    let bg = c_of(d.bg);
    Preview { bg, panel: over(d.sidebar, bg), card: over(d.surface, over(d.sidebar, bg)), text: d.text, accent: d.accent }
}

/// The themes by group, as the picker shows them.
pub fn groups() -> ModelRc<crate::ThemeGroup> {
    let groups: Vec<crate::ThemeGroup> = Group::ALL
        .iter()
        .map(|&g| {
            let cards: Vec<crate::ThemeCard> = all()
                .iter()
                .filter(|d| d.group == g)
                .map(|d| {
                    let p = preview(d);
                    crate::ThemeCard {
                        id: d.id.into(),
                        name: d.name.into(),
                        bg: color(p.bg),
                        panel: color(p.panel),
                        card: color(p.card),
                        text: color(p.text),
                        accent: color(p.accent),
                    }
                })
                .collect();
            crate::ThemeGroup { title: g.title().into(), themes: ModelRc::new(VecModel::from(cards)) }
        })
        .collect();
    ModelRc::new(VecModel::from(groups))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eff(top: Col, base: Col) -> Col {
        over(top, base)
    }

    #[test]
    fn ids_are_unique_the_default_exists_and_every_group_has_themes() {
        let all = all();
        for (i, a) in all.iter().enumerate() {
            assert!(all[i + 1..].iter().all(|b| b.id != a.id), "duplicate id {}", a.id);
            assert!(a.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "odd id {}", a.id);
        }
        assert_eq!(find(DEFAULT).id, DEFAULT);
        assert_eq!(find("no-such-theme").id, DEFAULT);
        assert_eq!(find("").id, DEFAULT);
        for g in Group::ALL {
            assert!(all.iter().any(|d| d.group == g), "{g:?} is empty");
        }
        assert_eq!(legacy_id(0), "wisp-dark");
        assert_eq!(legacy_id(1), "wisp-light");
        assert!(all.len() >= 30, "{} themes", all.len());
    }

    /// Every theme must be legible, whatever its palette: text on every surface it is drawn on,
    /// secondary text, controls and the label on accent-filled buttons.
    #[test]
    fn every_theme_is_legible() {
        let mut problems = Vec::new();
        for d in all() {
            check(d, d.accent, &mut problems);
        }
        assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    }

    fn check(d: &ThemeDef, accent: Col, problems: &mut Vec<String>) {
        let bg = c_of(d.bg);
        let bg_bottom = c_of(d.bg2.unwrap_or(d.bg));
        let high = d.id == "high-contrast";
        // The surfaces text sits on, as the user sees them (translucent layers over the window).
        for (name, base) in [("top", bg), ("bottom", bg_bottom)] {
            let side = eff(d.sidebar, base);
            let card = eff(d.surface, side);
            let surfaces = [
                ("window", base),
                ("sidebar", side),
                ("card", eff(d.surface, base)),
                ("raised card", eff(d.surface_hi, base)),
                ("player bar", eff(d.bar, base)),
                ("menu", eff(d.overlay, base)),
                ("card on sidebar", card),
            ];
            for (what, s) in surfaces {
                for (label, fg, floor, floor_high) in
                    [("text", d.text, 4.5, 14.0), ("secondary text", d.text2, 4.5, 12.0), ("tertiary text", d.text3, 2.6, 7.0)]
                {
                    let need = if high { floor_high } else { floor };
                    let got = contrast(eff(fg, s), s);
                    if got < need && !(label == "tertiary text" && what.starts_with("card on")) {
                        problems.push(format!("{}: {label} on the {what} ({name}) is {got:.2}:1, wants {need:.1}", d.id));
                    }
                }
            }
            // The accent as an icon/outline color, and as small text, on the plain window and on cards.
            let as_text = accent_text(d, accent);
            for (what, s) in [("window", base), ("card", eff(d.surface, base))] {
                let got = contrast(eff(accent, s), s);
                if got < 2.4 {
                    problems.push(format!("{}: accent on the {what} ({name}) is {got:.1}:1", d.id));
                }
                let got = contrast(as_text, s);
                if got < 4.5 {
                    problems.push(format!("{}: accent text on the {what} ({name}) is {got:.2}:1", d.id));
                }
            }
        }
        let on = on_accent(d, accent);
        let got = contrast(on, accent);
        if got < 4.4 {
            problems.push(format!("{}: label on accent buttons is {got:.1}:1", d.id));
        }
        // A label color written into the table must be one that is actually used.
        if let Some(own) = d.on_accent.filter(|_| accent == d.accent) {
            if contrast(own, accent) < 4.5 {
                problems.push(format!("{}: its own accent label is only {:.1}:1 and is ignored", d.id, contrast(own, accent)));
            }
        }
    }

    /// With a backdrop, the scrim has to keep text readable on any artwork, from black to white.
    #[test]
    fn text_stays_readable_over_any_artwork() {
        let mut problems = Vec::new();
        for d in all().iter().filter(|d| d.backdrop) {
            for art in [BLACK, WHITE, c(0x808080), c(0xff0000), c(0x00ff00), c(0xffff00), c(0x0000ff)] {
                let under = eff(d.scrim, art);
                for (label, fg, floor) in [("text", d.text, 4.5), ("secondary text", d.text2, 3.0)] {
                    let got = contrast(eff(fg, under), under);
                    if got < floor {
                        problems.push(format!("{}: {label} over a {:02x}{:02x}{:02x} cover is {got:.1}:1", d.id, art.r, art.g, art.b));
                    }
                }
            }
        }
        assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    }

    /// Album Chameleon over a sweep of cover colors, bright and dark, vivid and gray.
    #[test]
    fn the_chameleon_is_legible_for_any_cover() {
        let mut problems = Vec::new();
        for hue in (0..360).step_by(30) {
            for sat in [0.0, 0.35, 1.0] {
                for val in [0.1, 0.5, 0.95] {
                    let [r, g, b] = art::from_hsv(hue as f32, sat, val);
                    let luma = luminance(Col { r, g, b, a: 255 });
                    let d = chameleon(Some((hue as f32, sat, val)), luma);
                    let mut found = Vec::new();
                    check(&d, artwork_accent(&d, (hue as f32, sat, val)), &mut found);
                    for art in [BLACK, WHITE, c((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))] {
                        let under = eff(d.scrim, art);
                        for (label, fg, floor) in [("text", d.text, 4.5), ("secondary text", d.text2, 3.0)] {
                            let got = contrast(eff(fg, under), under);
                            if got < floor {
                                found.push(format!("{label} over a {:02x}{:02x}{:02x} cover is {got:.2}:1", art.r, art.g, art.b));
                            }
                        }
                    }
                    problems.extend(found.into_iter().map(|p| format!("h{hue} s{sat} v{val} (luma {luma:.2}): {p}")));
                }
            }
        }
        assert!(problems.is_empty(), "{} problems, e.g.\n{}", problems.len(), problems[..problems.len().min(30)].join("\n"));
        // Light covers get dark text, dark covers light text.
        assert!(!chameleon(Some((200.0, 0.2, 0.98)), 0.9).dark);
        assert!(chameleon(Some((200.0, 0.9, 0.2)), 0.02).dark);
        assert_eq!(chameleon(None, 0.5).id, "chameleon");
    }

    #[test]
    fn contrast_math_matches_the_wcag_reference_values() {
        assert!((contrast(BLACK, WHITE) - 21.0).abs() < 0.01);
        assert!((contrast(WHITE, WHITE) - 1.0).abs() < 0.001);
        assert!((contrast(c(0x777777), WHITE) - 4.48).abs() < 0.05);
        assert_eq!(over(ca(0xffffff, 50), BLACK), c(0x808080));
    }

    #[test]
    fn the_label_on_any_accent_is_readable() {
        let d = find("wisp-dark");
        for accent in [0xfa2d48, 0xff7a3d, 0xf2b632, 0x2fc467, 0x2fb5c8, 0x4f7cff, 0x9b5cf0, 0xff5c93, 0xffffff, 0x000000] {
            let a = c(accent);
            assert!(contrast(on_accent(d, a), a) >= 4.4, "{accent:06x}");
        }
    }
}
