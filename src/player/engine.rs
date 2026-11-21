//! The engine thread: owns the output device and the decoder, keeps the chunk pipe full, and
//! hands over from one track to the next without a gap.
//!
//! Playback is a pipeline `file -> packets -> f32 -> channel mix -> resample -> chunks`. The
//! decoder runs ahead of the speakers by up to the pipe depth (~480 ms), which is what makes
//! gapless hand-over, instant pause and glitch-free seeks possible. The audio callback only ever
//! sees finished chunks (see `pipe.rs`).
//!
//! **One conversion stream across tracks.** The mixer and resampler are not per track. As long as
//! consecutive tracks share a format they are fed through the same resampler, so a continuous
//! recording split into tracks is resampled as the continuous signal it is (no filter ringing or
//! sub-sample phase slip at the join). Output frames are attributed back to the track they belong
//! to by cumulative frame index (`Owner`/`Seg`), which keeps positions, ReplayGain and the
//! "now playing" switch exact to the sample.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::decode::{self, Opened, Step};
use super::mixdown::{Mixer, Role};
use super::output::{Backend, Format};
use super::pipe::{Chunk, Gain, Kind, PipeProducer, Shared, pipe};
use super::queue::{Item, Queue};
use super::resample::StreamResampler;

/// Maps a library track id to its file.
pub type Resolver = Arc<dyn Fn(u32) -> Option<PathBuf> + Send + Sync>;

pub enum Cmd {
    /// Start playing a queue entry from the beginning.
    Play(u32),
    /// Continue after a pause; if the queue already ended, replay the last track.
    Resume,
    /// Seek within the audible track.
    Seek(u64),
    Next,
    Prev,
    /// The queue was edited: anything prepared from it is stale.
    QueueChanged,
    SetDevice(Option<String>),
    Shutdown,
}

#[derive(Clone, Debug)]
pub enum Event {
    /// Something the user should see (unreadable file, no audio device...).
    Error(String),
    /// The output device was (re)opened.
    Output { device: String, rate: u32, channels: usize },
}

/// Open the next track this long before the current one ends.
const PREFETCH_SECS: u64 = 8;
const FLUSH_TIMEOUT: Duration = Duration::from_millis(400);
const RETRY_DEVICE_EVERY: Duration = Duration::from_secs(3);
/// How many unplayable tracks in a row are skipped before giving up.
const MAX_SKIPS: usize = 16;

struct Out {
    prod: PipeProducer,
    rate: u32,
    ch: usize,
}

/// Channel mixer + (optional) resampler for one stream format.
struct Conv {
    rate: u32,
    roles: Vec<Role>,
    mixer: Mixer,
    rs: Option<StreamResampler>,
    /// The track that fed this converter most recently.
    last_uid: u32,
}

/// The track being decoded.
struct Cur {
    uid: u32,
    open: Opened,
    gains: (Gain, Gain),
    /// Native frames read from the decoder so far (including skipped ones).
    src_pos: u64,
    /// Native frames still to be dropped after an accurate seek.
    skip_src: u64,
    /// The demuxer is exhausted (or failed) and the track's end is marked in the stream.
    eof: bool,
}

struct Prepared {
    uid: u32,
    open: Opened,
}

/// A track's claim on the output stream: its frames are the cumulative indices up to `end`.
struct Owner {
    uid: u32,
    gains: (Gain, Gain),
    /// Position within the track of the next frame to be attributed.
    pos: u64,
    /// Cumulative output index where this track's audio ends, once all its input is known.
    end: Option<u64>,
}

/// A run of staged frames that belong to one track.
struct Seg {
    uid: u32,
    gains: (Gain, Gain),
    pos: u64,
    frames: usize,
}

enum Pump {
    Worked,
    Blocked,
    Idle,
}

enum Outcome {
    Worked,
    Done,
}

