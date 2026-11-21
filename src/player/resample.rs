//! Streaming sample-rate conversion with *exact* output length, so gapless playback stays
//! sample-accurate when the track rate differs from the device rate.
//!
//! Wraps rubato's FFT resampler (high quality, fixed ratio, low CPU). It removes the filter's
//! start-up delay from the front and flushes its tail at the end, so a track of N input frames
//! produces exactly `ceil(N * ratio)` output frames.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler};

const CHUNK: usize = 1024;

pub struct StreamResampler {
    rs: Fft<f32>,
    ch: usize,
    /// Source and destination rates. Lengths are computed with integers: a float ratio such as
    /// 48000/44100 is inexact and `ceil` would add a frame at every track end.
    src: u64,
    dst: u64,
    /// Interleaved input that has not been consumed yet.
    pending: Vec<f32>,
    /// Output scratch, `output_frames_max * ch`.
    scratch: Vec<f32>,
    /// Output frames still to be dropped (filter delay).
    skip: usize,
    in_frames: u64,
    out_frames: u64,
}

impl StreamResampler {
    pub fn new(src_rate: u32, dst_rate: u32, ch: usize) -> Result<Self, String> {
        let rs = Fft::<f32>::new(src_rate as usize, dst_rate as usize, CHUNK, ch, FixedSync::Input)
            .map_err(|e| format!("resampler: {e}"))?;
        let scratch = vec![0.0; rs.output_frames_max() * ch];
        Ok(StreamResampler {
            skip: rs.output_delay(),
            src: src_rate as u64,
            dst: dst_rate as u64,
            rs,
            ch,
            pending: Vec::with_capacity(CHUNK * ch * 2),
            scratch,
            in_frames: 0,
            out_frames: 0,
        })
    }

    /// Output frames the stream will have produced once everything pushed so far is flushed:
    /// `ceil(input frames * ratio)`. This is where a track ends in the output stream.
    pub fn expected_out(&self) -> u64 {
        ((self.in_frames as u128 * self.dst as u128).div_ceil(self.src as u128)) as u64
    }

    /// Feeds interleaved input of any length; appends whatever output is ready to `out`.
    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<(), String> {
        self.in_frames += (input.len() / self.ch) as u64;
        self.pending.extend_from_slice(input);
        loop {
            let need = self.rs.input_frames_next();
            if self.pending.len() < need * self.ch {
                return Ok(());
            }
            let produced = self.step(None)?;
            self.emit(produced, None, out);
        }
    }

