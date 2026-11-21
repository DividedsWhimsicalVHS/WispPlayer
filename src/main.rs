// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(feature = "screenshot")]
    if let Some(code) = wisp::screenshot_cli() {
        std::process::exit(code);
    }
    if let Err(e) = wisp::run() {
        eprintln!("wisp: {e}");
        std::process::exit(1);
    }
}