pub struct Engine {
    rx: Receiver<Cmd>,
    events: Sender<Event>,
    shared: Arc<Shared>,
    queue: Arc<Mutex<Queue>>,
    resolve: Resolver,
    backend: Box<dyn Backend>,
    device: Option<String>,
    out: Option<Out>,
    retry_at: Instant,
    out_error_reported: bool,
    cur: Option<Cur>,
    prepared: Option<Prepared>,
    /// Uid for which a prefetch was already attempted.
    prefetch_for: u32,

    conv: Option<Conv>,
    owners: VecDeque<Owner>,
    /// Output frames produced / attributed to owners since the stream started.
    stream_out: u64,
    attributed: u64,
    /// Uid and final position of the most recently completed track (for the End marker).
    tail_mark: (u32, u64),

    fill: Option<Box<Chunk>>,
    /// Converted frames (device format) not yet copied into chunks, in `segs` order.
    stage: Vec<f32>,
    stage_pos: usize,
    segs: VecDeque<Seg>,
    scratch_in: Vec<f32>,
    scratch_map: Vec<f32>,
    end_pending: Option<(u32, u64)>,
}

fn ms_to_frames(ms: u64, rate: u32) -> u64 {
    ms * rate as u64 / 1000
}

impl Engine {
    pub fn new(
        rx: Receiver<Cmd>,
        events: Sender<Event>,
        shared: Arc<Shared>,
        queue: Arc<Mutex<Queue>>,
        resolve: Resolver,
        backend: Box<dyn Backend>,
        device: Option<String>,
    ) -> Engine {
        Engine {
            rx,
            events,
            shared,
            queue,
            resolve,
            backend,
            device,
            out: None,
            retry_at: Instant::now(),
            out_error_reported: false,
            cur: None,
            prepared: None,
            prefetch_for: 0,
            conv: None,
            owners: VecDeque::new(),
            stream_out: 0,
            attributed: 0,
            tail_mark: (0, 0),
            fill: None,
            stage: Vec::new(),
            stage_pos: 0,
            segs: VecDeque::new(),
            scratch_in: Vec::new(),
            scratch_map: Vec::new(),
            end_pending: None,
        }
    }

    fn lock_queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn report(&self, msg: impl Into<String>) {
        let _ = self.events.send(Event::Error(msg.into()));
    }

    // -- main loop ------------------------------------------------------------------------

    pub fn run(mut self) {
        self.open_output();
        let mut wait = Duration::ZERO;
        loop {
            let got = if wait.is_zero() {
                self.rx.try_recv().map_err(|e| matches!(e, TryRecvError::Disconnected))
            } else {
                self.rx.recv_timeout(wait).map_err(|e| matches!(e, RecvTimeoutError::Disconnected))
            };
            match got {
                Ok(Cmd::Shutdown) | Err(true) => break,
                Ok(cmd) => {
                    self.guard(|e| e.handle(cmd));
                    wait = Duration::ZERO;
                    continue;
                }
                Err(false) => {}
            }

            if self.shared.device_lost.swap(false, Relaxed) {
                self.guard(|e| e.recover_output());
            } else if self.out.is_none() && Instant::now() >= self.retry_at {
                self.guard(|e| e.recover_output());
            }

            wait = match self.guard(|e| e.pump()).unwrap_or(Pump::Worked) {
                Pump::Worked => Duration::ZERO,
                Pump::Blocked => Duration::from_millis(8),
                Pump::Idle => Duration::from_millis(100),
            };
        }
        self.shared.playing.store(false, Relaxed);
        self.backend.close();
    }

    /// Runs `f`, turning a decoder panic (corrupt file) into "skip this track".
    fn guard<R>(&mut self, f: impl FnOnce(&mut Engine) -> R) -> Option<R> {
        match catch_unwind(AssertUnwindSafe(|| f(self))) {
            Ok(r) => Some(r),
            Err(_) => {
                self.report("The audio decoder crashed on this file; skipping it.");
                self.abandon_current();
                None
            }
        }
    }

