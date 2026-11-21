//! Background loading of artwork and lyrics, off the UI thread.
//!
//! One worker thread. Now-playing work always goes first, then the album detail cover, then grid
//! thumbnails *newest first* (what the user scrolled to last matters most), and a backlog from a
//! fast scroll is trimmed instead of being worked off.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use crate::art::{self, Rgb};
use crate::lyrics::{self, Lyrics};

pub const THUMB: u32 = 192;
pub const DETAIL: u32 = 336;
pub const NOW: u32 = 560;
const MAX_PENDING_THUMBS: usize = 64;

/// `cover` is the cover the user saved in Wisp for a file that cannot hold one itself ("" when none).
pub enum Job {
    /// Album grid thumbnail.
    Thumb { index: u32, path: PathBuf, cover: String },
    /// Cover for the album detail header.
    Detail { index: u32, path: PathBuf, cover: String },
    /// Everything the now-playing screen needs for one queue entry. `lyrics` are the ones saved in
    /// Wisp for it, if any; they stand in for the ones in the file itself.
    Now { uid: u32, path: PathBuf, cover: String, lyrics: Option<String> },
}

pub struct NowAssets {
    pub art: Option<Rgb>,
    pub backdrop: Option<Rgb>,
    pub hsv: Option<(f32, f32, f32)>,
    pub lyrics: Option<Lyrics>,
}

pub enum Done {
    Thumb { index: u32, art: Option<Rgb> },
    Detail { index: u32, art: Option<Rgb> },
    Now { uid: u32, assets: NowAssets },
}

pub struct Loader {
    tx: Sender<Job>,
}

impl Loader {
    pub fn spawn(deliver: impl Fn(Done) + Send + 'static) -> Loader {
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("wisp-loader".into())
            .spawn(move || run(rx, deliver))
            .expect("failed to start the loader thread");
        Loader { tx }
    }

    pub fn request(&self, job: Job) {
        let _ = self.tx.send(job);
    }
}

fn cover(path: &Path, saved: &str, max: u32) -> (Option<Rgb>, Option<String>) {
    let embedded = art::read_embedded(path);
    let lyrics = embedded.lyrics.clone();
    let rgb = art::cover_with_fallback(path, embedded.cover, saved).and_then(|bytes| art::decode(&bytes, max));
    (rgb, lyrics)
}

fn process(job: Job) -> Done {
    match job {
        Job::Thumb { index, path, cover: c } => Done::Thumb { index, art: cover(&path, &c, THUMB).0 },
        Job::Detail { index, path, cover: c } => Done::Detail { index, art: cover(&path, &c, DETAIL).0 },
        Job::Now { uid, path, cover: c, lyrics: saved } => {
            let (art, tag_lyrics) = cover(&path, &c, NOW);
            let lyrics = lyrics::load(&path, saved.as_deref().or(tag_lyrics.as_deref()));
            let (backdrop, hsv) = match &art {
                Some(a) => (Some(art::backdrop(a)), art::dominant_hsv(a)),
                None => (None, None),
            };
            Done::Now { uid, assets: NowAssets { art, backdrop, hsv, lyrics } }
        }
    }
}

fn empty_result(job: &Job) -> Done {
    match job {
        Job::Thumb { index, .. } => Done::Thumb { index: *index, art: None },
        Job::Detail { index, .. } => Done::Detail { index: *index, art: None },
        Job::Now { uid, .. } => Done::Now { uid: *uid, assets: NowAssets { art: None, backdrop: None, hsv: None, lyrics: None } },
    }
}

fn run(rx: Receiver<Job>, deliver: impl Fn(Done)) {
    let mut now: Option<Job> = None;
    let mut detail: Vec<Job> = Vec::new();
    let mut thumbs: Vec<Job> = Vec::new();
    loop {
        if now.is_none() && detail.is_empty() && thumbs.is_empty() {
            match rx.recv() {
                Ok(job) => file(job, &mut now, &mut detail, &mut thumbs),
                Err(_) => return,
            }
        }
        while let Ok(job) = rx.try_recv() {
            file(job, &mut now, &mut detail, &mut thumbs);
        }
        if thumbs.len() > MAX_PENDING_THUMBS {
            let excess = thumbs.len() - MAX_PENDING_THUMBS;
            thumbs.drain(..excess);
        }
        let job = now.take().or_else(|| detail.pop()).or_else(|| thumbs.pop());
        let Some(job) = job else { continue };
        let fallback = empty_result(&job);
        // A decoder panicking on a hostile image must not take the loader down.
        let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process(job))).unwrap_or(fallback);
        deliver(done);
    }
}

fn file(job: Job, now: &mut Option<Job>, detail: &mut Vec<Job>, thumbs: &mut Vec<Job>) {
    match job {
        Job::Now { .. } => *now = Some(job), // only the newest matters
        Job::Detail { .. } => {
            detail.clear();
            detail.push(job);
        }
        Job::Thumb { .. } => thumbs.push(job),
    }
}
