//! Integration tests: the real engine, decoders, resampler and audio-callback state machine
//! running against a fake sound card that captures every sample it is asked to play.
//!
//! The fake device is patient (it waits for data before rendering a block), because these tests
//! check *correctness* of what is played. Under-run behaviour of the callback itself is covered
//! by the unit tests in `pipe.rs`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::output::{Backend, Format};
use super::pipe::{Renderer, Shared};
use super::*;

const BLOCK: usize = 480;

// ---------------------------------------------------------------------------------------------
// Test media
// ---------------------------------------------------------------------------------------------

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let dir = std::env::temp_dir().join(format!("wisp-test-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn pcm_bytes(samples: &[f32], bits: u16, big_endian: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for &s in samples {
        let s = s.clamp(-1.0, 1.0);
        match bits {
            8 => out.push(((s * 127.0).round() as i32 + 128) as u8),
            16 => {
                let v = (s * 32767.0).round() as i16;
                out.extend(if big_endian { v.to_be_bytes() } else { v.to_le_bytes() });
            }
            24 => {
                let b = ((s * 8_388_607.0).round() as i32).to_le_bytes();
                out.extend(if big_endian { [b[2], b[1], b[0]] } else { [b[0], b[1], b[2]] });
            }
            _ => unreachable!(),
        }
    }
    out
}

fn write_wav(path: &Path, rate: u32, ch: u16, bits: u16, data: &[f32]) {
    let pcm = pcm_bytes(data, bits, false);
    let mut f = Vec::new();
    f.extend(b"RIFF");
    f.extend((36 + pcm.len() as u32).to_le_bytes());
    f.extend(b"WAVE");
    f.extend(b"fmt ");
    f.extend(16u32.to_le_bytes());
    f.extend(1u16.to_le_bytes());
    f.extend(ch.to_le_bytes());
    f.extend(rate.to_le_bytes());
    f.extend((rate * ch as u32 * bits as u32 / 8).to_le_bytes());
    f.extend((ch * bits / 8).to_le_bytes());
    f.extend(bits.to_le_bytes());
    f.extend(b"data");
    f.extend((pcm.len() as u32).to_le_bytes());
    f.extend(&pcm);
    std::fs::write(path, f).unwrap();
}

fn ext80(rate: u32) -> [u8; 10] {
    let e = 31 - rate.leading_zeros();
    let mut b = [0u8; 10];
    b[..2].copy_from_slice(&(16383 + e as u16).to_be_bytes());
    b[2..].copy_from_slice(&((rate as u64) << (63 - e)).to_be_bytes());
    b
}

fn write_aiff16(path: &Path, rate: u32, ch: u16, data: &[f32]) {
    let pcm = pcm_bytes(data, 16, true);
    let mut comm = Vec::new();
    comm.extend(ch.to_be_bytes());
    comm.extend(((data.len() / ch as usize) as u32).to_be_bytes());
    comm.extend(16u16.to_be_bytes());
    comm.extend(ext80(rate));
    let mut ssnd = Vec::new();
    ssnd.extend(0u32.to_be_bytes());
    ssnd.extend(0u32.to_be_bytes());
    ssnd.extend(&pcm);
    let mut body = Vec::new();
    body.extend(b"AIFF");
    body.extend(b"COMM");
    body.extend((comm.len() as u32).to_be_bytes());
    body.extend(&comm);
    body.extend(b"SSND");
    body.extend((ssnd.len() as u32).to_be_bytes());
    body.extend(&ssnd);
    let mut f = Vec::new();
    f.extend(b"FORM");
    f.extend((body.len() as u32).to_be_bytes());
    f.extend(&body);
    std::fs::write(path, f).unwrap();
}

fn stereo(mono: &[f32]) -> Vec<f32> {
    mono.iter().flat_map(|&s| [s, s]).collect()
}

