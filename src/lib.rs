//! Wisp Player: a featherweight, gapless audio player for local files.
//!
//! The program is this library plus thin launchers: `src/main.rs` for computers, and the `android`
//! crate, whose `android_main` hands the Android activity to the same [`run`].

mod app;
mod art;
mod edit;
mod library;
mod loader;
mod lookup;
mod lyrics;
mod models;
mod player;
mod playlists;
mod settings;
mod themes;
#[cfg(feature = "screenshot")]
mod demo;
#[cfg(feature = "screenshot")]
mod shot;

slint::include_modules!();

/// Opens the window and runs the player until the window is closed.
pub fn run() -> Result<(), String> {
    app::run()
}

/// `wisp --screenshot out.png [--scene songs|albums|artists|queue|settings|detail|unidentified|editor|np|np-queue|empty|toast]
/// [--theme wisp-dark|catppuccin-mocha|...] [--light] [--size 1180x760]` renders the UI headlessly
/// with demo data (`--light` is short for `--theme wisp-light`). `None` when the arguments don't ask
/// for a screenshot.
#[cfg(feature = "screenshot")]
pub fn screenshot_cli() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    let i = args.iter().position(|a| a == "--screenshot")?;
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let out = args.get(i + 1).cloned().unwrap_or_else(|| "shot.png".into());
    let scene = value("--scene").unwrap_or_else(|| "songs".into());
    let size = value("--size")
        .and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))))
        .unwrap_or((1180u32, 760u32));
    let theme = value("--theme").unwrap_or_else(|| if args.iter().any(|a| a == "--light") { "wisp-light" } else { themes::DEFAULT }.into());
    Some(match shot::run(std::path::Path::new(&out), &scene, &theme, size.0, size.1) {
        Ok(()) => {
            println!("wrote {out}");
            0
        }
        Err(e) => {
            eprintln!("screenshot failed: {e}");
            1
        }
    })
}
