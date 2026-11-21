//! Album artwork, loaded lazily and kept tiny.
//!
//! A cover is only decoded when something is about to show it, and never at full size: JPEGs are
//! decoded at 1/2, 1/4 or 1/8 scale straight from the DCT coefficients, and PNGs stream through a
//! box-filter downscaler row by row. A 4000x4000 cover therefore costs a few hundred KB of
//! transient memory, and the result is at most `max`x`max` RGB (no alpha channel, 3 bytes/pixel).
//! The ambient backdrop is a 40x40 blurred copy that the renderer stretches (bilinear) to the
//! window.

use std::io::Cursor;
use std::path::Path;

use symphonia::core::meta::{StandardTag, StandardVisualKey};

/// 8-bit RGB pixels, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Rgb {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

/// What the audio file's own tags carry that the now-playing screen wants.
#[derive(Default)]
pub struct Embedded {
    pub cover: Option<Vec<u8>>,
    pub lyrics: Option<String>,
}

pub fn read_embedded(path: &Path) -> Embedded {
    let mut out = Embedded::default();
    let Ok(mut fmt) = crate::player::open_format(path) else { return out };
    let mut best_is_front = false;
    let mut md = fmt.metadata();
    loop {
        if let Some(rev) = md.current() {
            let visuals = rev.media.visuals.iter().chain(rev.per_track.iter().flat_map(|t| t.metadata.visuals.iter()));
            for v in visuals {
                let front = v.usage == Some(StandardVisualKey::FrontCover);
                if out.cover.is_none() || (front && !best_is_front) {
                    out.cover = Some(v.data.to_vec());
                    best_is_front = front;
                }
            }
            let tags = rev.media.tags.iter().chain(rev.per_track.iter().flat_map(|t| t.metadata.tags.iter()));
            for tag in tags {
                if let Some(StandardTag::Lyrics(s)) = &tag.std {
                    if out.lyrics.is_none() && !s.trim().is_empty() {
                        out.lyrics = Some(s.to_string());
                    }
                }
            }
        }
        if md.pop().is_none() {
            break;
        }
    }
    out
}

/// `cover.jpg`, `folder.png`, ... next to the audio file.
pub fn folder_cover(dir: &Path) -> Option<Vec<u8>> {
    const NAMES: [&str; 6] = ["cover", "folder", "front", "album", "albumart", "artwork"];
    let mut best: Option<(usize, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
        if !matches!(ext.as_deref(), Some("jpg" | "jpeg" | "png")) {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).map(str::to_ascii_lowercase)?;
        if let Some(rank) = NAMES.iter().position(|n| *n == stem) {
            if best.as_ref().is_none_or(|(r, _)| rank < *r) {
                best = Some((rank, path));
            }
        }
    }
    let (_, path) = best?;
    let md = std::fs::metadata(&path).ok()?;
    (md.len() < 32 << 20).then(|| std::fs::read(path).ok()).flatten()
}

// ---------------------------------------------------------------------------------------------
// Decoding + downscaling
// ---------------------------------------------------------------------------------------------

fn fit_size(w: u32, h: u32, max: u32) -> (u32, u32) {
    let big = w.max(h);
    if big <= max {
        return (w.max(1), h.max(1));
    }
    ((w as u64 * max as u64 / big as u64).max(1) as u32, (h as u64 * max as u64 / big as u64).max(1) as u32)
}

#[derive(Clone, Copy)]
enum Kind {
    Gray,
    GrayAlpha,
    Rgb,
    Rgba,
    Gray16,
    Cmyk,
}

/// Streaming box-filter downscaler: rows go in, a small image comes out. Memory is the output
/// image plus one accumulator row, whatever the source size.
struct Downscaler {
    sw: usize,
    sh: usize,
    tw: usize,
    th: usize,
    acc: Vec<u32>,
    cols: Vec<u32>,
    rows: u32,
    dy: usize,
    y: usize,
    out: Vec<u8>,
}

impl Downscaler {
    fn new(sw: u32, sh: u32, tw: u32, th: u32) -> Downscaler {
        let (sw, sh, tw, th) = (sw as usize, sh as usize, tw as usize, th as usize);
        let mut cols = vec![0u32; tw];
        for x in 0..sw {
            cols[x * tw / sw] += 1;
        }
        Downscaler { sw, sh, tw, th, acc: vec![0; tw * 3], cols, rows: 0, dy: 0, y: 0, out: vec![0; tw * th * 3] }
    }

    fn finish_row(&mut self) {
        if self.rows > 0 && self.dy < self.th {
            for x in 0..self.tw {
                let n = (self.rows * self.cols[x].max(1)).max(1);
                for c in 0..3 {
                    self.out[(self.dy * self.tw + x) * 3 + c] = (self.acc[x * 3 + c] / n) as u8;
                }
            }
        }
        self.acc.fill(0);
        self.rows = 0;
    }

