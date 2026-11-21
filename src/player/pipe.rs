//! The real-time boundary between the decoder thread and the audio callback.
//!
//! Audio travels in fixed-size *chunks* (~20 ms each) that are allocated once and then circulate
//! between two lock-free queues: `full` (decoder -> callback) and `free` (callback -> decoder).
//! Nothing is allocated, freed or locked on the audio thread, and the total memory in flight is
//! bounded by `CHUNKS * chunk size` (about 190 KB for 48 kHz stereo).
//!
//! Every chunk is self-describing (which queue entry it belongs to, where in that track its first
//! frame is, which ReplayGain applies), so gapless track changes, position reporting and
//! end-of-queue detection need no side channel: they happen exactly at the chunk boundary where
//! the audio itself changes.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering::*};
use std::time::{Duration, Instant};

use crossbeam_queue::ArrayQueue;

/// Chunks in circulation. With 20 ms chunks this is ~480 ms of audio buffered ahead.
pub const CHUNKS: usize = 24;
const CHUNK_MS: usize = 20;

/// Pause/seek/start fade length. Long enough to be click-free, short enough to feel instant.
const FADE_SECS: f32 = 0.012;
/// Time constant of the volume / ReplayGain smoother.
const GAIN_SECS: f32 = 0.006;

pub const RG_OFF: u8 = 0;
pub const RG_TRACK: u8 = 1;
pub const RG_ALBUM: u8 = 2;

/// A linear gain plus the largest gain that does not clip the track's own peak.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gain {
    pub lin: f32,
    pub max: f32,
}

impl Gain {
    pub const UNITY: Gain = Gain { lin: 1.0, max: f32::INFINITY };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Audio,
    /// Nothing follows: when the callback reaches this it stops playback.
    End,
}

pub struct Chunk {
    pub kind: Kind,
    /// Queue entry these frames belong to.
    pub uid: u32,
    /// Position (in output frames) of the first frame within its track.
    pub pos: u64,
    pub track_gain: Gain,
    pub album_gain: Gain,
    /// Valid frames in `data`.
    pub len: usize,
    /// `capacity * channels` interleaved samples.
    pub data: Vec<f32>,
    channels: usize,
}

impl Chunk {
    fn new(frames: usize, channels: usize) -> Box<Chunk> {
        Box::new(Chunk {
            kind: Kind::Audio,
            uid: 0,
            pos: 0,
            track_gain: Gain::UNITY,
            album_gain: Gain::UNITY,
            len: 0,
            data: vec![0.0; frames * channels],
            channels,
        })
    }

    /// Capacity in frames.
    pub fn capacity(&self) -> usize {
        self.data.len() / self.channels
    }

    pub fn is_full(&self) -> bool {
        self.len >= self.capacity()
    }

    pub fn reset(&mut self) {
        self.kind = Kind::Audio;
        self.len = 0;
    }

    /// Appends up to `src.len() / channels` frames; returns how many were taken.
    pub fn append(&mut self, src: &[f32]) -> usize {
        let take = (src.len() / self.channels).min(self.capacity() - self.len);
        let (a, b) = (self.len * self.channels, (self.len + take) * self.channels);
        self.data[a..b].copy_from_slice(&src[..take * self.channels]);
        self.len += take;
        take
    }
}

/// State shared between the UI, the engine thread and the audio callback. Everything is an
/// atomic so the callback never blocks.
pub struct Shared {
    /// The user wants sound (false = paused or stopped).
    pub playing: AtomicBool,
    /// Volume slider position 0..=1 (f32 bits); mapped through a perceptual curve.
    pub volume: AtomicU32,
    pub muted: AtomicBool,
    pub rg_mode: AtomicU8,
    /// Linear pre-amp applied on top of ReplayGain (f32 bits).
    pub preamp: AtomicU32,
    pub prevent_clip: AtomicBool,
    pub flush_req: AtomicU32,
    pub flush_ack: AtomicU32,
    /// `(uid << 32) | position_ms` of the audio that is being played right now.
    pub now: AtomicU64,
    /// Number of times the queue ran out (End chunk reached).
    pub ended: AtomicU32,
    /// Buffer underruns since start (diagnostic).
    pub underruns: AtomicU32,
    /// Set by the stream error callback; the engine reopens the device.
    pub device_lost: AtomicBool,
    /// The engine has a track open (decoding or buffered).
    pub loaded: AtomicBool,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared {
            playing: AtomicBool::new(false),
            volume: AtomicU32::new(0.8f32.to_bits()),
            muted: AtomicBool::new(false),
            rg_mode: AtomicU8::new(RG_OFF),
            preamp: AtomicU32::new(1.0f32.to_bits()),
            prevent_clip: AtomicBool::new(true),
            flush_req: AtomicU32::new(0),
            flush_ack: AtomicU32::new(0),
            now: AtomicU64::new(0),
            ended: AtomicU32::new(0),
            underruns: AtomicU32::new(0),
            device_lost: AtomicBool::new(false),
            loaded: AtomicBool::new(false),
        })
    }

    pub fn set_volume(&self, v: f32) {
        self.volume.store(v.clamp(0.0, 1.0).to_bits(), Relaxed);
    }
    pub fn set_preamp_db(&self, db: f32) {
        self.preamp.store(db_to_lin(db).to_bits(), Relaxed);
    }

    /// `(uid, position_ms)` of what is audible. `uid == 0` means nothing.
    pub fn now(&self) -> (u32, u32) {
        let v = self.now.load(Relaxed);
        ((v >> 32) as u32, v as u32)
    }
    pub fn set_now(&self, uid: u32, ms: u32) {
        self.now.store(((uid as u64) << 32) | ms as u64, Relaxed);
    }
}

pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Slider position -> amplitude. A cubic curve gives fine control where it matters.
pub fn volume_curve(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    v * v * v
}

/// Frames per chunk for a device rate.
pub fn chunk_frames(rate: u32) -> usize {
    let f = rate as usize * CHUNK_MS / 1000;
    f.next_multiple_of(16).max(64)
}

/// Engine-side handle of the pipe.
pub struct PipeProducer {
    full: Arc<ArrayQueue<Box<Chunk>>>,
    free: Arc<ArrayQueue<Box<Chunk>>>,
    shared: Arc<Shared>,
}

impl PipeProducer {
    /// An empty chunk to fill, or `None` when the callback has not consumed any yet.
    pub fn take_free(&self) -> Option<Box<Chunk>> {
        let mut c = self.free.pop()?;
        c.reset();
        Some(c)
    }

    pub fn push(&self, chunk: Box<Chunk>) {
        // `full` can hold every chunk in existence, so this cannot fail.
        let _ = self.full.push(chunk);
    }

    /// Returns an unused chunk without playing it.
    pub fn give_back(&self, chunk: Box<Chunk>) {
        let _ = self.free.push(chunk);
    }

    /// Chunks currently waiting to be played.
    pub fn queued(&self) -> usize {
        self.full.len()
    }

    /// Asks the callback to fade out, drop everything queued and acknowledge. Returns `false` if
    /// the callback did not answer in time (no running stream).
    pub fn flush(&self, timeout: Duration) -> bool {
        let req = self.shared.flush_req.fetch_add(1, AcqRel).wrapping_add(1);
        let start = Instant::now();
        while self.shared.flush_ack.load(Acquire) != req {
            if start.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        true
    }
}

/// Creates the two queues and the callback-side renderer for a device format.
pub fn pipe(shared: Arc<Shared>, rate: u32, channels: usize) -> (PipeProducer, Renderer) {
    let full = Arc::new(ArrayQueue::new(CHUNKS));
    let free = Arc::new(ArrayQueue::new(CHUNKS));
    let frames = chunk_frames(rate);
    for _ in 0..CHUNKS {
        let _ = free.push(Chunk::new(frames, channels));
    }
    let epoch = shared.flush_req.load(Acquire);
    shared.flush_ack.store(epoch, Release);
    let producer = PipeProducer {
        full: full.clone(),
        free: free.clone(),
        shared: shared.clone(),
    };
    let renderer = Renderer {
        full,
        free,
        shared,
        ch: channels,
        rate,
        cur: None,
        off: 0,
        epoch,
        fade: 0.0,
        fade_step: 1.0 / (FADE_SECS * rate as f32),
        gain: 0.0,
        gain_coef: 1.0 - (-1.0 / (GAIN_SECS * rate as f32)).exp(),
        published: None,
        primed: false,
    };
    (producer, renderer)
}

/// Everything the audio callback does. No allocation, no locks, no syscalls.
pub struct Renderer {
    full: Arc<ArrayQueue<Box<Chunk>>>,
    free: Arc<ArrayQueue<Box<Chunk>>>,
    shared: Arc<Shared>,
    ch: usize,
    rate: u32,
    cur: Option<Box<Chunk>>,
    off: usize,
    epoch: u32,
    fade: f32,
    fade_step: f32,
    gain: f32,
    gain_coef: f32,
    /// Queue entry and frame position of the last frame played (None right after a flush).
    published: Option<(u32, u64)>,
    /// Audio has been delivered since the last flush. Until then an empty queue only means the
    /// engine is still opening the file after a start, seek or skip; that wait is not an underrun.
    primed: bool,
}

fn approach(x: f32, target: f32, step: f32) -> f32 {
    if x < target { (x + step).min(target) } else { (x - step).max(target) }
}

fn chunk_gain(c: &Chunk, mode: u8, preamp: f32, prevent_clip: bool) -> f32 {
    let g = match mode {
        RG_TRACK => c.track_gain,
        RG_ALBUM => c.album_gain,
        _ => return 1.0,
    };
    let lin = g.lin * preamp;
    if prevent_clip { lin.min(g.max) } else { lin }
}

impl Renderer {
    /// Chunks waiting to be played (test rigs use it to emulate a patient device).
    #[cfg(test)]
    pub fn queued(&self) -> usize {
        self.full.len()
    }