fn cos_tone(rate: u32, hz: f32, start: usize, frames: usize) -> Vec<f32> {
    (start..start + frames)
        .map(|n| 0.5 * (2.0 * std::f32::consts::PI * hz * (n as f32 / rate as f32)).cos())
        .collect()
}

/// 0.1 -> 0.9 over the file: the sample value encodes the position in the file.
fn ramp(frames: usize) -> Vec<f32> {
    (0..frames).map(|n| 0.1 + 0.8 * n as f32 / frames as f32).collect()
}

fn dc(level: f32, frames: usize) -> Vec<f32> {
    vec![level; frames]
}

fn write_mono_wav(path: &Path, rate: u32, samples: &[f32]) {
    write_wav(path, rate, 2, 16, &stereo(samples));
}

// ---------------------------------------------------------------------------------------------
// The rig: engine + fake device + capture
// ---------------------------------------------------------------------------------------------

struct FakeCard {
    slot: Arc<Mutex<Option<Renderer>>>,
    rate: u32,
    opens: Arc<AtomicU32>,
    /// Set whenever a new stream replaces the old one, so the capture can mark the discontinuity
    /// even if the swap happens between two of its polls.
    reopened: Arc<AtomicBool>,
}

impl Backend for FakeCard {
    fn open(&mut self, _id: Option<&str>, make: &mut dyn FnMut(&Format) -> Renderer) -> Result<Format, String> {
        let fmt = Format { rate: self.rate, channels: 2, name: "fake".into() };
        self.reopened.store(true, Relaxed);
        *self.slot.lock().unwrap() = Some(make(&fmt));
        self.opens.fetch_add(1, Relaxed);
        Ok(fmt)
    }
    fn close(&mut self) {
        *self.slot.lock().unwrap() = None;
    }
}

/// One block handed to the fake device.
#[derive(Clone, Copy)]
struct Block {
    /// Frames captured once this block is included.
    end: usize,
    /// `now()` right after rendering it.
    now: Now,
    /// The device was starved (or reopened) just before this block: audio before and after are
    /// not contiguous, and this block starts with a fade-in.
    gap: bool,
}

struct Rig {
    player: Player,
    shared: Arc<Shared>,
    captured: Arc<Mutex<Vec<f32>>>,
    blocks: Arc<Mutex<Vec<Block>>>,
    /// Block counts at the moment a command was issued; fades follow them.
    marks: Mutex<Vec<usize>>,
    opens: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    _dir: Option<TempDir>,
}

