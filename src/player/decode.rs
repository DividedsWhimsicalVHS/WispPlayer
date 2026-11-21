//! Opening audio files and turning packets into interleaved `f32`.
//!
//! Symphonia demuxes and decodes everything except Opus, which it only demuxes; the Opus packets
//! are handed to a pure-Rust Opus decoder behind the same `Decoder` enum. Callers never see the
//! difference: `Opened::next` yields interleaved f32 frames at the codec's native rate.

use std::fs::File;
use std::path::{Path, PathBuf};

use opus_decoder::{OpusDecoder, OpusMultistreamDecoder};
use symphonia::core::audio::{Channels, Position};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::{Error as SymError, SeekErrorKind};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, Track, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::{MetadataOptions, RawValue, StandardTag};
use symphonia::core::packet::Packet;
use symphonia::core::units::{Duration, Time, TimeBase};

use super::mixdown::{Role, roles_from};
use super::replaygain::ReplayGain;

/// Gives up on a file after this many undecodable packets in a row.
const MAX_CONSECUTIVE_ERRORS: u32 = 64;
/// A seek that lands this much after its target did not really seek (a packet is far shorter).
const MAX_LATE_MS: u64 = 250;
/// 120 ms at 48 kHz: the largest Opus frame group.
const OPUS_MAX_FRAMES: usize = 5760;

/// What the decoder reported for the most recent packet.
pub enum Step {
    /// `n` frames were appended (replacing the buffer) at `Opened::rate()`.
    Frames(usize),
    Eof,
}

enum Decoder {
    Sym(Box<dyn AudioDecoder>),
    Opus(OpusDec),
}

pub struct Opened {
    path: PathBuf,
    fmt: Box<dyn FormatReader>,
    dec: Decoder,
    track_id: u32,
    time_base: Option<TimeBase>,
    declared_rate: u32,
    /// Playable frames at the native rate (excludes encoder delay/padding), when known.
    pub total_frames: Option<u64>,
    pub rg: ReplayGain,
    cur_rate: u32,
    cur_layout: Channels,
    cur_roles: Vec<Role>,
    errors: u32,
}

pub fn short_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Opens the container and reads its tags. `with_visuals` is accepted for symmetry with the
/// artwork loader; see `art.rs` for the picture path.
pub fn open_format(path: &Path) -> Result<Box<dyn FormatReader>, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", short_name(path)))?;
    let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| format!("{}: {e}", short_name(path)))
}

/// The decoder for an audio track: Symphonia's, or the separate Opus one.
fn make_decoder(params: &AudioCodecParameters) -> Result<Decoder, String> {
    if params.codec == CODEC_ID_OPUS {
        return OpusDec::new(params).map(Decoder::Opus);
    }
    symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map(Decoder::Sym)
        .map_err(|e| format!("unsupported audio format ({e})"))
}

/// Whether this audio can be decoded here. The library scan asks, so that a file whose sound
/// nothing here decodes (Dolby Digital or 5.1 AAC in a film, say) is left out instead of being
/// listed as a song that never plays.
pub fn can_decode(params: &AudioCodecParameters) -> bool {
    make_decoder(params).is_ok()
}

/// Duration in milliseconds from whatever the container knows: the track's own length or, when it
/// states none (Matroska and WebM only give the length of the whole file), that.
pub fn track_duration_ms(fmt: &dyn FormatReader, track: &Track, rate: Option<u32>) -> Option<u64> {
    if let (Some(n), Some(rate)) = (track.num_frames, rate.filter(|r| *r > 0)) {
        return Some(n * 1000 / rate as u64);
    }
    let millis = |tb: Option<TimeBase>, dur: Option<Duration>| tb?.calc_duration(dur?).and_then(|t| u64::try_from(t.as_millis()).ok());
    let media = fmt.media_info();
    millis(track.time_base, track.duration).or_else(|| millis(media.time_base, media.duration))
}