    fn push_row(&mut self, row: &[u8], kind: Kind) {
        if self.y >= self.sh {
            return;
        }
        let dy = self.y * self.th / self.sh;
        if dy != self.dy {
            self.finish_row();
            self.dy = dy;
        }
        let bytes = match kind {
            Kind::Gray => 1,
            Kind::GrayAlpha | Kind::Gray16 => 2,
            Kind::Rgb => 3,
            Kind::Rgba | Kind::Cmyk => 4,
        };
        for (x, p) in row.chunks_exact(bytes).take(self.sw).enumerate() {
            let rgb = match kind {
                Kind::Gray | Kind::Gray16 => [p[0], p[0], p[0]],
                Kind::GrayAlpha => {
                    let v = (p[0] as u32 * p[1] as u32 / 255) as u8;
                    [v, v, v]
                }
                Kind::Rgb => [p[0], p[1], p[2]],
                Kind::Rgba => {
                    let a = p[3] as u32;
                    [(p[0] as u32 * a / 255) as u8, (p[1] as u32 * a / 255) as u8, (p[2] as u32 * a / 255) as u8]
                }
                Kind::Cmyk => {
                    // Adobe-style inverted CMYK as written by most JPEG encoders.
                    let k = p[3] as u32;
                    [(p[0] as u32 * k / 255) as u8, (p[1] as u32 * k / 255) as u8, (p[2] as u32 * k / 255) as u8]
                }
            };
            let dx = x * self.tw / self.sw;
            for c in 0..3 {
                self.acc[dx * 3 + c] += rgb[c] as u32;
            }
        }
        self.rows += 1;
        self.y += 1;
    }

    fn finish(mut self) -> Rgb {
        self.finish_row();
        Rgb { w: self.tw as u32, h: self.th as u32, px: self.out }
    }
}

fn decode_jpeg(bytes: &[u8], max: u32) -> Option<Rgb> {
    let mut d = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    d.read_info().ok()?;
    let full = d.info()?;
    let (tw, th) = fit_size(full.width as u32, full.height as u32, max);
    // Decode at the smallest power-of-two scale that is still >= the target.
    d.scale(tw.min(u16::MAX as u32) as u16, th.min(u16::MAX as u32) as u16).ok()?;
    let px = d.decode().ok()?;
    let info = d.info()?;
    let (w, h) = (info.width as u32, info.height as u32);
    let (kind, bytes_per_px) = match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => (Kind::Gray, 1),
        jpeg_decoder::PixelFormat::L16 => (Kind::Gray16, 2),
        jpeg_decoder::PixelFormat::RGB24 => (Kind::Rgb, 3),
        jpeg_decoder::PixelFormat::CMYK32 => (Kind::Cmyk, 4),
    };
    let (ow, oh) = fit_size(w, h, max);
    let mut ds = Downscaler::new(w, h, ow, oh);
    for row in px.chunks_exact(w as usize * bytes_per_px) {
        ds.push_row(row, kind);
    }
    Some(ds.finish())
}

fn decode_png(bytes: &[u8], max: u32) -> Option<Rgb> {
    let mut dec = png::Decoder::new(Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let (w, h) = (reader.info().width, reader.info().height);
    if w == 0 || h == 0 || w as u64 * h as u64 > 400_000_000 {
        return None;
    }
    let kind = match reader.output_color_type().0 {
        png::ColorType::Grayscale => Kind::Gray,
        png::ColorType::GrayscaleAlpha => Kind::GrayAlpha,
        png::ColorType::Rgb => Kind::Rgb,
        png::ColorType::Rgba => Kind::Rgba,
        png::ColorType::Indexed => return None,
    };
    let (ow, oh) = fit_size(w, h, max);
    let mut ds = Downscaler::new(w, h, ow, oh);
    while let Ok(Some(row)) = reader.next_row() {
        ds.push_row(row.data(), kind);
    }
    Some(ds.finish())
}

/// Decodes a JPEG or PNG to at most `max`x`max` RGB.
pub fn decode(bytes: &[u8], max: u32) -> Option<Rgb> {
    match bytes {
        [0xFF, 0xD8, ..] => decode_jpeg(bytes, max),
        [0x89, b'P', b'N', b'G', ..] => decode_png(bytes, max),
        _ => None,
    }
}

/// Width and height of a JPEG or PNG, read from its header without decoding the pixels.
#[cfg(test)]
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    match bytes {
        [0xFF, 0xD8, ..] => {
            let mut d = jpeg_decoder::Decoder::new(Cursor::new(bytes));
            d.read_info().ok()?;
            let info = d.info()?;
            Some((info.width as u32, info.height as u32))
        }
        [0x89, b'P', b'N', b'G', ..] => {
            let reader = png::Decoder::new(Cursor::new(bytes)).read_info().ok()?;
            Some((reader.info().width, reader.info().height))
        }
        _ => None,
    }
}

