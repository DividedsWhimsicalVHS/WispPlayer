//! The seam between the player and the phone it runs on. The Android launcher (the `android` crate)
//! installs the hooks and forwards what happens outside the window; on computers nothing is installed
//! and every call here does nothing.

use std::path::PathBuf;
use std::sync::OnceLock;

pub use crate::app::{media_action, rescan};

/// The song the media notification shows.
pub struct NowPlaying<'a> {
    /// Empty when nothing is loaded any more.
    pub title: &'a str,
    pub artist: &'a str,
    pub playing: bool,
}

/// A button pressed outside the window: the notification, the lock screen, a headset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaAction {
    Play,
    Pause,
    Next,
    Prev,
}

/// What the player asks of the phone.
#[derive(Clone, Copy)]
pub struct Hooks {
    /// The playing song, or whether it is playing, changed.
    pub now_playing: fn(&NowPlaying),
    /// Back was pressed on the first screen: move the app out of sight, but keep it (and the music)
    /// running.
    pub leave: fn(),
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Installs the hooks. The first call wins; the hooks stay for the life of the process.
pub fn install(hooks: Hooks) {
    let _ = HOOKS.set(hooks);
}

/// Where the library index and the settings live, when the platform picks the folder (Android's
/// private app folder). The first call wins.
pub fn set_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

pub(crate) fn data_dir() -> Option<PathBuf> {
    DATA_DIR.get().cloned()
}

pub(crate) fn now_playing(now: &NowPlaying) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.now_playing)(now);
    }
}

pub(crate) fn leave() {
    if let Some(hooks) = HOOKS.get() {
        (hooks.leave)();
    }
}