fn read_replaygain(fmt: &mut dyn FormatReader) -> ReplayGain {
    let mut rg = ReplayGain::default();
    let mut md = fmt.metadata();
    loop {
        if let Some(rev) = md.current() {
            let tags = rev.media.tags.iter().chain(rev.per_track.iter().flat_map(|t| t.metadata.tags.iter()));
            for tag in tags {
                match &tag.std {
                    Some(StandardTag::ReplayGainTrackGain(v)) => rg.apply_tag("REPLAYGAIN_TRACK_GAIN", v),
                    Some(StandardTag::ReplayGainTrackPeak(v)) => rg.apply_tag("REPLAYGAIN_TRACK_PEAK", v),
                    Some(StandardTag::ReplayGainAlbumGain(v)) => rg.apply_tag("REPLAYGAIN_ALBUM_GAIN", v),
                    Some(StandardTag::ReplayGainAlbumPeak(v)) => rg.apply_tag("REPLAYGAIN_ALBUM_PEAK", v),
                    _ => {
                        if let RawValue::String(s) = &tag.raw.value {
                            rg.apply_tag(&tag.raw.key, s);
                        }
                    }
                }
            }
        }
        if md.pop().is_none() {
            break;
        }
    }
    rg
}

pub fn open(path: &Path) -> Result<Opened, String> {
    let mut fmt = open_format(path)?;
    let name = short_name(path);
    let track = fmt
        .default_track(TrackType::Audio)
        .ok_or_else(|| format!("{name}: no audio track"))?;
    let track_id = track.id;
    let time_base = track.time_base;
    let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
        return Err(format!("{name}: no audio codec parameters"));
    };
    let declared_rate = params.sample_rate.unwrap_or(0);
    let total_frames = track
        .num_frames
        .or_else(|| track_duration_ms(&*fmt, track, params.sample_rate).map(|ms| ms * declared_rate as u64 / 1000));

    let dec = make_decoder(&params).map_err(|e| format!("{name}: {e}"))?;
    let rg = read_replaygain(&mut *fmt);

    Ok(Opened {
        path: path.to_path_buf(),
        fmt,
        dec,
        track_id,
        time_base,
        declared_rate,
        total_frames,
        rg,
        cur_rate: declared_rate,
        cur_layout: Channels::None,
        cur_roles: Vec::new(),
        errors: 0,
    })
}

impl Opened {
    /// Sample rate of the most recently decoded packet (the declared rate before the first).
    pub fn rate(&self) -> u32 {
        self.cur_rate
    }

    pub fn declared_rate(&self) -> u32 {
        self.declared_rate
    }

    /// Channel roles of the most recently decoded packet.
    pub fn roles(&self) -> &[Role] {
        &self.cur_roles
    }