/// The cover for a track: the file's own, then the one saved in Wisp for it (the user chose it for
/// this very song, so it beats the folder's), then a folder image.
pub fn cover_with_fallback(audio: &Path, embedded: Option<Vec<u8>>, saved: &str) -> Option<Vec<u8>> {
    embedded
        .or_else(|| (!saved.is_empty()).then(|| std::fs::read(saved).ok()).flatten())
        .or_else(|| folder_cover(audio.parent()?))
}

// ---------------------------------------------------------------------------------------------
// Backdrop + accent
// ---------------------------------------------------------------------------------------------

pub const BACKDROP_SIZE: u32 = 40;

fn resize(src: &Rgb, tw: u32, th: u32) -> Rgb {
    let mut ds = Downscaler::new(src.w, src.h, tw.min(src.w).max(1), th.min(src.h).max(1));
    for row in src.px.chunks_exact(src.w as usize * 3) {
        ds.push_row(row, Kind::Rgb);
    }
    ds.finish()
}

fn box_blur(img: &mut Rgb, radius: usize, passes: usize) {
    let (w, h) = (img.w as usize, img.h as usize);
    let mut tmp = vec![0u8; img.px.len()];
    for _ in 0..passes {
        for horizontal in [true, false] {
            let (len, lines) = if horizontal { (w, h) } else { (h, w) };
            for line in 0..lines {
                for pos in 0..len {
                    let lo = pos.saturating_sub(radius);
                    let hi = (pos + radius).min(len - 1);
                    let n = (hi - lo + 1) as u32;
                    for c in 0..3 {
                        let sum: u32 = (lo..=hi)
                            .map(|i| {
                                let (x, y) = if horizontal { (i, line) } else { (line, i) };
                                img.px[(y * w + x) * 3 + c] as u32
                            })
                            .sum();
                        let (x, y) = if horizontal { (pos, line) } else { (line, pos) };
                        tmp[(y * w + x) * 3 + c] = (sum / n) as u8;
                    }
                }
            }
            std::mem::swap(&mut img.px, &mut tmp);
        }
    }
}

fn to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if max == 0.0 { 0.0 } else { d / max }, max)
}

pub fn from_hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [((r + m) * 255.0).round() as u8, ((g + m) * 255.0).round() as u8, ((b + m) * 255.0).round() as u8]
}

/// A 40x40, heavily blurred, slightly more saturated copy of the cover: the ambient backdrop.
pub fn backdrop(art: &Rgb) -> Rgb {
    let mut small = resize(art, BACKDROP_SIZE, BACKDROP_SIZE);
    box_blur(&mut small, 3, 3);
    for p in small.px.chunks_exact_mut(3) {
        let (h, s, v) = to_hsv(p[0], p[1], p[2]);
        let rgb = from_hsv(h, (s * 1.25).min(1.0), (v * 1.05).min(1.0));
        p.copy_from_slice(&rgb);
    }
    small
}

