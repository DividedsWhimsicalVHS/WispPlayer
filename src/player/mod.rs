//! The audio engine: gapless playback over a lock-free real-time path.
//!
//! ```text
//!  UI thread ──commands──▶ engine thread ──chunks (lock-free)──▶ audio callback ──▶ device
//!      ▲                    decode · mix · resample                 fades · volume · RG
//!      └──── atomics: position, state, volume ◀────────────────────────┘
//! ```

mod decode;
mod engine;
mod mixdown;
pub mod output;
mod pipe;
pub mod queue;
mod replaygain;
mod resample;

use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

pub use decode::{can_decode, open_format, track_duration_ms};
pub use engine::{Event, Resolver};
pub use output::list_devices;
pub use pipe::RG_OFF;
pub use queue::Repeat;

use engine::{Cmd, Engine};
use pipe::Shared;
use queue::Queue;

/// Audio preferences that are applied immediately and persisted by the app.
#[derive(Clone, Debug)]
pub struct AudioSettings {
    /// Output device id; `None` = system default.
    pub device: Option<String>,
    pub volume: f32,
    pub muted: bool,
    /// `RG_OFF`, `RG_TRACK` or `RG_ALBUM`.
    pub normalization: u8,
    pub preamp_db: f32,
    pub prevent_clipping: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        AudioSettings {
            device: None,
            volume: 0.8,
            muted: false,
            normalization: RG_OFF,
            preamp_db: 0.0,
            prevent_clipping: true,
        }
    }
}

/// What is audible right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Now {
    /// Queue entry uid; 0 = nothing.
    pub uid: u32,
    pub pos_ms: u32,
}

pub struct Player {
    tx: Sender<Cmd>,
    shared: Arc<Shared>,
    queue: Arc<Mutex<Queue>>,
    events: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
}

impl Player {
    /// Starts the engine thread and opens the output device.
    pub fn start(resolve: Resolver, settings: &AudioSettings) -> Player {
        Player::with_backend(resolve, settings, |shared| Box::new(output::Cpal::new(shared)))
    }

    pub(crate) fn with_backend(
        resolve: Resolver,
        settings: &AudioSettings,
        make_backend: impl FnOnce(Arc<Shared>) -> Box<dyn output::Backend> + Send + 'static,
    ) -> Player {
        let shared = Shared::new();
        shared.set_volume(settings.volume);
        shared.muted.store(settings.muted, Relaxed);
        shared.rg_mode.store(settings.normalization, Relaxed);
        shared.set_preamp_db(settings.preamp_db);
        shared.prevent_clip.store(settings.prevent_clipping, Relaxed);

        let queue = Arc::new(Mutex::new(Queue::new()));
        let (tx, rx) = channel();
        let (etx, events) = channel();
        let (sh, q, device) = (shared.clone(), queue.clone(), settings.device.clone());
        let thread = std::thread::Builder::new()
            .name("wisp-engine".into())
            .spawn(move || {
                let backend = make_backend(sh.clone());
                Engine::new(rx, etx, sh, q, resolve, backend, device).run();
            })
            .expect("failed to start the audio engine thread");
        Player { tx, shared, queue, events, thread: Some(thread) }
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    // -- queue ------------------------------------------------------------------------------

    pub fn queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The queue itself, for UI models that read it lazily.
    pub fn queue_arc(&self) -> Arc<Mutex<Queue>> {
        self.queue.clone()
    }

    /// Call after any queue edit so a stale "next track" is not played.
    pub fn queue_changed(&self) {
        self.send(Cmd::QueueChanged);
    }

    // -- transport --------------------------------------------------------------------------

    /// Starts a queue entry from the beginning.
    pub fn play(&self, uid: u32) {
        self.send(Cmd::Play(uid));
    }

    pub fn pause(&self) {
        self.shared.playing.store(false, Relaxed);
    }

    /// Pause when playing, otherwise resume (or replay the last track if the queue ended).
    pub fn toggle(&self) {
        if self.shared.playing.load(Relaxed) {
            self.pause();
        } else {
            self.send(Cmd::Resume);
        }
    }

    pub fn next(&self) {
        self.send(Cmd::Next);
    }
    pub fn prev(&self) {
        self.send(Cmd::Prev);
    }
    pub fn seek_ms(&self, ms: u64) {
        self.send(Cmd::Seek(ms));
    }

    // -- state ------------------------------------------------------------------------------

    pub fn now(&self) -> Now {
        let (uid, pos_ms) = self.shared.now();
        Now { uid, pos_ms }
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Relaxed)
    }

    /// How many times the queue has run out (changes when playback stops by itself).
    pub fn ended_count(&self) -> u32 {
        self.shared.ended.load(Relaxed)
    }

    /// Buffer underruns since start; non-zero means the decoder could not keep up at least once.
    pub fn underruns(&self) -> u32 {
        self.shared.underruns.load(Relaxed)
    }

    pub fn poll_events(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    // -- settings ---------------------------------------------------------------------------

    pub fn set_volume(&self, v: f32) {
        self.shared.set_volume(v);
    }
    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Relaxed);
    }
    pub fn set_normalization(&self, mode: u8, preamp_db: f32, prevent_clipping: bool) {
        self.shared.rg_mode.store(mode, Relaxed);
        self.shared.set_preamp_db(preamp_db);
        self.shared.prevent_clip.store(prevent_clipping, Relaxed);
    }
    pub fn set_device(&self, device: Option<String>) {
        self.send(Cmd::SetDevice(device));
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shared.playing.store(false, Relaxed);
        self.send(Cmd::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests;