    fn abandon_current(&mut self) {
        let uid = self.cur.take().map_or(0, |c| c.uid);
        self.prepared = None;
        self.reset_pipe_only();
        let next = self.lock_queue().next(uid);
        match next {
            Some(item) if item.uid != uid => self.start_item(item, 0),
            _ => {
                self.shared.loaded.store(false, Relaxed);
                self.end_pending = Some((uid, 0));
            }
        }
    }

    // -- commands -------------------------------------------------------------------------

    fn audible_uid(&self) -> u32 {
        let (uid, _) = self.shared.now();
        if uid != 0 { uid } else { self.cur.as_ref().map_or(0, |c| c.uid) }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play(uid) => {
                let item = self.lock_queue().get(uid);
                if let Some(item) = item {
                    self.play_item(item, 0);
                }
            }
            Cmd::Resume => {
                let (uid, _) = self.shared.now();
                let buffered = self.out.as_ref().is_some_and(|o| o.prod.queued() > 0);
                if self.cur.is_some() || self.end_pending.is_some() || !self.segs.is_empty() || buffered {
                    self.shared.playing.store(true, Relaxed);
                } else if uid != 0 {
                    // Nothing left to play (the queue ended): start the last track again.
                    let item = self.lock_queue().get(uid);
                    if let Some(item) = item {
                        self.play_item(item, 0);
                    }
                }
            }
            Cmd::Seek(ms) => self.seek(ms),
            Cmd::Next => {
                let next = self.lock_queue().next(self.audible_uid());
                if let Some(item) = next {
                    self.play_item(item, 0);
                }
            }
            Cmd::Prev => {
                let (uid, ms) = self.shared.now();
                let prev = self.lock_queue().prev(uid);
                match prev {
                    // More than 3 s in: restart the track, like every player does.
                    Some(item) if ms <= 3000 => self.play_item(item, 0),
                    _ => self.seek(0),
                }
            }
            Cmd::QueueChanged => {
                self.prepared = None;
                self.prefetch_for = 0;
            }
            Cmd::SetDevice(device) => {
                self.device = device;
                self.recover_output();
            }
            Cmd::Shutdown => {}
        }
    }

    fn play_item(&mut self, item: Item, start_ms: u64) {
        self.shared.playing.store(true, Relaxed);
        self.reset_stream();
        self.start_item(item, start_ms);
        if self.cur.is_some() {
            self.shared.set_now(item.uid, start_ms as u32);
        }
    }

    fn seek(&mut self, ms: u64) {
        let (uid, _) = self.shared.now();
        if uid == 0 {
            return;
        }
        let in_place = self.out.is_some() && self.cur.as_ref().is_some_and(|c| c.uid == uid);
        if in_place {
            self.reset_pipe_only();
            let rate = self.out.as_ref().map_or(48_000, |o| o.rate);
            let result = {
                let cur = self.cur.as_mut().expect("checked above");
                cur.open.seek(ms).map(|skip| {
                    cur.skip_src = skip;
                    cur.src_pos = ms_to_frames(ms, cur.open.declared_rate());
                    cur.eof = false;
                    Owner { uid, gains: cur.gains, pos: ms_to_frames(ms, rate), end: None }
                })
            };
            match result {
                Ok(owner) => {
                    self.owners.push_back(owner);
                    self.shared.set_now(uid, ms as u32);
                }
                Err(e) => self.report(e),
            }
        } else if self.shared.playing.load(Relaxed) {
            // The decoder already moved on to the next track while this one's tail is still
            // buffered; re-open the audible one at the requested position.
            let item = self.lock_queue().get(uid);
            if let Some(item) = item {
                self.play_item(item, ms);
            }
        }
    }

    /// Fades out and drops all buffered audio and the in-flight decode state.
    fn reset_stream(&mut self) {
        self.reset_pipe_only();
        self.cur = None;
        self.shared.loaded.store(false, Relaxed);
    }

    /// Drops buffered audio, staged frames and the conversion stream, keeping the open track.
    fn reset_pipe_only(&mut self) {
        if let Some(out) = &self.out {
            if let Some(c) = self.fill.take() {
                out.prod.give_back(c);
            }
            if !out.prod.flush(FLUSH_TIMEOUT) {
                // The callback is not answering: the stream is dead.
                self.shared.device_lost.store(true, Relaxed);
            }
        }
        self.fill = None;
        self.stage.clear();
        self.stage_pos = 0;
        self.segs.clear();
        self.owners.clear();
        self.conv = None;
        self.stream_out = 0;
        self.attributed = 0;
        self.end_pending = None;
    }

    // -- output device --------------------------------------------------------------------

    fn open_output(&mut self) {
        self.out = None;
        self.backend.close();
        self.reset_pipe_only();
        let shared = self.shared.clone();
        let mut producer = None;
        let result = self.backend.open(self.device.as_deref(), &mut |fmt: &Format| {
            let (p, r) = pipe(shared.clone(), fmt.rate, fmt.channels);
            producer = Some(p);
            r
        });
        match (result, producer) {
            (Ok(fmt), Some(prod)) => {
                self.out = Some(Out { prod, rate: fmt.rate, ch: fmt.channels });
                self.shared.device_lost.store(false, Relaxed);
                self.out_error_reported = false;
                let _ = self.events.send(Event::Output { device: fmt.name, rate: fmt.rate, channels: fmt.channels });
            }
            (Err(e), _) => {
                if !self.out_error_reported {
                    self.report(format!("Audio output unavailable: {e}"));
                    self.out_error_reported = true;
                }
                self.retry_at = Instant::now() + RETRY_DEVICE_EVERY;
            }
            (Ok(_), None) => unreachable!("the backend must call `make`"),
        }
    }

    /// Reopens the device and resumes the audible track where it was.
    fn recover_output(&mut self) {
        let (uid, ms) = self.shared.now();
        let was_playing = self.shared.playing.load(Relaxed);
        let had_track = uid != 0 && self.shared.loaded.load(Relaxed);
        self.cur = None;
        self.prepared = None;
        self.open_output();
        if self.out.is_some() && had_track {
            let item = self.lock_queue().get(uid);
            if let Some(item) = item {
                self.start_item(item, ms as u64);
                self.shared.playing.store(was_playing, Relaxed);
                self.shared.set_now(uid, ms);
            }
        }
    }

    // -- starting tracks ------------------------------------------------------------------

    fn open_cur(&mut self, item: Item, start_ms: u64) -> Result<(Cur, u64), String> {
        let out_rate = self.out.as_ref().map(|o| o.rate).ok_or("No audio output device")?;
        let open = match self.prepared.take() {
            Some(p) if p.uid == item.uid && start_ms == 0 => p.open,
            _ => {
                let path = (self.resolve)(item.track).ok_or("This track is no longer in the library")?;
                decode::open(&path)?
            }
        };
        let gains = open.rg.gains();
        let mut cur = Cur { uid: item.uid, open, gains, src_pos: 0, skip_src: 0, eof: false };
        if start_ms > 0 {
            cur.skip_src = cur.open.seek(start_ms)?;
            cur.src_pos = ms_to_frames(start_ms, cur.open.declared_rate());
        }
        Ok((cur, ms_to_frames(start_ms, out_rate)))
    }

    /// Makes `item` the decoding track. Unplayable files are reported and skipped.
    fn start_item(&mut self, item: Item, start_ms: u64) {
        let mut item = item;
        let mut start_ms = start_ms;
        for _ in 0..MAX_SKIPS {
            match self.open_cur(item, start_ms) {
                Ok((cur, pos)) => {
                    self.owners.push_back(Owner { uid: cur.uid, gains: cur.gains, pos, end: None });
                    self.cur = Some(cur);
                    self.shared.loaded.store(true, Relaxed);
                    self.prefetch_for = 0;
                    return;
                }
                Err(msg) => {
                    self.report(msg);
                    start_ms = 0;
                    let next = self.lock_queue().next(item.uid);
                    match next {
                        Some(n) if n.uid != item.uid => item = n,
                        _ => break,
                    }
                }
            }
        }
        // Nothing playable: end the queue here.
        self.cur = None;
        self.shared.loaded.store(false, Relaxed);
        self.flush_conv();
        self.end_pending = Some(self.tail_mark);
    }

    fn maybe_prefetch(&mut self, cur: &Cur) {
        if self.prepared.is_some() || self.prefetch_for == cur.uid {
            return;
        }
        let rate = cur.open.declared_rate() as u64;
        let Some(total) = cur.open.total_frames.filter(|_| rate > 0) else { return };
        if total.saturating_sub(cur.src_pos) > PREFETCH_SECS * rate {
            return;
        }
        self.prefetch_for = cur.uid;
        let next = self.lock_queue().after_end(cur.uid);
        if let Some(item) = next {
            if let Some(path) = (self.resolve)(item.track) {
                if let Ok(open) = decode::open(&path) {
                    self.prepared = Some(Prepared { uid: item.uid, open });
                }
            }
        }
    }

    // -- the pump -------------------------------------------------------------------------

    fn pump(&mut self) -> Pump {
        if self.out.is_none() {
            return Pump::Idle;
        }
        if !self.segs.is_empty() && !self.deliver() {
            return Pump::Blocked;
        }
        if let Some((uid, pos)) = self.end_pending {
            let out = self.out.as_ref().expect("checked above");
            return match out.prod.take_free() {
                Some(mut c) => {
                    c.kind = Kind::End;
                    c.uid = uid;
                    c.pos = pos;
                    c.len = 0;
                    out.prod.push(c);
                    self.end_pending = None;
                    Pump::Worked
                }
                None => Pump::Blocked,
            };
        }
        let Some(mut cur) = self.cur.take() else { return Pump::Idle };
        match self.pump_cur(&mut cur) {
            Outcome::Worked => {
                self.cur = Some(cur);
                Pump::Worked
            }
            Outcome::Done => {
                self.finish_track(cur);
                Pump::Worked
            }
        }
    }

    fn pump_cur(&mut self, cur: &mut Cur) -> Outcome {
        if cur.eof {
            return Outcome::Done;
        }
        self.maybe_prefetch(cur);
        match cur.open.next(&mut self.scratch_in) {
            Ok(Step::Frames(n)) => {
                cur.src_pos += n as u64;
                if let Err(e) = self.convert(cur, n) {
                    self.report(format!("Playback error: {e}"));
                    self.end_decode(cur);
                }
            }
            Ok(Step::Eof) => self.end_decode(cur),
            Err(e) => {
                self.report(format!("Playback error: {e}"));
                self.end_decode(cur);
            }
        }
        Outcome::Worked
    }

    /// Cumulative output index the stream will reach once everything fed so far is flushed.
    fn expected_total(&self) -> u64 {
        match self.conv.as_ref().and_then(|c| c.rs.as_ref()) {
            Some(rs) => rs.expected_out(),
            None => self.stream_out,
        }
    }

    /// The source is exhausted (or broken): mark where this track's audio ends in the stream.
    /// The resampler is *not* flushed: the next track (if compatible) continues through it.
    fn end_decode(&mut self, cur: &mut Cur) {
        let end = self.expected_total();
        if let Some(o) = self.owners.iter_mut().find(|o| o.uid == cur.uid && o.end.is_none()) {
            o.end = Some(end);
        }
        cur.eof = true;
        self.attribute();
    }

    /// Flushes the converter's tail (end of queue, or the format changes) and closes the stream.
    fn flush_conv(&mut self) {
        let ch = self.out.as_ref().map_or(2, |o| o.ch);
        if let Some(mut conv) = self.conv.take() {
            let before = self.stage.len();
            if let Some(rs) = conv.rs.as_mut() {
                if let Err(e) = rs.finish(&mut self.stage) {
                    self.report(e);
                }
            }
            self.stream_out += ((self.stage.len() - before) / ch) as u64;
        }
        // Whoever is still open ends with the stream.
        let end = self.stream_out;
        if let Some(o) = self.owners.back_mut() {
            if o.end.is_none() {
                o.end = Some(end);
            }
        }
        self.attribute();
        self.stream_out = 0;
        self.attributed = 0;
    }

    /// Assigns newly produced frames to the tracks they belong to.
    fn attribute(&mut self) {
        while self.attributed < self.stream_out {
            let Some(front) = self.owners.front_mut() else {
                // Cannot happen (an owner exists before its first packet is converted); keep the
                // staged frames and segments in step anyway.
                let orphan = (self.stream_out - self.attributed) as usize;
                let ch = self.out.as_ref().map_or(2, |o| o.ch);
                let keep = self.stage.len().saturating_sub(orphan * ch);
                self.stage.truncate(keep.max(self.stage_pos));
                self.attributed = self.stream_out;
                break;
            };
            let room = front.end.map_or(u64::MAX, |e| e.saturating_sub(self.attributed));
            let take = (self.stream_out - self.attributed).min(room);
            if take > 0 {
                match self.segs.back_mut() {
                    Some(s) if s.uid == front.uid && s.pos + s.frames as u64 == front.pos => {
                        s.frames += take as usize;
                    }
                    _ => self.segs.push_back(Seg {
                        uid: front.uid,
                        gains: front.gains,
                        pos: front.pos,
                        frames: take as usize,
                    }),
                }
                front.pos += take;
                self.attributed += take;
            }
            if front.end.is_some_and(|e| self.attributed >= e) {
                self.tail_mark = (front.uid, front.pos);
                self.owners.pop_front();
            } else {
                break;
            }
        }
        // A finished owner with nothing left to say (empty track) must not block the queue.
        while self.owners.front().is_some_and(|o| o.end.is_some_and(|e| self.attributed >= e)) {
            if let Some(o) = self.owners.pop_front() {
                self.tail_mark = (o.uid, o.pos);
            }
        }
    }

    /// Channel mix + resample the packet in `scratch_in` into `stage`.
    fn convert(&mut self, cur: &mut Cur, frames: usize) -> Result<(), String> {
        let (out_rate, out_ch) = {
            let out = self.out.as_ref().ok_or("no output")?;
            (out.rate, out.ch)
        };
        let (rate, roles) = (cur.open.rate(), cur.open.roles());
        if roles.is_empty() || rate == 0 {
            return Err("unsupported channel layout".into());
        }
        let stale = self.conv.as_ref().is_none_or(|c| c.rate != rate || c.roles != roles);
        if stale {
            if self.conv.as_ref().is_some_and(|c| c.last_uid == cur.uid) {
                return Err("the stream changed format mid-file".into());
            }
            // Track boundary with a different format: finish the old stream, start a new one.
            // The previous track's end is already marked; this one's owner is still queued.
            self.flush_conv_keep_owners();
            let rs = if rate != out_rate { Some(StreamResampler::new(rate, out_rate, out_ch)?) } else { None };
            self.conv = Some(Conv { rate, roles: roles.to_vec(), mixer: Mixer::new(roles, out_ch), rs, last_uid: cur.uid });
        }
        let src_ch = roles.len();
        let mut data = &self.scratch_in[..frames * src_ch];
        if cur.skip_src > 0 {
            let drop = (cur.skip_src as usize).min(frames);
            cur.skip_src -= drop as u64;
            data = &data[drop * src_ch..];
            if data.is_empty() {
                return Ok(());
            }
        }
        let conv = self.conv.as_mut().expect("just ensured");
        conv.last_uid = cur.uid;
        self.scratch_map.clear();
        conv.mixer.process(data, &mut self.scratch_map);
        let before = self.stage.len();
        match conv.rs.as_mut() {
            Some(rs) => rs.push(&self.scratch_map, &mut self.stage)?,
            None => self.stage.extend_from_slice(&self.scratch_map),
        }
        self.stream_out += ((self.stage.len() - before) / out_ch) as u64;
        self.attribute();
        Ok(())
    }

    /// Like `flush_conv`, but for a format change at a track boundary: the owners queued for the
    /// *next* track must survive, and counters restart for the new stream.
    fn flush_conv_keep_owners(&mut self) {
        let ch = self.out.as_ref().map_or(2, |o| o.ch);
        if let Some(mut conv) = self.conv.take() {
            let before = self.stage.len();
            if let Some(rs) = conv.rs.as_mut() {
                if let Err(e) = rs.finish(&mut self.stage) {
                    self.report(e);
                }
            }
            self.stream_out += ((self.stage.len() - before) / ch) as u64;
        }
        // Everything produced so far belongs to tracks that already ended; their ends are known.
        // Any owner whose end is still open is the *next* track (nothing produced for it yet).
        self.attribute();
        // Restart cumulative counting for the new stream: re-base the remaining owners' ends.
        let base = self.stream_out;
        for o in self.owners.iter_mut() {
            if let Some(e) = o.end.as_mut() {
                *e = e.saturating_sub(base);
            }
        }
        self.stream_out = 0;
        self.attributed = 0;
    }

    /// Copies staged frames into chunks. Returns `false` when it ran out of free chunks.
    fn deliver(&mut self) -> bool {
        let Some(out) = self.out.as_ref() else { return false };
        while let Some(seg) = self.segs.front_mut() {
            while seg.frames > 0 {
                if self.fill.is_none() {
                    match out.prod.take_free() {
                        Some(c) => self.fill = Some(c),
                        None => return false,
                    }
                }
                let c = self.fill.as_mut().expect("just ensured");
                if c.len == 0 {
                    c.kind = Kind::Audio;
                    c.uid = seg.uid;
                    c.pos = seg.pos;
                    c.track_gain = seg.gains.0;
                    c.album_gain = seg.gains.1;
                }
                let want = seg.frames.min(c.capacity() - c.len);
                let avail = &self.stage[self.stage_pos..self.stage_pos + want * out.ch];
                let taken = c.append(avail);
                self.stage_pos += taken * out.ch;
                seg.frames -= taken;
                seg.pos += taken as u64;
                if c.is_full() {
                    if let Some(c) = self.fill.take() {
                        out.prod.push(c);
                    }
                }
            }
            self.segs.pop_front();
            // The next segment may belong to another track: a chunk never mixes tracks.
            if let (Some(c), Some(out)) = (self.fill.take(), self.out.as_ref()) {
                if c.len > 0 {
                    out.prod.push(c);
                } else {
                    self.fill = Some(c);
                }
            }
        }
        if self.stage_pos >= self.stage.len() {
            self.stage.clear();
            self.stage_pos = 0;
            if self.stage.capacity() > (4 << 20) / 4 {
                self.stage.shrink_to(1 << 16);
            }
        }
        true
    }

    /// The track is fully decoded: continue with whatever the queue says.
    fn finish_track(&mut self, cur: Cur) {
        let next = self.lock_queue().after_end(cur.uid);
        match next {
            Some(item) => self.start_item(item, 0),
            None => {
                self.shared.loaded.store(false, Relaxed);
                self.flush_conv();
                self.end_pending = Some(self.tail_mark);
            }
        }
    }
}