impl Rig {
    fn new(files: Vec<PathBuf>, device_rate: u32) -> Rig {
        let slot: Arc<Mutex<Option<Renderer>>> = Arc::new(Mutex::new(None));
        let opens = Arc::new(AtomicU32::new(0));
        let reopened = Arc::new(AtomicBool::new(false));
        let resolve: Resolver = Arc::new(move |id| files.get(id as usize).cloned());
        let settings = AudioSettings { volume: 1.0, ..AudioSettings::default() };
        let (s2, o2, r2) = (slot.clone(), opens.clone(), reopened.clone());
        let player = Player::with_backend(resolve, &settings, move |_| {
            Box::new(FakeCard { slot: s2, rate: device_rate, opens: o2, reopened: r2 })
        });
        let shared = player.shared.clone();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let blocks = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (slot2, cap2, blocks2, stop2, sh2) =
            (slot.clone(), captured.clone(), blocks.clone(), stop.clone(), shared.clone());
        let thread = std::thread::spawn(move || {
            let mut buf = vec![0.0f32; BLOCK * 2];
            let mut starved = Instant::now();
            let mut gap = true;
            while !stop2.load(Relaxed) {
                let mut rendered = false;
                if reopened.swap(false, Relaxed) {
                    gap = true;
                }
                {
                    let mut guard = slot2.lock().unwrap();
                    if let Some(r) = guard.as_mut() {
                        // A patient device: wait (briefly) for data instead of recording an underrun.
                        let waiting = sh2.playing.load(Relaxed) && r.queued() < 2;
                        if !waiting || starved.elapsed() > Duration::from_millis(300) {
                            r.render(&mut buf);
                            rendered = true;
                            starved = Instant::now();
                        } else {
                            gap = true;
                        }
                    } else {
                        starved = Instant::now();
                        gap = true;
                    }
                }
                if rendered {
                    let mut c = cap2.lock().unwrap();
                    c.extend_from_slice(&buf);
                    let (uid, ms) = sh2.now();
                    blocks2.lock().unwrap().push(Block { end: c.len() / 2, now: Now { uid, pos_ms: ms }, gap });
                    gap = false;
                    drop(c);
                    std::thread::sleep(Duration::from_millis(2));
                } else {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        });
        Rig {
            player,
            shared,
            captured,
            blocks,
            marks: Mutex::new(Vec::new()),
            opens,
            stop,
            thread: Some(thread),
            _dir: None,
        }
    }

    fn with_dir(mut self, dir: TempDir) -> Rig {
        self._dir = Some(dir);
        self
    }

    /// Replaces the queue and starts playing entry `start`.
    fn play(&self, tracks: &[u32], start: usize) -> u32 {
        let uid = self.player.queue().set(tracks, start).expect("non-empty queue");
        self.player.queue_changed();
        self.mark();
        self.player.play(uid);
        uid
    }

    /// Call right before a command that fades the audio (play, seek, pause, resume, device loss).
    fn mark(&self) {
        let n = self.blocks.lock().unwrap().len();
        self.marks.lock().unwrap().push(n);
    }

    fn wait(&self, what: &str, secs: u64, cond: impl Fn(&Rig) -> bool) {
        let t = Instant::now();
        while !cond(self) {
            assert!(t.elapsed() < Duration::from_secs(secs), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn captured(&self) -> Vec<f32> {
        self.captured.lock().unwrap().clone()
    }

    fn blocks(&self) -> Vec<Block> {
        self.blocks.lock().unwrap().clone()
    }

    fn frames(&self) -> usize {
        self.captured.lock().unwrap().len() / 2
    }

    /// True when the last `n` blocks are all within `tol` of `level` (left and right).
    fn steady_at(&self, level: f32, n: usize) -> bool {
        let cap = self.captured.lock().unwrap();
        let frames = cap.len() / 2;
        if frames < n * BLOCK {
            return false;
        }
        cap[(frames - n * BLOCK) * 2..].iter().all(|s| (s - level).abs() < 0.01)
    }

    fn first_audio_frame(&self) -> Option<usize> {
        self.captured.lock().unwrap().chunks(2).position(|f| f[0] != 0.0 || f[1] != 0.0)
    }

    /// Distinct steady DC levels in order of appearance (0.1 resolution).
    fn level_sequence(&self) -> Vec<f32> {
        let cap = self.captured();
        let mut seq: Vec<f32> = Vec::new();
        for block in cap.chunks(BLOCK * 2) {
            if block.len() < BLOCK * 2 {
                break;
            }
            let first = block[0];
            if first.abs() > 0.05 && block.iter().all(|s| (s - first).abs() < 0.005) {
                let level = (first * 10.0).round() / 10.0;
                if seq.last().is_none_or(|l| (*l - level).abs() > 0.05) {
                    seq.push(level);
                }
            }
        }
        seq
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Position in ms implied by the sample value of a `ramp` file of `dur_ms`.
fn ramp_pos_ms(sample: f32, dur_ms: f32) -> f32 {
    (sample - 0.1) / 0.8 * dur_ms
}

/// Checks every settled stretch of playback: the position the engine *reports* must match the
/// position encoded in the audio actually being played, and consecutive blocks must be exactly
/// one block apart (nothing skipped, nothing repeated). Returns the settled `(pos, decoded)` list.
/// Blocks right after a command or a device gap are skipped: they contain fades.
fn check_ramp_playback(rig: &Rig, dur_ms: f32, rate: u32) -> Vec<(f32, f32)> {
    let cap = rig.captured();
    let blocks = rig.blocks();
    let marks = rig.marks.lock().unwrap().clone();
    let block_ms = BLOCK as f32 * 1000.0 / rate as f32;
    let mut run = 0usize;
    let mut settled = Vec::new();
    let mut prev: Option<f32> = None;
    for (i, b) in blocks.iter().enumerate() {
        let block = &cap[(b.end - BLOCK) * 2..b.end * 2];
        let all_audio = block.iter().step_by(2).all(|s| *s != 0.0);
        let after_mark = marks.iter().any(|&m| i >= m && i < m + 3);
        if !all_audio || b.gap || after_mark {
            run = 0;
            prev = None;
        }
        if all_audio {
            run += 1;
        }
        // Fade (12 ms) and resampler warm-up are over after a few blocks.
        if run < 6 || b.now.uid == 0 {
            continue;
        }
        let now = b.now;
        let decoded = ramp_pos_ms(block[(BLOCK - 1) * 2], dur_ms);
        let reported = now.pos_ms as f32;
        if (decoded - reported).abs() >= 3.0 {
            let mut dump = String::new();
            for (j, bb) in blocks.iter().enumerate().take(i + 3).skip(i.saturating_sub(16)) {
                let blk = &cap[(bb.end - BLOCK) * 2..bb.end * 2];
                dump += &format!(
                    "  block {j}: end {} uid {} reported {} ms decoded {:.1} ms gap {} first {:.4} last {:.4}\n",
                    bb.end,
                    bb.now.uid,
                    bb.now.pos_ms,
                    ramp_pos_ms(blk[(BLOCK - 1) * 2], dur_ms),
                    bb.gap,
                    blk[0],
                    blk[(BLOCK - 1) * 2]
                );
            }
            panic!("reported {reported} ms but audio is at {decoded} ms (block {i}, marks {marks:?})\n{dump}");
        }
        if let Some(p) = prev {
            assert!(
                ((decoded - p) - block_ms).abs() < 1.0,
                "blocks are {} ms apart, expected {block_ms}: audio was skipped or repeated",
                decoded - p
            );
        }
        prev = Some(decoded);
        settled.push((reported, decoded));
    }
    settled
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[test]
fn gapless_across_files_is_sample_exact() {
    let dir = TempDir::new("gapless");
    let rate = 48_000;
    // Deliberately awkward lengths: no relation to packet or chunk sizes.
    let lens = [31_337usize, 20_011, 40_000];
    let total: usize = lens.iter().sum();
    let full = cos_tone(rate, 997.0, 0, total);
    let mut files = Vec::new();
    let mut at = 0;
    for (i, &n) in lens.iter().enumerate() {
        let p = dir.path(&format!("t{i}.wav"));
        write_mono_wav(&p, rate, &full[at..at + n]);
        at += n;
        files.push(p);
    }
    let rig = Rig::new(files, rate).with_dir(dir);
    rig.play(&[0, 1, 2], 0);
    rig.wait("the queue to end", 30, |r| r.player.ended_count() == 1);
    rig.wait("trailing silence", 5, |r| r.frames() > 0);

    let start = rig.first_audio_frame().expect("audio was played");
    let cap = rig.captured();
    let mut worst = (0.0f32, 0usize);
    for n in 2_000..total {
        let e = (cap[(start + n) * 2] - full[n]).abs();
        if e > worst.0 {
            worst = (e, n);
        }
    }
    assert!(worst.0 < 1.5e-3, "sample {} deviates by {}: gap, overlap or glitch at a join", worst.1, worst.0);
    // Nothing plays after the last frame.
    let tail = &cap[(start + total) * 2..((start + total) * 2 + 1000).min(cap.len())];
    assert!(tail.iter().all(|s| *s == 0.0), "audio continues past the end of the last track");
    assert_eq!(rig.player.now().uid, 3, "now-playing ended on the last entry");
}

#[test]
fn gapless_with_sample_rate_conversion_is_continuous() {
    let dir = TempDir::new("gapless-rs");
    // 44.1 kHz files on a 48 kHz device. Lengths are multiples of 14700 so every file maps to a
    // whole number of output frames and the signal is truly continuous across the joins.
    let lens = [44_100usize, 44_100, 29_400];
    let total_in: usize = lens.iter().sum();
    let total_out = total_in * 160 / 147;
    let full_in = cos_tone(44_100, 997.0, 0, total_in);
    let reference = cos_tone(48_000, 997.0, 0, total_out);
    let mut files = Vec::new();
    let mut at = 0;
    for (i, &n) in lens.iter().enumerate() {
        let p = dir.path(&format!("t{i}.wav"));
        write_mono_wav(&p, 44_100, &full_in[at..at + n]);
        at += n;
        files.push(p);
    }
    let rig = Rig::new(files, 48_000).with_dir(dir);
    rig.play(&[0, 1, 2], 0);
    rig.wait("the queue to end", 30, |r| r.player.ended_count() == 1);
    rig.wait("capture", 5, |r| r.frames() > 0);

    let start = rig.first_audio_frame().expect("audio was played");
    let cap = rig.captured();
    let join1 = 44_100 * 160 / 147;
    let join2 = 2 * join1;
    let mut worst_overall = 0.0f32;
    let mut worst_at_join = 0.0f32;
    for n in 3_000..total_out - 400 {
        let e = (cap[(start + n) * 2] - reference[n]).abs();
        worst_overall = worst_overall.max(e);
        if n.abs_diff(join1) < 300 || n.abs_diff(join2) < 300 {
            worst_at_join = worst_at_join.max(e);
        }
    }
    assert!(worst_overall < 4e-3, "resampled signal deviates by {worst_overall}");
    assert!(
        worst_at_join < 4e-3,
        "the join between tracks deviates by {worst_at_join}: the resampler was restarted instead of continued"
    );
    // The output is exactly as long as the source implies: no frames gained or lost.
    let tail = &cap[(start + total_out) * 2..((start + total_out) * 2 + 500).min(cap.len())];
    assert!(tail.iter().all(|s| s.abs() < 1e-6), "audio continues past the computed end");
}

#[test]
fn reported_position_matches_audio_through_seeks_native_rate() {
    seek_and_position_test(48_000);
}

#[test]
fn reported_position_matches_audio_through_seeks_resampled() {
    seek_and_position_test(44_100);
}

fn seek_and_position_test(file_rate: u32) {
    let dir = TempDir::new("seek");
    let frames = file_rate as usize * 6;
    let dur_ms = 6000.0;
    let p = dir.path("ramp.wav");
    write_mono_wav(&p, file_rate, &ramp(frames));
    let rig = Rig::new(vec![p], 48_000).with_dir(dir);
    rig.play(&[0], 0);
    rig.wait("1 s of playback", 20, |r| r.player.now().pos_ms > 1000);

    // Forward seek.
    rig.mark();
    rig.player.seek_ms(4000);
    rig.wait("position after the forward seek", 20, |r| (4100..4800).contains(&r.player.now().pos_ms));
    // Backward seek.
    rig.mark();
    rig.player.seek_ms(1500);
    rig.wait("position after the backward seek", 20, |r| (1600..2300).contains(&r.player.now().pos_ms));
    // Seek again while the previous one is still settling.
    rig.mark();
    rig.player.seek_ms(2500);
    rig.player.seek_ms(3500);
    rig.wait("position after back-to-back seeks", 20, |r| (3600..4300).contains(&r.player.now().pos_ms));

    let settled = check_ramp_playback(&rig, dur_ms, 48_000);
    assert!(settled.len() > 50, "too little settled audio to judge ({} blocks)", settled.len());
    // The seeks landed where requested: after the 4000 ms seek the first settled block is near it.
    let near = |target: f32| settled.iter().any(|(p, _)| *p >= target && *p < target + 160.0);
    assert!(near(4000.0) && near(1500.0) && near(3500.0), "a seek did not land at its target");
}

#[test]
fn pause_freezes_the_position_and_resume_continues_exactly() {
    let dir = TempDir::new("pause");
    let p = dir.path("ramp.wav");
    write_mono_wav(&p, 48_000, &ramp(48_000 * 8));
    let rig = Rig::new(vec![p], 48_000).with_dir(dir);
    rig.play(&[0], 0);
    rig.wait("1.5 s of playback", 20, |r| r.player.now().pos_ms > 1500);

    rig.mark();
    rig.player.toggle();
    assert!(!rig.player.is_playing(), "pausing is immediate");
    let after_fade = rig.frames() + 3 * BLOCK;
    rig.wait("the fade-out to finish", 5, |r| r.frames() >= after_fade);
    let paused_at = rig.player.now();
    let mark = rig.frames();
    rig.wait("a paused stretch", 5, |r| r.frames() >= mark + 20 * BLOCK);
    {
        let cap = rig.captured();
        assert!(cap[mark * 2..].iter().all(|s| *s == 0.0), "paused output must be silent");
    }
    assert_eq!(rig.player.now(), paused_at, "the position does not move while paused");

    rig.mark();
    rig.player.toggle();
    // Resuming goes through the engine thread, so it is not instantaneous.
    rig.wait("the player to report playing", 5, |r| r.player.is_playing());
    rig.wait("playback to resume", 20, |r| r.player.now().pos_ms > paused_at.pos_ms + 400);
    let settled = check_ramp_playback(&rig, 8000.0, 48_000);
    let first_after = settled.iter().find(|(p, _)| *p > paused_at.pos_ms as f32 - 1.0).expect("settled audio after resume");
    assert!(
        first_after.0 - (paused_at.pos_ms as f32) < 120.0,
        "resumed {} ms after the pause point",
        first_after.0 - paused_at.pos_ms as f32
    );
}

#[test]
fn next_prev_repeat_and_end_of_queue() {
    let dir = TempDir::new("nav");
    let levels = [0.2f32, 0.4, 0.6];
    let mut files = Vec::new();
    for (i, l) in levels.iter().enumerate() {
        let p = dir.path(&format!("l{i}.wav"));
        write_mono_wav(&p, 48_000, &dc(*l, 48_000 * 20));
        files.push(p);
    }
    let rig = Rig::new(files, 48_000).with_dir(dir);
    let uid_a = rig.play(&[0, 1, 2], 0);
    rig.wait("A", 20, |r| r.steady_at(0.2, 4));
    rig.player.next();
    rig.wait("B", 20, |r| r.steady_at(0.4, 4));
    rig.player.next();
    rig.wait("C", 20, |r| r.steady_at(0.6, 4));
    rig.player.next();
    std::thread::sleep(Duration::from_millis(300));
    assert!(rig.steady_at(0.6, 4), "next on the last entry (repeat off) is ignored");
    rig.player.prev();
    rig.wait("previous within 3 s goes back to B", 20, |r| r.steady_at(0.4, 4));

    // Repeat-all wraps from the end to the start.
    rig.player.queue().set_repeat(Repeat::All);
    rig.player.queue_changed();
    let last_uid = rig.player.queue().at(2).unwrap().uid;
    rig.player.play(last_uid);
    rig.wait("C again", 20, |r| r.steady_at(0.6, 4));
    rig.player.next();
    rig.wait("wrap to A", 20, |r| r.steady_at(0.2, 4));
    assert_eq!(rig.player.queue().get(uid_a).unwrap().track, 0);
}

#[test]
fn repeat_all_cycles_gaplessly_and_stopping_at_the_end_allows_replay() {
    let dir = TempDir::new("cycle");
    let mut files = Vec::new();
    for (i, l) in [0.2f32, 0.4].iter().enumerate() {
        let p = dir.path(&format!("c{i}.wav"));
        write_mono_wav(&p, 48_000, &dc(*l, 14_400)); // 0.3 s each
        files.push(p);
    }
    let rig = Rig::new(files, 48_000).with_dir(dir);
    rig.player.queue().set_repeat(Repeat::All);
    rig.play(&[0, 1], 0);
    rig.wait("two full cycles", 30, |r| r.frames() > 48_000 * 2);
    let seq = rig.level_sequence();
    assert!(seq.len() >= 5, "expected an alternating sequence, got {seq:?}");
    for (i, l) in seq.iter().enumerate() {
        let want = if i % 2 == 0 { 0.2 } else { 0.4 };
        assert!((l - want).abs() < 0.05, "sequence {seq:?} breaks at {i}");
    }
    assert_eq!(rig.player.ended_count(), 0);

    // Repeat off: the queue ends, then play restarts the last track.
    rig.player.queue().set_repeat(Repeat::Off);
    rig.player.queue_changed();
    rig.wait("the queue to end", 30, |r| r.player.ended_count() == 1);
    assert!(!rig.player.is_playing());
    let mark = rig.frames();
    rig.player.toggle();
    rig.wait("the last track to replay", 20, |r| r.steady_at(0.4, 2) && r.frames() > mark);
    rig.wait("the queue to end again", 30, |r| r.player.ended_count() == 2);
}

#[test]
fn unplayable_files_are_skipped_and_reported() {
    let dir = TempDir::new("bad");
    let a = dir.path("a.wav");
    let c = dir.path("c.wav");
    write_mono_wav(&a, 48_000, &dc(0.2, 14_400));
    write_mono_wav(&c, 48_000, &dc(0.6, 14_400));
    let garbage = dir.path("garbage.wav");
    std::fs::write(&garbage, vec![0x5Au8; 4096]).unwrap();
    let missing = dir.path("missing.wav");
    let rig = Rig::new(vec![a, garbage, missing, c], 48_000).with_dir(dir);
    rig.play(&[0, 1, 2, 3], 0);
    rig.wait("the queue to end", 30, |r| r.player.ended_count() == 1);
    let seq = rig.level_sequence();
    assert_eq!(seq.len(), 2, "A then C, got {seq:?}");
    assert!((seq[0] - 0.2).abs() < 0.05 && (seq[1] - 0.6).abs() < 0.05);
    let errors: Vec<String> = rig
        .player
        .poll_events()
        .into_iter()
        .filter_map(|e| if let Event::Error(m) = e { Some(m) } else { None })
        .collect();
    assert!(errors.iter().any(|m| m.contains("garbage.wav")), "no error for the corrupt file: {errors:?}");
    assert!(errors.iter().any(|m| m.contains("missing.wav")), "no error for the missing file: {errors:?}");
}

#[test]
fn editing_the_queue_replaces_the_prepared_next_track() {
    let dir = TempDir::new("edit");
    let mut files = Vec::new();
    for (i, l) in [0.2f32, 0.4, 0.6, 0.8].iter().enumerate() {
        let p = dir.path(&format!("e{i}.wav"));
        write_mono_wav(&p, 48_000, &dc(*l, 19_200)); // 0.4 s
        files.push(p);
    }
    let rig = Rig::new(files, 48_000).with_dir(dir);
    let uid = rig.play(&[0, 1, 2], 0);
    rig.wait("A to start", 20, |r| r.steady_at(0.2, 2));
    // "Play next" D while B has already been opened ahead of time.
    rig.player.queue().play_next(Some(uid), 3);
    rig.player.queue_changed();
    rig.wait("the queue to end", 30, |r| r.player.ended_count() == 1);
    let seq = rig.level_sequence();
    let want = [0.2, 0.8, 0.4, 0.6];
    assert_eq!(seq.len(), 4, "got {seq:?}");
    for (a, b) in seq.iter().zip(want) {
        assert!((a - b).abs() < 0.05, "got {seq:?}, wanted {want:?}");
    }
}

#[test]
fn losing_the_audio_device_recovers_at_the_same_position() {
    let dir = TempDir::new("devloss");
    let p = dir.path("ramp.wav");
    write_mono_wav(&p, 48_000, &ramp(48_000 * 8));
    let rig = Rig::new(vec![p], 48_000).with_dir(dir);
    rig.play(&[0], 0);
    rig.wait("2 s of playback", 20, |r| r.player.now().pos_ms > 2000);
    assert_eq!(rig.opens.load(Relaxed), 1);

    let before = rig.player.now().pos_ms;
    rig.mark();
    rig.shared.device_lost.store(true, Relaxed);
    rig.wait("the device to be reopened", 20, |r| r.opens.load(Relaxed) == 2);
    rig.wait("playback to continue", 20, |r| r.player.now().pos_ms > before + 600);
    let settled = check_ramp_playback(&rig, 8000.0, 48_000);
    let resumed = settled.iter().filter(|(p, _)| *p > before as f32 - 1.0).map(|(p, _)| *p).fold(f32::MAX, f32::min);
    assert!(resumed < before as f32 + 250.0, "resumed at {resumed} ms after losing the device at {before} ms");
    assert!(rig.player.is_playing());
}

#[test]
fn every_container_and_bit_depth_decodes_to_the_right_level() {
    let dir = TempDir::new("formats");
    let cases: [(&str, f32, PathBuf); 3] = [
        ("AIFF 16-bit stereo", 0.3, dir.path("a.aiff")),
        ("WAV 24-bit mono", 0.25, dir.path("b.wav")),
        ("WAV 8-bit stereo @ 22.05 kHz", 0.5, dir.path("c.wav")),
    ];
    write_aiff16(&cases[0].2, 44_100, 2, &stereo(&dc(0.3, 44_100)));
    write_wav(&cases[1].2, 48_000, 1, 24, &dc(0.25, 48_000));
    write_wav(&cases[2].2, 22_050, 2, 8, &stereo(&dc(0.5, 22_050)));
    let files: Vec<PathBuf> = cases.iter().map(|c| c.2.clone()).collect();
    let rig = Rig::new(files, 48_000).with_dir(dir);
    for (i, (name, level, _)) in cases.iter().enumerate() {
        rig.play(&[i as u32], 0);
        let level = *level;
        rig.wait(name, 20, move |r| r.steady_at(level, 4));
        // Both channels carry the signal (mono is spread to the front pair).
        let cap = rig.captured();
        let n = cap.len();
        assert!((cap[n - 1] - level).abs() < 0.01 && (cap[n - 2] - level).abs() < 0.01, "{name}: channels differ");
    }
}

#[test]
fn volume_and_mute_apply_to_live_playback_without_clicks() {
    let dir = TempDir::new("volume");
    let p = dir.path("dc.wav");
    write_mono_wav(&p, 48_000, &dc(0.8, 48_000 * 20));
    let rig = Rig::new(vec![p], 48_000).with_dir(dir);
    rig.play(&[0], 0);
    rig.wait("full volume", 20, |r| r.steady_at(0.8, 4));
    let from = rig.frames();
    rig.player.set_volume(0.5);
    rig.wait("half volume", 20, |r| r.steady_at(0.8 * 0.125, 4));
    rig.player.set_muted(true);
    rig.wait("mute", 20, |r| r.steady_at(0.0, 4));
    rig.player.set_muted(false);
    rig.wait("unmute", 20, |r| r.steady_at(0.8 * 0.125, 4));
    let cap = rig.captured();
    let mut max_step = 0.0f32;
    for f in from + 1..cap.len() / 2 {
        max_step = max_step.max((cap[f * 2] - cap[(f - 1) * 2]).abs());
    }
    assert!(max_step < 0.02, "a volume change produced a step of {max_step}");
}
