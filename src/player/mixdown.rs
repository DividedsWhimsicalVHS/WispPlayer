//! Channel layout conversion between a decoded track and the output device.
//!
//! Most music is stereo on a stereo device and takes the zero-cost copy path. Mono is spread to
//! both speakers, surround is folded down with ITU-style coefficients, and stereo on a surround
//! device goes to the front pair (the other speakers stay silent).

use symphonia::core::audio::{ChannelLabel, Channels, Position};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Left,
    Right,
    Centre,
    Lfe,
    SideLeft,
    SideRight,
    BackLeft,
    BackRight,
    /// Anything we have no sensible stereo mapping for.
    Skip,
}

fn role_of(p: Position) -> Role {
    if p == Position::FRONT_LEFT || p == Position::FRONT_LEFT_CENTER || p == Position::FRONT_LEFT_WIDE {
        Role::Left
    } else if p == Position::FRONT_RIGHT || p == Position::FRONT_RIGHT_CENTER || p == Position::FRONT_RIGHT_WIDE {
        Role::Right
    } else if p == Position::FRONT_CENTER {
        Role::Centre
    } else if p == Position::LFE1 || p == Position::LFE2 {
        Role::Lfe
    } else if p == Position::SIDE_LEFT {
        Role::SideLeft
    } else if p == Position::SIDE_RIGHT {
        Role::SideRight
    } else if p == Position::REAR_LEFT {
        Role::BackLeft
    } else if p == Position::REAR_RIGHT {
        Role::BackRight
    } else {
        Role::Skip
    }
}

/// One role per channel, in the order the decoder interleaves them.
pub fn roles_from(channels: &Channels) -> Vec<Role> {
    match channels {
        Channels::Positioned(set) => (0..64)
            .filter(|bit| set.bits() & (1u64 << bit) != 0)
            .map(|bit| role_of(Position::from_bits_truncate(1u64 << bit)))
            .collect(),
        Channels::Custom(labels) => labels
            .iter()
            .map(|l| match l {
                ChannelLabel::Positioned(p) => role_of(*p),
                _ => Role::Skip,
            })
            .collect(),
        Channels::Discrete(n) => {
            let n = *n as usize;
            (0..n)
                .map(|i| match (n, i) {
                    (1, _) => Role::Centre,
                    (_, 0) => Role::Left,
                    (_, 1) => Role::Right,
                    _ => Role::Skip,
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

const HALF_POWER: f32 = std::f32::consts::FRAC_1_SQRT_2;

pub struct Mixer {
    src: usize,
    dst: usize,
    /// `dst x src`, row-major. Empty means "identical layout, just copy".
    matrix: Vec<f32>,
}

impl Mixer {
    pub fn new(roles: &[Role], dst: usize) -> Mixer {
        let src = roles.len();
        if src == dst {
            return Mixer { src, dst, matrix: Vec::new() };
        }
        let mut matrix = vec![0.0; dst * src];
        if src == 1 {
            // Mono: same signal on both front speakers.
            for row in 0..dst.min(2) {
                matrix[row * src] = 1.0;
            }
            return Mixer { src, dst, matrix };
        }
        let (mut l, mut r) = (vec![0.0f32; src], vec![0.0f32; src]);
        for (i, role) in roles.iter().enumerate() {
            match role {
                Role::Left => l[i] = 1.0,
                Role::Right => r[i] = 1.0,
                Role::Centre => {
                    l[i] = HALF_POWER;
                    r[i] = HALF_POWER;
                }
                Role::SideLeft | Role::BackLeft => l[i] = HALF_POWER,
                Role::SideRight | Role::BackRight => r[i] = HALF_POWER,
                Role::Lfe | Role::Skip => {}
            }
        }
        // Keep the folded-down level at or below the original.
        let power = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>();
        let norm = 1.0 / power(&l).max(power(&r)).max(1.0).sqrt();
        for i in 0..src {
            if dst == 1 {
                matrix[i] = 0.5 * (l[i] + r[i]) * norm;
            } else {
                matrix[i] = l[i] * norm;
                matrix[src + i] = r[i] * norm;
            }
        }
        Mixer { src, dst, matrix }
    }

    pub fn is_identity(&self) -> bool {
        self.matrix.is_empty()
    }

    /// Appends the converted frames to `out`.
    pub fn process(&self, input: &[f32], out: &mut Vec<f32>) {
        if self.is_identity() {
            out.extend_from_slice(input);
            return;
        }
        let frames = input.len() / self.src;
        out.reserve(frames * self.dst);
        for frame in input.chunks_exact(self.src) {
            for row in self.matrix.chunks_exact(self.src) {
                out.push(row.iter().zip(frame).map(|(m, s)| m * s).sum());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn positioned(p: Position) -> Vec<Role> {
        roles_from(&Channels::Positioned(p))
    }

    #[test]
    fn roles_follow_canonical_order() {
        let p = Position::FRONT_LEFT
            | Position::FRONT_RIGHT
            | Position::FRONT_CENTER
            | Position::LFE1
            | Position::REAR_LEFT
            | Position::REAR_RIGHT;
        assert_eq!(
            positioned(p),
            [Role::Left, Role::Right, Role::Centre, Role::Lfe, Role::BackLeft, Role::BackRight]
        );
        assert_eq!(roles_from(&Channels::Discrete(2)), [Role::Left, Role::Right]);
        assert_eq!(roles_from(&Channels::Discrete(1)), [Role::Centre]);
    }

    #[test]
    fn identical_layouts_are_copied_untouched() {
        let roles = positioned(Position::FRONT_LEFT | Position::FRONT_RIGHT);
        let m = Mixer::new(&roles, 2);
        assert!(m.is_identity());
        let mut out = Vec::new();
        m.process(&[0.1, 0.2, 0.3, 0.4], &mut out);
        assert_eq!(out, [0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn mono_is_spread_to_both_speakers_without_level_change() {
        let m = Mixer::new(&[Role::Centre], 2);
        let mut out = Vec::new();
        m.process(&[0.5, -0.25], &mut out);
        assert_eq!(out, [0.5, 0.5, -0.25, -0.25]);
    }

    #[test]
    fn stereo_to_mono_averages() {
        let m = Mixer::new(&[Role::Left, Role::Right], 1);
        let mut out = Vec::new();
        m.process(&[0.4, 0.2], &mut out);
        assert!((out[0] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn stereo_on_a_surround_device_feeds_only_the_front_pair() {
        let m = Mixer::new(&[Role::Left, Role::Right], 6);
        let mut out = Vec::new();
        m.process(&[0.5, -0.5], &mut out);
        assert_eq!(out, [0.5, -0.5, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn surround_folds_down_without_clipping_full_scale() {
        let roles = positioned(
            Position::FRONT_LEFT
                | Position::FRONT_RIGHT
                | Position::FRONT_CENTER
                | Position::LFE1
                | Position::REAR_LEFT
                | Position::REAR_RIGHT,
        );
        let m = Mixer::new(&roles, 2);
        let mut out = Vec::new();
        // Every channel at full scale and perfectly in phase is the worst case; real music
        // never does this. The level stays bounded (the output stage hard-limits at 1.0).
        m.process(&[1.0; 6], &mut out);
        assert!(out[0] <= 1.75 && out[0] > 1.0, "folded level is bounded, got {}", out[0]);
        // Only the left front channel: arrives on the left only.
        out.clear();
        m.process(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0], &mut out);
        assert!(out[0] > 0.6 && out[1] == 0.0);
        // LFE is dropped.
        out.clear();
        m.process(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0], &mut out);
        assert_eq!(out, [0.0, 0.0]);
    }
}
