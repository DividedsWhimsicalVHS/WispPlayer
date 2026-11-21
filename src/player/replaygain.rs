//! ReplayGain / EBU R128 tag parsing and conversion into playback gains.

use super::pipe::{Gain, db_to_lin};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReplayGain {
    pub track_gain_db: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain_db: Option<f32>,
    pub album_peak: Option<f32>,
}

/// "-6.54 dB", "+3.2dB", "0.00" -> dB.
pub fn parse_gain_db(s: &str) -> Option<f32> {
    let t = s.trim().trim_end_matches(|c: char| c.is_alphabetic()).trim();
    let t = t.replace('\u{2212}', "-").replace(',', ".");
    let v: f32 = t.trim().parse().ok()?;
    v.is_finite().then_some(v)
}

/// A sample peak such as "0.988121" (1.0 = full scale).
pub fn parse_peak(s: &str) -> Option<f32> {
    let v: f32 = s.trim().replace(',', ".").parse().ok()?;
    (v.is_finite() && v > 0.0).then_some(v)
}

/// Opus `R128_*_GAIN` tags: a Q7.8 fixed-point number relative to -23 LUFS. ReplayGain 2.0 is
/// defined against -18 LUFS, so 5 dB are added to get the equivalent ReplayGain value.
pub fn parse_r128_db(s: &str) -> Option<f32> {
    let q: i32 = s.trim().parse().ok()?;
    Some(q as f32 / 256.0 + 5.0)
}

impl ReplayGain {
    /// Feeds one tag. `key` may be a plain Vorbis key, an ID3 `TXXX:` description or an MP4
    /// freeform name; matching is case-insensitive on the trailing field name.
    pub fn apply_tag(&mut self, key: &str, value: &str) {
        let k = key.to_ascii_uppercase();
        let name = k.rsplit([':', '/']).next().unwrap_or(&k);
        match name {
            "REPLAYGAIN_TRACK_GAIN" => self.track_gain_db = parse_gain_db(value).or(self.track_gain_db),
            "REPLAYGAIN_TRACK_PEAK" => self.track_peak = parse_peak(value).or(self.track_peak),
            "REPLAYGAIN_ALBUM_GAIN" => self.album_gain_db = parse_gain_db(value).or(self.album_gain_db),
            "REPLAYGAIN_ALBUM_PEAK" => self.album_peak = parse_peak(value).or(self.album_peak),
            // Real ReplayGain tags win over R128 ones when both are present.
            "R128_TRACK_GAIN" if self.track_gain_db.is_none() => {
                self.track_gain_db = parse_r128_db(value)
            }
            "R128_ALBUM_GAIN" if self.album_gain_db.is_none() => {
                self.album_gain_db = parse_r128_db(value)
            }
            _ => {}
        }
    }

    /// `(track, album)` gains. Album falls back to track and vice versa; untagged audio plays at
    /// unity gain.
    pub fn gains(&self) -> (Gain, Gain) {
        let make = |db: Option<f32>, peak: Option<f32>| {
            db.map(|db| Gain { lin: db_to_lin(db), max: peak.map_or(f32::INFINITY, |p| 1.0 / p) })
        };
        let track = make(self.track_gain_db, self.track_peak);
        let album = make(self.album_gain_db, self.album_peak);
        (track.or(album).unwrap_or(Gain::UNITY), album.or(track).unwrap_or(Gain::UNITY))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_gain_spellings() {
        assert_eq!(parse_gain_db("-6.54 dB"), Some(-6.54));
        assert_eq!(parse_gain_db("+3.2dB"), Some(3.2));
        assert_eq!(parse_gain_db("0.00"), Some(0.0));
        assert_eq!(parse_gain_db(" -4,5 dB "), Some(-4.5));
        assert_eq!(parse_gain_db("\u{2212}2.0 dB"), Some(-2.0));
        assert_eq!(parse_gain_db("loud"), None);
        assert_eq!(parse_gain_db("NaN dB"), None);
    }

    #[test]
    fn parses_peaks() {
        assert_eq!(parse_peak("0.988121"), Some(0.988121));
        assert_eq!(parse_peak("0"), None);
        assert_eq!(parse_peak("-1"), None);
    }

    #[test]
    fn r128_is_converted_to_replaygain_reference() {
        // -23 LUFS -> -18 LUFS reference: +5 dB. A tag of -256 (-1 dB) becomes +4 dB.
        assert_eq!(parse_r128_db("-256"), Some(4.0));
        assert_eq!(parse_r128_db("0"), Some(5.0));
    }

    #[test]
    fn tag_keys_from_every_container_match() {
        let mut rg = ReplayGain::default();
        rg.apply_tag("REPLAYGAIN_TRACK_GAIN", "-7.0 dB");
        rg.apply_tag("TXXX:replaygain_track_peak", "0.5");
        rg.apply_tag("----:com.apple.iTunes:replaygain_album_gain", "-5.0 dB");
        rg.apply_tag("replaygain_album_peak", "0.8");
        assert_eq!(rg.track_gain_db, Some(-7.0));
        assert_eq!(rg.track_peak, Some(0.5));
        assert_eq!(rg.album_gain_db, Some(-5.0));
        assert_eq!(rg.album_peak, Some(0.8));
    }

    #[test]
    fn replaygain_tags_beat_r128() {
        let mut rg = ReplayGain::default();
        rg.apply_tag("R128_TRACK_GAIN", "-512");
        rg.apply_tag("REPLAYGAIN_TRACK_GAIN", "-3.0 dB");
        assert_eq!(rg.track_gain_db, Some(-3.0));
        let mut rg = ReplayGain::default();
        rg.apply_tag("REPLAYGAIN_TRACK_GAIN", "-3.0 dB");
        rg.apply_tag("R128_TRACK_GAIN", "-512");
        assert_eq!(rg.track_gain_db, Some(-3.0));
    }

    #[test]
    fn gains_fall_back_between_track_and_album_and_to_unity() {
        let none = ReplayGain::default();
        assert_eq!(none.gains(), (Gain::UNITY, Gain::UNITY));

        let only_track = ReplayGain { track_gain_db: Some(-6.0206), track_peak: Some(0.5), ..Default::default() };
        let (t, a) = only_track.gains();
        assert!((t.lin - 0.5).abs() < 1e-3);
        assert!((t.max - 2.0).abs() < 1e-6, "peak 0.5 allows a gain of 2");
        assert_eq!(t, a, "album falls back to track");

        let only_album = ReplayGain { album_gain_db: Some(6.0206), ..Default::default() };
        let (t, a) = only_album.gains();
        assert!((a.lin - 2.0).abs() < 1e-3);
        assert_eq!(t, a, "track falls back to album");
        assert!(a.max.is_infinite(), "no peak, no limit");
    }
}