/// The dominant vivid hue of the cover as `(hue, saturation, value)`, or `None` for artwork with
/// no real color (black and white, flat gray).
pub fn dominant_hsv(art: &Rgb) -> Option<(f32, f32, f32)> {
    let small = resize(art, 32, 32);
    // 12 hue buckets, each weighted by saturation * brightness so vivid pixels dominate.
    let mut weight = [0.0f32; 12];
    let mut sum = [[0.0f32; 3]; 12];
    for p in small.px.chunks_exact(3) {
        let (h, s, v) = to_hsv(p[0], p[1], p[2]);
        if s < 0.22 || v < 0.2 {
            continue;
        }
        let w = s * v;
        let bucket = ((h / 30.0) as usize).min(11);
        weight[bucket] += w;
        sum[bucket][0] += w * h;
        sum[bucket][1] += w * s;
        sum[bucket][2] += w * v;
    }
    let (best, &total) = weight.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
    if total < 1.5 {
        return None;
    }
    Some((sum[best][0] / total, sum[best][1] / total, sum[best][2] / total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut wr = enc.write_header().unwrap();
            let mut data = Vec::new();
            for y in 0..h {
                for x in 0..w {
                    data.extend(f(x, y));
                }
            }
            wr.write_image_data(&data).unwrap();
        }
        out
    }

    #[test]
    fn large_covers_are_downscaled_while_streaming() {
        let bytes = png_bytes(1600, 1200, |x, _| [(x * 255 / 1599) as u8, 40, 200]);
        let img = decode(&bytes, 400).unwrap();
        assert_eq!((img.w, img.h), (400, 300), "aspect ratio is kept");
        // A horizontal gradient stays monotonic and spans the range.
        let first = img.px[0];
        let last = img.px[(399) * 3];
        assert!(first < 3 && last > 250, "gradient endpoints {first}..{last}");
        for x in 1..400usize {
            assert!(img.px[x * 3] >= img.px[(x - 1) * 3], "monotonic at {x}");
        }
        assert_eq!(img.px[1], 40);
        assert_eq!(img.px[2], 200);
    }

    #[test]
    fn small_covers_are_never_upscaled() {
        let img = decode(&png_bytes(100, 100, |_, _| [9, 9, 9]), 512).unwrap();
        assert_eq!((img.w, img.h), (100, 100));
    }

    #[test]
    fn garbage_and_unsupported_data_is_rejected_quietly() {
        assert!(decode(b"GIF89a....", 100).is_none());
        assert!(decode(&[0xFF, 0xD8, 0, 1, 2, 3], 100).is_none());
        assert!(decode(&[0x89, b'P', b'N', b'G', 1, 2, 3], 100).is_none());
        assert!(decode(&[], 100).is_none());
    }

    #[test]
    fn backdrop_is_small_smooth_and_keeps_the_color() {
        // Left half blue, right half red: hard edge.
        let bytes = png_bytes(200, 200, |x, _| if x < 100 { [20, 30, 220] } else { [220, 30, 20] });
        let art = decode(&bytes, 200).unwrap();
        let b = backdrop(&art);
        assert_eq!((b.w, b.h), (40, 40));
        let row = |x: usize| &b.px[(20 * 40 + x) * 3..(20 * 40 + x) * 3 + 3];
        assert!(row(2)[2] > 150 && row(37)[0] > 150, "colors survive");
        let mid = row(20);
        assert!(mid[0] > 40 && mid[2] > 40, "the hard edge is blurred into a blend, got {mid:?}");
    }

    #[test]
    fn accent_picks_the_vivid_hue_and_ignores_gray() {
        let red = decode(&png_bytes(64, 64, |_, _| [230, 30, 40]), 64).unwrap();
        let (h, s, v) = dominant_hsv(&red).unwrap();
        assert!(h < 15.0 || h > 345.0, "hue {h}");
        assert!(s > 0.7 && v > 0.8);
        // Mostly gray with a little blue: blue still wins over the gray.
        let mixed = decode(&png_bytes(64, 64, |x, _| if x < 12 { [20, 80, 230] } else { [120, 120, 120] }), 64).unwrap();
        assert!(dominant_hsv(&mixed).is_none() || (200.0..250.0).contains(&dominant_hsv(&mixed).unwrap().0));
        let gray = decode(&png_bytes(64, 64, |x, _| [(x * 4) as u8; 3]), 64).unwrap();
        assert!(dominant_hsv(&gray).is_none(), "black and white art has no accent");
    }

    #[test]
    fn hsv_round_trips() {
        for rgb in [[255, 0, 0], [12, 200, 90], [30, 30, 220], [250, 250, 10]] {
            let (h, s, v) = to_hsv(rgb[0], rgb[1], rgb[2]);
            let back = from_hsv(h, s, v);
            for c in 0..3 {
                assert!((back[c] as i32 - rgb[c] as i32).abs() <= 1, "{rgb:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn folder_cover_prefers_cover_over_folder_and_ignores_other_images() {
        let dir = std::env::temp_dir().join(format!("wisp-art-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(folder_cover(&dir).is_none());
        std::fs::write(dir.join("holiday.jpg"), b"x").unwrap();
        std::fs::write(dir.join("Folder.PNG"), b"folder").unwrap();
        assert_eq!(folder_cover(&dir).unwrap(), b"folder");
        std::fs::write(dir.join("cover.jpg"), b"cover").unwrap();
        assert_eq!(folder_cover(&dir).unwrap(), b"cover");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_cover_saved_in_wisp_beats_the_folder_image_but_not_the_files_own() {
        let dir = std::env::temp_dir().join(format!("wisp-art-saved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cover.jpg"), b"folder").unwrap();
        let saved = dir.join("7.jpg");
        std::fs::write(&saved, b"saved").unwrap();
        let (audio, saved) = (dir.join("song.flac"), saved.to_string_lossy().into_owned());

        assert_eq!(cover_with_fallback(&audio, Some(b"own".to_vec()), &saved).unwrap(), b"own");
        assert_eq!(cover_with_fallback(&audio, None, &saved).unwrap(), b"saved", "chosen for this very song");
        assert_eq!(cover_with_fallback(&audio, None, "").unwrap(), b"folder");
        assert_eq!(cover_with_fallback(&audio, None, "gone.jpg").unwrap(), b"folder", "a vanished file falls through");
        std::fs::remove_file(dir.join("cover.jpg")).unwrap();
        assert!(cover_with_fallback(&audio, None, "gone.jpg").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