    /// Decodes the next packet into `buf` (replacing its contents).
    pub fn next(&mut self, buf: &mut Vec<f32>) -> Result<Step, String> {
        loop {
            let packet = match self.fmt.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(Step::Eof),
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(Step::Eof);
                }
                Err(SymError::ResetRequired) => return Err("the stream changed format mid-file".into()),
                Err(e) => return Err(e.to_string()),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let decoded = match &mut self.dec {
                Decoder::Sym(d) => decode_symphonia(d.as_mut(), &packet, buf),
                Decoder::Opus(o) => o.decode(&packet, buf),
            };
            match decoded {
                Ok(info) => {
                    self.errors = 0;
                    if info.frames == 0 {
                        continue;
                    }
                    self.cur_rate = info.rate;
                    if self.cur_layout != info.channels {
                        self.cur_roles = roles_from(&info.channels);
                        self.cur_layout = info.channels;
                    }
                    return Ok(Step::Frames(info.frames));
                }
                // A corrupt packet is skipped; playback continues with the next one.
                Err(SymError::DecodeError(_)) | Err(SymError::IoError(_)) => {
                    self.errors += 1;
                    if self.errors > MAX_CONSECUTIVE_ERRORS {
                        return Err("too many corrupt packets".into());
                    }
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    /// Seeks to `ms`. Returns how many native-rate frames must be discarded after decoding resumes
    /// (the container can only land on a packet boundary at or before the target).
    pub fn seek(&mut self, ms: u64) -> Result<u64, String> {
        let first = match self.seek_here(ms) {
            Ok(skip) => return Ok(skip),
            Err(e) => e,
        };
        if matches!(first, SymError::SeekError(_)) {
            // A Matroska or WebM file without an index (browsers write them so) can only seek
            // forward from where it is: start over from the top, which is always forward.
            if let Ok(mut fresh) = open(&self.path) {
                if let Ok(skip) = fresh.seek_here(ms) {
                    *self = fresh;
                    return Ok(skip);
                }
            }
        }
        Err(format!("seek failed: {first}"))
    }

    fn seek_here(&mut self, ms: u64) -> Result<u64, SymError> {
        let to = SeekTo::Time { time: Time::from_millis_u64(ms), track_id: Some(self.track_id) };
        let seeked = self.fmt.seek(SeekMode::Accurate, to)?;
        // A container that cannot go back reports success but lands on the next frame after where
        // it is, however far past the target that is: that is a failed seek, not a late one.
        let late = seeked.actual_ts.duration_from(seeked.required_ts).zip(self.time_base);
        if late.and_then(|(d, tb)| tb.calc_duration(d)).is_some_and(|t| u64::try_from(t.as_millis()).is_ok_and(|ms| ms > MAX_LATE_MS)) {
            return Err(SymError::SeekError(SeekErrorKind::OutOfRange));
        }
        match &mut self.dec {
            Decoder::Sym(d) => d.reset(),
            Decoder::Opus(o) => o.reset(),
        }
        self.errors = 0;
        let skip = seeked.required_ts.duration_from(seeked.actual_ts).map_or(0, |d| match self.time_base {
            Some(tb) => tb
                .calc_duration(d)
                .map_or(d.get(), |t| (t.as_secs_f64() * self.cur_rate.max(self.declared_rate) as f64).round() as u64),
            None => d.get(),
        });
        Ok(skip)
    }
}

struct Info {
    frames: usize,
    rate: u32,
    channels: Channels,
}

fn decode_symphonia(dec: &mut dyn AudioDecoder, packet: &Packet, buf: &mut Vec<f32>) -> Result<Info, SymError> {
    let decoded = dec.decode(packet)?;
    let spec = decoded.spec();
    let info = Info { frames: decoded.frames(), rate: spec.rate(), channels: spec.channels().clone() };
    decoded.copy_to_vec_interleaved::<f32>(buf);
    Ok(info)
}

// ---------------------------------------------------------------------------------------------
// Opus
// ---------------------------------------------------------------------------------------------

enum OpusCore {
    Single(OpusDecoder),
    Multi(OpusMultistreamDecoder),
}

struct OpusDec {
    core: OpusCore,
    channels: usize,
    /// Output gain from the header, linear.
    gain: f32,
    /// For surround streams: source channel index for every output channel (WAVE order).
    perm: Option<&'static [usize]>,
    layout: Channels,
    pcm: Vec<f32>,
}

struct OpusHead {
    channels: usize,
    gain_q8: i16,
    family: u8,
    streams: usize,
    coupled: usize,
    mapping: Vec<u8>,
}

fn parse_opus_head(b: &[u8]) -> Option<OpusHead> {
    if b.len() < 19 || &b[..8] != b"OpusHead" {
        return None;
    }
    let channels = b[9] as usize;
    let gain_q8 = i16::from_le_bytes([b[16], b[17]]);
    let family = b[18];
    let (mut streams, mut coupled, mut mapping) = (1, 0, Vec::new());
    if family != 0 {
        if b.len() < 21 + channels {
            return None;
        }
        streams = b[19] as usize;
        coupled = b[20] as usize;
        mapping = b[21..21 + channels].to_vec();
    }
    Some(OpusHead { channels, gain_q8, family, streams, coupled, mapping })
}

/// Vorbis channel order -> WAVE order, for 3..=8 channel Opus (mapping family 1).
fn vorbis_layout(channels: usize) -> Option<(Position, &'static [usize])> {
    use Position as P;
    Some(match channels {
        3 => (P::FRONT_LEFT | P::FRONT_RIGHT | P::FRONT_CENTER, &[0, 2, 1]),
        4 => (P::FRONT_LEFT | P::FRONT_RIGHT | P::REAR_LEFT | P::REAR_RIGHT, &[0, 1, 2, 3]),
        5 => (P::FRONT_LEFT | P::FRONT_RIGHT | P::FRONT_CENTER | P::REAR_LEFT | P::REAR_RIGHT, &[0, 2, 1, 3, 4]),
        6 => (
            P::FRONT_LEFT | P::FRONT_RIGHT | P::FRONT_CENTER | P::LFE1 | P::REAR_LEFT | P::REAR_RIGHT,
            &[0, 2, 1, 5, 3, 4],
        ),
        7 => (
            P::FRONT_LEFT | P::FRONT_RIGHT | P::FRONT_CENTER | P::LFE1 | P::REAR_CENTER | P::SIDE_LEFT | P::SIDE_RIGHT,
            &[0, 2, 1, 6, 5, 3, 4],
        ),
        8 => (
            P::FRONT_LEFT
                | P::FRONT_RIGHT
                | P::FRONT_CENTER
                | P::LFE1
                | P::REAR_LEFT
                | P::REAR_RIGHT
                | P::SIDE_LEFT
                | P::SIDE_RIGHT,
            &[0, 2, 1, 7, 5, 6, 3, 4],
        ),
        _ => return None,
    })
}

impl OpusDec {
    fn new(params: &AudioCodecParameters) -> Result<OpusDec, String> {
        let extra = params.extra_data.as_deref().ok_or("Opus stream without a header")?;
        let head = parse_opus_head(extra).ok_or("invalid Opus header")?;
        let gain = 10f32.powf(head.gain_q8 as f32 / 256.0 / 20.0);
        let err = |e: opus_decoder::OpusError| format!("Opus decoder: {e}");
        let (core, perm, layout) = match (head.family, head.channels) {
            (0, 1) => (OpusCore::Single(OpusDecoder::new(48_000, 1).map_err(err)?), None, Channels::Positioned(Position::FRONT_CENTER)),
            (0, 2) => (
                OpusCore::Single(OpusDecoder::new(48_000, 2).map_err(err)?),
                None,
                Channels::Positioned(Position::FRONT_LEFT | Position::FRONT_RIGHT),
            ),
            (1, n @ 3..=8) => {
                let (pos, perm) = vorbis_layout(n).ok_or("unsupported Opus channel layout")?;
                let d = OpusMultistreamDecoder::new(48_000, n, head.streams, head.coupled, &head.mapping).map_err(err)?;
                (OpusCore::Multi(d), Some(perm), Channels::Positioned(pos))
            }
            _ => return Err("unsupported Opus channel mapping".into()),
        };
        Ok(OpusDec { core, channels: head.channels, gain, perm, layout, pcm: vec![0.0; OPUS_MAX_FRAMES * head.channels] })
    }

    fn reset(&mut self) {
        match &mut self.core {
            OpusCore::Single(d) => d.reset(),
            OpusCore::Multi(d) => d.reset(),
        }
    }

    fn decode(&mut self, packet: &Packet, buf: &mut Vec<f32>) -> Result<Info, SymError> {
        let bad = |_| SymError::DecodeError("opus: corrupt packet");
        let n = match &mut self.core {
            OpusCore::Single(d) => d.decode_float(&packet.data, &mut self.pcm, false).map_err(bad)?,
            OpusCore::Multi(d) => d.decode_float(&packet.data, &mut self.pcm, false).map_err(bad)?,
        };
        // Pre-skip (front) and end trimming come from the Ogg demuxer as packet trims.
        let end = n.saturating_sub(packet.trim_end.get() as usize);
        let start = (packet.trim_start.get() as usize).min(end);
        let ch = self.channels;
        buf.clear();
        match self.perm {
            None => buf.extend_from_slice(&self.pcm[start * ch..end * ch]),
            Some(perm) => {
                for frame in self.pcm[start * ch..end * ch].chunks_exact(ch) {
                    buf.extend(perm.iter().map(|&i| frame[i]));
                }
            }
        }
        if (self.gain - 1.0).abs() > 1e-6 {
            for s in buf.iter_mut() {
                *s *= self.gain;
            }
        }
        Ok(Info { frames: end - start, rate: 48_000, channels: self.layout.clone() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_head_parsing() {
        let mut h = b"OpusHead".to_vec();
        h.extend([1, 2, 0x38, 0x01, 0x80, 0xBB, 0, 0, 0x00, 0x01, 0]);
        let head = parse_opus_head(&h).unwrap();
        assert_eq!(head.channels, 2);
        assert_eq!(head.gain_q8, 256, "+1 dB in Q7.8");
        assert_eq!(head.family, 0);
        assert!(parse_opus_head(b"OpusTags....................").is_none());
        assert!(parse_opus_head(b"OpusHead").is_none());
    }

    #[test]
    fn vorbis_surround_reordering_tables_are_permutations() {
        for ch in 3..=8 {
            let (pos, perm) = vorbis_layout(ch).unwrap();
            assert_eq!(pos.bits().count_ones() as usize, ch);
            let mut seen = perm.to_vec();
            seen.sort_unstable();
            assert_eq!(seen, (0..ch).collect::<Vec<_>>());
        }
        assert!(vorbis_layout(2).is_none() && vorbis_layout(9).is_none());
    }

    #[test]
    fn audio_nothing_here_decodes_is_recognized_up_front() {
        use symphonia::core::codecs::audio::well_known::*;
        let params = |codec, extra: &[u8]| {
            let mut p = AudioCodecParameters::new();
            p.for_codec(codec).with_sample_rate(44_100);
            if !extra.is_empty() {
                p.with_extra_data(extra.into());
            }
            p
        };
        // AAC-LC stereo and 5.1 (AudioSpecificConfig 1210 and 1230), the way an MP4 declares them.
        assert!(can_decode(&params(CODEC_ID_AAC, &[0x12, 0x10])));
        assert!(!can_decode(&params(CODEC_ID_AAC, &[0x12, 0x30])), "5.1 AAC is not supported by the decoder");
        // The usual soundtracks of films and some videos.
        for no in [CODEC_ID_AC3, CODEC_ID_EAC3, CODEC_ID_DCA] {
            assert!(!can_decode(&params(no, &[])), "{no:?}");
        }
        // Opus goes to its own decoder, which needs a valid header.
        let mut head = b"OpusHead".to_vec();
        head.extend([1, 2, 0x38, 0x01, 0x80, 0xBB, 0, 0, 0x00, 0x01, 0]);
        assert!(can_decode(&params(CODEC_ID_OPUS, &head)));
        assert!(!can_decode(&params(CODEC_ID_OPUS, b"not an Opus header at all")));
    }

    // The fixtures: one second of a half-scale 262 Hz sine, in every container a video or a download
    // can arrive in. The MP4 was encoded by Windows' own AAC and H.264 encoders. The Matroska and
    // WebM files carry real Opus and Vorbis (from libsndfile) or that same AAC and H.264, muxed by
    // a small script. Where there is a picture, it is a solid colour and its track comes first.
    const MP4_AAC: &[u8] = include_bytes!("../testdata/sine-262hz-with-video.mp4");
    const WEBM_OPUS: &[u8] = include_bytes!("../testdata/sine-262hz-opus.webm");
    const WEBM_OPUS_NO_INDEX: &[u8] = include_bytes!("../testdata/sine-262hz-opus-recorder.webm");
    const MKV_VORBIS: &[u8] = include_bytes!("../testdata/sine-262hz-vorbis-video.mkv");
    const MKV_AAC: &[u8] = include_bytes!("../testdata/sine-262hz-aac-video.mkv");

    /// (file name, contents, sample rate, whether the picture track comes first)
    const SINE_FILES: [(&str, &[u8], u32, bool); 4] = [
        ("clip.mp4", MP4_AAC, 44_100, true),
        ("clip.webm", WEBM_OPUS, 48_000, false),
        ("vorbis.mkv", MKV_VORBIS, 44_100, true),
        ("aac.mkv", MKV_AAC, 44_100, true),
    ];

    /// A fixture written to a folder of its own (tests run in parallel).
    fn fixture(tag: &str, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wisp-sine-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn forget(path: &std::path::Path) {
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Goertzel: the power of one frequency in a mono signal, scaled so that a sine of amplitude
    /// `a` at that frequency gives about `a * a / 4`.
    fn tone_power(mono: &[f32], rate: f32, freq: f32) -> f32 {
        let c = 2.0 * (2.0 * std::f32::consts::PI * freq / rate).cos();
        let (mut s1, mut s2) = (0.0f32, 0.0f32);
        for &x in mono {
            let s = x + c * s1 - s2;
            s2 = s1;
            s1 = s;
        }
        (s1 * s1 + s2 * s2 - c * s1 * s2) / (mono.len() as f32).powi(2)
    }

    /// Everything the file has left to play, interleaved, and its channel count.
    fn decode_rest(o: &mut Opened) -> (Vec<f32>, usize) {
        let (mut pcm, mut buf) = (Vec::new(), Vec::new());
        while let Step::Frames(_) = o.next(&mut buf).unwrap() {
            pcm.extend_from_slice(&buf);
        }
        (pcm, o.roles().len().max(1))
    }

    /// Plays at least `seconds` of the file and drops it.
    fn play_for(o: &mut Opened, seconds: f32) {
        let (mut buf, mut frames) = (Vec::new(), 0usize);
        while (frames as f32) < seconds * o.declared_rate() as f32 {
            match o.next(&mut buf).unwrap() {
                Step::Frames(n) => frames += n,
                Step::Eof => panic!("the file ended after {frames} frames"),
            }
        }
    }

    /// Plays the file to its end and checks that it is the sine the fixtures carry: about a second
    /// long, at the expected rate, with that tone in it and not another.
    fn check_the_sine(path: &std::path::Path, rate: u32, duration_declared: bool) {
        let mut o = open(path).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(o.declared_rate(), rate);
        if duration_declared {
            let total = o.total_frames.expect("the duration is known") as f32 / rate as f32;
            assert!((0.9..1.15).contains(&total), "{total} s declared");
        }
        let (pcm, ch) = decode_rest(&mut o);
        assert_eq!(ch, 2);
        let rate = o.rate() as f32;
        let mono: Vec<f32> = pcm.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
        let seconds = mono.len() as f32 / rate;
        assert!((0.9..1.2).contains(&seconds), "{seconds} s of audio");
        let rms = (mono.iter().map(|x| x * x).sum::<f32>() / mono.len() as f32).sqrt();
        assert!((0.25..0.45).contains(&rms), "a half-scale sine has an RMS of 0.35, got {rms}");
        let (right, wrong) = (tone_power(&mono, rate, 262.0), tone_power(&mono, rate, 330.0));
        assert!(right > 100.0 * wrong, "262 Hz is in the sound and 330 Hz is not: {right} against {wrong}");
    }

    /// Seeks forward on a fresh file, and back on one that has been playing: each time about the
    /// rest of the second must be left.
    fn check_seeking(path: &std::path::Path) {
        let left_after = |o: &mut Opened, skip: u64| {
            let (pcm, ch) = decode_rest(o);
            ((pcm.len() / ch).saturating_sub(skip as usize)) as f32 / o.rate() as f32
        };
        let mut o = open(path).unwrap();
        let skip = o.seek(500).expect("a fresh file seeks forward");
        let left = left_after(&mut o, skip);
        assert!((0.35..0.65).contains(&left), "about half a second is left after seeking to 0.5 s, got {left}");

        let mut o = open(path).unwrap();
        play_for(&mut o, 0.8);
        let skip = o.seek(300).expect("a file that has been playing seeks back");
        let left = left_after(&mut o, skip);
        assert!((0.55..0.9).contains(&left), "about 0.7 s is left after seeking back to 0.3 s, got {left}");
    }

    #[test]
    fn every_container_plays_its_audio_and_only_its_audio() {
        for (name, bytes, rate, picture_first) in SINE_FILES {
            let path = fixture("play", name, bytes);
            let fmt = open_format(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            let first = fmt.tracks().first().and_then(|t| t.codec_params.as_ref());
            assert_eq!(matches!(first, Some(CodecParameters::Video(_))), picture_first, "{name}: which track comes first");
            drop(fmt);
            check_the_sine(&path, rate, true);
            forget(&path);
        }
    }

    #[test]
    fn seeking_works_in_every_container() {
        for (name, bytes, _, _) in SINE_FILES {
            let path = fixture("seek", name, bytes);
            check_seeking(&path);
            forget(&path);
        }
    }

    /// A browser's recording (MediaRecorder) has no duration and no index: it still plays, and it
    /// can be seeked back by starting over.
    #[test]
    fn a_webm_without_an_index_plays_and_seeks() {
        let path = fixture("recorder", "recording.webm", WEBM_OPUS_NO_INDEX);
        check_the_sine(&path, 48_000, false);
        assert!(open(&path).unwrap().total_frames.is_none(), "the file does not say how long it is");
        check_seeking(&path);
        forget(&path);
    }
}