    /// Flushes the tail so the total output is exactly `ceil(input frames * ratio)`.
    pub fn finish(&mut self, out: &mut Vec<f32>) -> Result<(), String> {
        let expected = self.expected_out();
        let rest = self.pending.len() / self.ch;
        if rest > 0 {
            let produced = self.step(Some(rest))?;
            self.emit(produced, Some(expected), out);
        }
        // Keep feeding silence until the delayed tail has come out.
        for _ in 0..64 {
            if self.out_frames >= expected {
                break;
            }
            let produced = self.step(Some(0))?;
            self.emit(produced, Some(expected), out);
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn reset(&mut self) {
        self.rs.reset();
        self.pending.clear();
        self.skip = self.rs.output_delay();
        self.in_frames = 0;
        self.out_frames = 0;
    }

    /// Runs one resampler call. `partial` = number of valid input frames when flushing.
    fn step(&mut self, partial: Option<usize>) -> Result<usize, String> {
        let ch = self.ch;
        let frames_in = partial.unwrap_or_else(|| self.rs.input_frames_next());
        let max_out = self.rs.output_frames_max();
        let produced = {
            let input = InterleavedSlice::new(&self.pending[..frames_in * ch], ch, frames_in)
                .map_err(|e| format!("resampler input: {e}"))?;
            let mut output = InterleavedSlice::new_mut(&mut self.scratch[..max_out * ch], ch, max_out)
                .map_err(|e| format!("resampler output: {e}"))?;
            let indexing = partial.map(|n| Indexing::new().partial_len(n));
            let (_, produced) = self
                .rs
                .process_into_buffer(&input, &mut output, indexing.as_ref())
                .map_err(|e| format!("resampler: {e}"))?;
            produced
        };
        self.pending.drain(..frames_in * ch);
        Ok(produced)
    }

    fn emit(&mut self, produced: usize, cap: Option<u64>, out: &mut Vec<f32>) {
        let ch = self.ch;
        let drop_frames = self.skip.min(produced);
        self.skip -= drop_frames;
        let mut n = produced - drop_frames;
        if let Some(cap) = cap {
            n = n.min(cap.saturating_sub(self.out_frames) as usize);
        }
        out.extend_from_slice(&self.scratch[drop_frames * ch..(drop_frames + n) * ch]);
        self.out_frames += n as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f32, frames: usize, ch: usize) -> Vec<f32> {
        let mut v = Vec::with_capacity(frames * ch);
        for n in 0..frames {
            let s = (2.0 * std::f32::consts::PI * hz * n as f32 / rate as f32).sin() * 0.5;
            for _ in 0..ch {
                v.push(s);
            }
        }
        v
    }

    fn run(src: u32, dst: u32, frames: usize, piece: usize) -> Vec<f32> {
        let input = sine(src, 1000.0, frames, 2);
        let mut rs = StreamResampler::new(src, dst, 2).unwrap();
        let mut out = Vec::new();
        for part in input.chunks(piece * 2) {
            rs.push(part, &mut out).unwrap();
        }
        rs.finish(&mut out).unwrap();
        out
    }

    #[test]
    fn output_length_is_exact_for_common_rate_pairs() {
        for (src, dst) in [(44_100, 48_000), (48_000, 44_100), (96_000, 48_000), (22_050, 48_000), (32_000, 48_000)] {
            for frames in [1, 100, 1023, 1024, 1025, 44_100, 100_003] {
                let out = run(src, dst, frames, 997);
                let expected = (frames as f64 * dst as f64 / src as f64).ceil() as usize;
                assert_eq!(out.len() / 2, expected, "{src}->{dst} with {frames} frames");
            }
        }
    }

    #[test]
    fn signal_is_aligned_and_clean() {
        let out = run(44_100, 48_000, 44_100, 4096);
        // Compare the middle of the stream with an ideal 1 kHz sine at the new rate.
        let mut worst = 0.0f32;
        for n in 4_000..44_000usize {
            let want = (2.0 * std::f32::consts::PI * 1000.0 * n as f32 / 48_000.0).sin() * 0.5;
            worst = worst.max((out[n * 2] - want).abs());
        }
        assert!(worst < 0.01, "worst deviation {worst}: delay compensation or filter is off");
    }

    #[test]
    fn packet_size_does_not_change_the_result() {
        let a = run(44_100, 48_000, 30_000, 1);
        let b = run(44_100, 48_000, 30_000, 4096);
        let c = run(44_100, 48_000, 30_000, 777);
        assert_eq!(a.len(), b.len());
        assert_eq!(a.len(), c.len());
        for i in 0..a.len() {
            assert!((a[i] - b[i]).abs() < 1e-4 && (a[i] - c[i]).abs() < 1e-4, "sample {i}");
        }
    }

    #[test]
    fn reset_restarts_cleanly() {
        let input = sine(44_100, 1000.0, 10_000, 2);
        let mut rs = StreamResampler::new(44_100, 48_000, 2).unwrap();
        let mut first = Vec::new();
        rs.push(&input, &mut first).unwrap();
        rs.finish(&mut first).unwrap();
        rs.reset();
        let mut second = Vec::new();
        rs.push(&input, &mut second).unwrap();
        rs.finish(&mut second).unwrap();
        assert_eq!(first.len(), second.len());
        for i in 0..first.len() {
            assert!((first[i] - second[i]).abs() < 1e-5);
        }
    }
}