    fn recycle(&self, chunk: Box<Chunk>) {
        let _ = self.free.push(chunk);
    }

    fn drain(&mut self) {
        if let Some(c) = self.cur.take() {
            self.recycle(c);
        }
        self.off = 0;
        while let Some(c) = self.full.pop() {
            self.recycle(c);
        }
        self.published = None;
        self.primed = false;
    }

    /// Fills `out` (interleaved, `channels` per frame) with the next audio.
    pub fn render(&mut self, out: &mut [f32]) {
        let ch = self.ch;
        let frames = out.len() / ch;
        let req = self.shared.flush_req.load(Acquire);
        let mut flushing = req != self.epoch;
        let mut want = self.shared.playing.load(Relaxed);
        let vol = if self.shared.muted.load(Relaxed) {
            0.0
        } else {
            volume_curve(f32::from_bits(self.shared.volume.load(Relaxed)))
        };
        let mode = self.shared.rg_mode.load(Relaxed);
        let preamp = f32::from_bits(self.shared.preamp.load(Relaxed));
        let clip = self.shared.prevent_clip.load(Relaxed);

        let mut i = 0;
        while i < frames {
            let target = if want && !flushing { 1.0 } else { 0.0 };

            // Fully faded out and not supposed to play: silence (and service a flush request).
            if self.fade <= 0.0 && target == 0.0 {
                if flushing {
                    self.drain();
                    self.epoch = req;
                    self.shared.flush_ack.store(req, Release);
                    flushing = false;
                    continue;
                }
                out[i * ch..].fill(0.0);
                break;
            }

            if self.cur.is_none() {
                match self.full.pop() {
                    Some(c) if c.kind == Kind::End => {
                        self.published = Some((c.uid, c.pos));
                        self.shared.playing.store(false, Relaxed);
                        self.shared.ended.fetch_add(1, Relaxed);
                        self.recycle(c);
                        want = false;
                        self.fade = 0.0;
                        continue;
                    }
                    Some(c) => {
                        self.cur = Some(c);
                        self.off = 0;
                        self.primed = true;
                    }
                    None => {
                        if flushing {
                            // Nothing left to fade out: go straight to the flush.
                            self.fade = 0.0;
                            continue;
                        }
                        // Silence now, fade back in when data arrives. It is an underrun (counted once
                        // per dry spell) only if the decoder fell behind mid-playback.
                        if self.primed {
                            self.shared.underruns.fetch_add(1, Relaxed);
                            self.primed = false;
                        }
                        self.fade = 0.0;
                        out[i * ch..].fill(0.0);
                        break;
                    }
                }
            }

            let Some(c) = self.cur.as_ref() else { break };
            let (uid, base) = (c.uid, c.pos);
            let n = (c.len - self.off).min(frames - i);
            let gtarget = vol * chunk_gain(c, mode, preamp, clip);
            let (step, coef) = (self.fade_step, self.gain_coef);
            let mut done = 0;
            while done < n {
                if self.fade <= 0.0 && target == 0.0 {
                    break;
                }
                self.fade = approach(self.fade, target, step);
                self.gain += (gtarget - self.gain) * coef;
                let g = self.gain * self.fade;
                let s = (self.off + done) * ch;
                let d = (i + done) * ch;
                for k in 0..ch {
                    out[d + k] = (c.data[s + k] * g).clamp(-1.0, 1.0);
                }
                done += 1;
            }
            self.off += done;
            i += done;
            if done > 0 {
                self.published = Some((uid, base + self.off as u64));
            }
            if self.off >= c.len {
                if let Some(c) = self.cur.take() {
                    self.recycle(c);
                }
                self.off = 0;
            }
        }

        if let Some((uid, pos)) = self.published {
            let ms = (pos * 1000 / self.rate as u64).min(u32::MAX as u64) as u32;
            self.shared.set_now(uid, ms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;
    const CH: usize = 2;

    /// Fills a chunk with a counter ramp so every frame is unique and checkable.
    fn ramp_chunk(p: &PipeProducer, uid: u32, pos: u64, start: usize, frames: usize) -> Box<Chunk> {
        let mut c = p.take_free().expect("free chunk");
        c.uid = uid;
        c.pos = pos;
        let mut buf = Vec::new();
        for k in 0..frames {
            let v = ((start + k) % 1000) as f32 / 2000.0 + 0.01;
            buf.extend([v, -v]);
        }
        assert_eq!(c.append(&buf), frames);
        c
    }

    fn setup() -> (Arc<Shared>, PipeProducer, Renderer) {
        let sh = Shared::new();
        sh.set_volume(1.0);
        let (p, r) = pipe(sh.clone(), RATE, CH);
        (sh, p, r)
    }

    fn render_frames(r: &mut Renderer, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * CH];
        r.render(&mut out);
        out
    }

    #[test]
    fn plays_in_order_with_exact_samples_after_fade_in() {
        let (sh, p, mut r) = setup();
        let fr = chunk_frames(RATE);
        let mut expected = Vec::new();
        for k in 0..4 {
            let c = ramp_chunk(&p, 1, (k * fr) as u64, k * fr, fr);
            expected.extend_from_slice(&c.data[..fr * CH]);
            p.push(c);
        }
        sh.playing.store(true, Relaxed);
        let out = render_frames(&mut r, 4 * fr);
        // After the fade-in (12 ms) and gain smoothing (~6 ms) the signal is bit-exact.
        let skip = (0.04 * RATE as f32) as usize;
        for f in skip..4 * fr {
            let (a, b) = (out[f * CH], expected[f * CH]);
            assert!((a - b).abs() < 1e-3, "frame {f}: {a} vs {b}");
        }
        let (uid, ms) = sh.now();
        assert_eq!(uid, 1);
        assert_eq!(ms as usize, (4 * fr) * 1000 / RATE as usize);
    }

    #[test]
    fn chunk_boundary_between_tracks_is_seamless_and_switches_uid() {
        let (sh, p, mut r) = setup();
        let fr = chunk_frames(RATE);
        // Track 1 ends with a partial chunk; track 2 starts immediately after.
        let mut a = ramp_chunk(&p, 1, 0, 0, fr / 2);
        a.pos = 0;
        p.push(a);
        p.push(ramp_chunk(&p, 2, 0, 0, fr));
        sh.playing.store(true, Relaxed);
        let _ = render_frames(&mut r, fr / 2);
        assert_eq!(sh.now().0, 1);
        let _ = render_frames(&mut r, fr);
        assert_eq!(sh.now().0, 2, "audible track switches exactly at the chunk boundary");
    }

    #[test]
    fn pause_freezes_position_and_resume_continues_without_skips() {
        let (sh, p, mut r) = setup();
        let fr = chunk_frames(RATE);
        for k in 0..6 {
            p.push(ramp_chunk(&p, 7, (k * fr) as u64, k * fr, fr));
        }
        sh.playing.store(true, Relaxed);
        let _ = render_frames(&mut r, 2 * fr);
        sh.playing.store(false, Relaxed);
        let _ = render_frames(&mut r, fr); // fade-out happens here
        let queued = p.queued();
        let pos = sh.now();
        let silent = render_frames(&mut r, 3 * fr);
        assert!(silent.iter().all(|&s| s == 0.0), "paused output is silence");
        assert_eq!(p.queued(), queued, "nothing is consumed while paused");
        assert_eq!(sh.now(), pos, "position is frozen");
        sh.playing.store(true, Relaxed);
        let out = render_frames(&mut r, fr);
        // Resumes at exactly the frame after the one consumed when the fade-out finished.
        let resumed_at = pos.1 as usize * RATE as usize / 1000;
        let skip = (0.04 * RATE as f32) as usize;
        for f in skip..fr {
            let want = (((resumed_at + f) % 1000) as f32) / 2000.0 + 0.01;
            // The position is rounded to ms, so allow a one-ms window of ramp phase.
            let got = out[f * CH];
            let ok = (-48..=48).any(|d: i64| {
                let w = ((((resumed_at + f) as i64 + d).rem_euclid(1000)) as f32) / 2000.0 + 0.01;
                (got - w).abs() < 1e-3
            });
            assert!(ok, "frame {f}: got {got}, expected about {want}");
        }
    }

    #[test]
    fn flush_discards_queued_audio_acknowledges_and_recycles_chunks() {
        let (sh, p, mut r) = setup();
        let fr = chunk_frames(RATE);
        for k in 0..10 {
            p.push(ramp_chunk(&p, 3, (k * fr) as u64, k * fr, fr));
        }
        sh.playing.store(true, Relaxed);
        let _ = render_frames(&mut r, fr);

        // The engine thread blocks in flush() while the "audio thread" keeps rendering.
        let h = std::thread::spawn(move || {
            let ok = p.flush(Duration::from_secs(2));
            (ok, p)
        });
        let start = Instant::now();
        while !h.is_finished() && start.elapsed() < Duration::from_secs(3) {
            let _ = render_frames(&mut r, 480);
            std::thread::sleep(Duration::from_millis(2));
        }
        let (ok, p) = h.join().unwrap();
        assert!(ok, "flush was acknowledged");
        let out = render_frames(&mut r, 4800);
        assert!(out.iter().all(|&s| s == 0.0), "stale audio must not leak after a flush");
        assert_eq!(p.queued(), 0);
        let mut free = 0;
        while p.take_free().is_some() {
            free += 1;
        }
        assert_eq!(free, CHUNKS, "every chunk is back in the free queue");
        assert_eq!(sh.now().0, 3, "uid is untouched; the engine republishes the position");
    }

    #[test]
    fn flush_works_while_paused_and_when_nothing_is_queued() {
        let (sh, p, mut r) = setup();
        sh.playing.store(false, Relaxed);
        let h = std::thread::spawn(move || p.flush(Duration::from_secs(2)));
        // The "audio thread" keeps running at (roughly) real-time pace until the flush is served.
        let start = Instant::now();
        while !h.is_finished() && start.elapsed() < Duration::from_secs(3) {
            let _ = render_frames(&mut r, 480);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(h.join().unwrap());
    }

    #[test]
    fn end_chunk_stops_playback_and_is_counted_once() {
        let (sh, p, mut r) = setup();
        let fr = chunk_frames(RATE);
        p.push(ramp_chunk(&p, 5, 0, 0, fr));
        let mut end = p.take_free().unwrap();
        end.kind = Kind::End;
        end.uid = 5;
        end.pos = fr as u64;
        p.push(end);
        sh.playing.store(true, Relaxed);
        let out = render_frames(&mut r, 3 * fr);
        assert!(out[(2 * fr) * CH..].iter().all(|&s| s == 0.0));
        assert!(!sh.playing.load(Relaxed), "playback stopped at end of queue");
        assert_eq!(sh.ended.load(Relaxed), 1);
        let _ = render_frames(&mut r, 4 * fr);
        assert_eq!(sh.ended.load(Relaxed), 1);
        assert_eq!(sh.underruns.load(Relaxed), 0, "a clean end is not an underrun");
    }

    #[test]
    fn underrun_gives_silence_then_fades_back_in() {
        let (sh, p, mut r) = setup();
        let fr = chunk_frames(RATE);
        p.push(ramp_chunk(&p, 1, 0, 0, fr));
        sh.playing.store(true, Relaxed);
        let _ = render_frames(&mut r, 2 * fr);
        assert!(sh.underruns.load(Relaxed) >= 1);
        p.push(ramp_chunk(&p, 1, fr as u64, fr, fr));
        let out = render_frames(&mut r, 480);
        // First sample after an underrun starts from silence (fade-in), never a hard step.
        assert!(out[0].abs() < 0.01, "fade-in starts near zero, got {}", out[0]);
        assert!(out[CH * 400].abs() > 0.0);
    }

    #[test]
    fn waiting_for_the_first_chunk_is_no_underrun_and_a_dry_spell_counts_once() {
        let (sh, p, mut r) = setup();
        let fr = chunk_frames(RATE);
        sh.playing.store(true, Relaxed);
        // Play was pressed but the engine is still opening the file: silence, nothing to report.
        let out = render_frames(&mut r, 3 * fr);
        assert!(out.iter().all(|&s| s == 0.0));
        assert_eq!(sh.underruns.load(Relaxed), 0);
        // Audio flows, then the decoder stalls for several callbacks: one underrun, not one per callback.
        p.push(ramp_chunk(&p, 1, 0, 0, fr));
        let _ = render_frames(&mut r, fr);
        for _ in 0..5 {
            let _ = render_frames(&mut r, 480);
        }
        assert_eq!(sh.underruns.load(Relaxed), 1);
        // After a flush (seek, skip) the wait for fresh audio is not an underrun either.
        let h = std::thread::spawn(move || {
            let ok = p.flush(Duration::from_secs(2));
            (ok, p)
        });
        let start = Instant::now();
        while !h.is_finished() && start.elapsed() < Duration::from_secs(3) {
            let _ = render_frames(&mut r, 480);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(h.join().unwrap().0);
        let _ = render_frames(&mut r, 3 * fr);
        assert_eq!(sh.underruns.load(Relaxed), 1);
    }

    #[test]
    fn volume_and_mute_change_without_steps() {
        let (sh, p, mut r) = setup();
        let fr = chunk_frames(RATE);
        for k in 0..8 {
            let mut c = p.take_free().unwrap();
            c.uid = 1;
            c.pos = (k * fr) as u64;
            c.append(&vec![0.5; fr * CH]);
            p.push(c);
        }
        sh.playing.store(true, Relaxed);
        let _ = render_frames(&mut r, fr);
        sh.set_volume(0.2);
        let out = render_frames(&mut r, 2 * fr);
        let mut max_step = 0.0f32;
        for f in 1..2 * fr {
            max_step = max_step.max((out[f * CH] - out[(f - 1) * CH]).abs());
        }
        assert!(max_step < 0.01, "volume change must be smoothed, max step {max_step}");
        let settled = out[(2 * fr - 1) * CH];
        assert!((settled - 0.5 * volume_curve(0.2)).abs() < 0.01);
        sh.muted.store(true, Relaxed);
        let out = render_frames(&mut r, 2 * fr);
        assert!(out[(2 * fr - 1) * CH].abs() < 1e-3, "mute reaches silence");
    }

    #[test]
    fn replaygain_modes_preamp_and_clip_prevention() {
        let (sh, p, mut r) = setup();
        let fr = chunk_frames(RATE);
        // Signal 0.2. Track gain 0.5 (never limited), album gain 2.0 with a clip limit of 2.5.
        let mk = |p: &PipeProducer, pos: u64| {
            let mut c = p.take_free().unwrap();
            c.uid = 1;
            c.pos = pos;
            c.track_gain = Gain { lin: 0.5, max: 4.0 };
            c.album_gain = Gain { lin: 2.0, max: 2.5 };
            c.append(&vec![0.2; fr * CH]);
            c
        };
        for k in 0..CHUNKS {
            p.push(mk(&p, (k * fr) as u64));
        }
        sh.playing.store(true, Relaxed);
        let settle = |r: &mut Renderer| {
            let _ = render_frames(r, 3 * 480);
            render_frames(r, 480)[CH * 479]
        };
        assert!((settle(&mut r) - 0.2).abs() < 1e-3, "RG off leaves the signal alone");
        sh.rg_mode.store(RG_TRACK, Relaxed);
        assert!((settle(&mut r) - 0.1).abs() < 1e-3, "track gain 0.5");
        sh.rg_mode.store(RG_ALBUM, Relaxed);
        assert!((settle(&mut r) - 0.4).abs() < 1e-3, "album gain 2.0 is below its clip limit");
        sh.set_preamp_db(6.0206); // x2 -> 4.0, above the 2.5 limit
        assert!((settle(&mut r) - 0.5).abs() < 1e-3, "clip prevention caps the gain at 2.5");
        sh.prevent_clip.store(false, Relaxed);
        assert!((settle(&mut r) - 0.8).abs() < 1e-3, "without prevention the full gain applies");
    }

    #[test]
    fn position_reports_milliseconds_of_the_current_track() {
        let (sh, p, mut r) = setup();
        let fr = chunk_frames(RATE);
        // Start 10 s into the track, as after a seek.
        let base = 10 * RATE as u64;
        for k in 0..4 {
            p.push(ramp_chunk(&p, 9, base + (k * fr) as u64, 0, fr));
        }
        sh.playing.store(true, Relaxed);
        let _ = render_frames(&mut r, 2 * fr);
        let (uid, ms) = sh.now();
        assert_eq!(uid, 9);
        assert_eq!(ms as u64, (base + 2 * fr as u64) * 1000 / RATE as u64);
    }
}
