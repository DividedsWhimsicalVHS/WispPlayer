//! Generates a demo music library to try Wisp without your own files.
//!
//!   cargo run --release --example demo_library -- <out_dir> [albums=24] [tracks=8] [seconds=4] [art=1]
//!
//! Writes silent MP3s (valid frames plus an Xing header) with ID3v2.4 tags, an embedded PNG cover,
//! embedded or sidecar lyrics for some tracks, and a folder `cover.png` for some albums. Also
//! writes a few audible sine-wave WAV files in `Tones/` so there is something to listen to.
//! `art=0` leaves out the embedded covers (every album gets a folder `cover.png` instead), which keeps
//! very large test libraries small.

use std::fs;
use std::path::Path;

const FRAME_LEN: usize = 417; // MPEG-1 Layer III, 128 kbps, 44.1 kHz, no padding
const FRAME_HEADER: [u8; 4] = [0xFF, 0xFB, 0x90, 0x64];
const SAMPLES_PER_FRAME: u32 = 1152;

const ARTISTS: [&str; 12] = [
    "Aurora Lane", "Björk Nilsson", "Cobalt Harbor", "Déjà Vu Club", "Echo & The Tides", "Fjord",
    "Glass Animals Of Doom", "Hollow Pines", "Iris Kaplan", "Jónsi Winter", "Koi Orchestra", "The Lumens",
];
const WORDS: [&str; 16] = [
    "Midnight", "Lantern", "Harbor", "Velvet", "Static", "Meridian", "Paper", "Orchid", "Signal", "Tide",
    "Aurora", "Echo", "Glass", "Ember", "Drift", "Halcyon",
];
const LYRICS: [&str; 8] = [
    "We were drifting through the static",
    "Lanterns on the water, burning slow",
    "Hold the line, the tide is turning",
    "Paper stars above the harbor",
    "Say my name like a secret",
    "Every signal finds its way home",
    "Velvet skies and amber light",
    "Stay a while, the night is young",
];

fn syncsafe(n: usize) -> [u8; 4] {
    [(n >> 21) as u8 & 0x7f, (n >> 14) as u8 & 0x7f, (n >> 7) as u8 & 0x7f, n as u8 & 0x7f]
}

fn frame(id: &str, payload: &[u8]) -> Vec<u8> {
    let mut f = id.as_bytes().to_vec();
    f.extend(syncsafe(payload.len()));
    f.extend([0, 0]);
    f.extend(payload);
    f
}

fn text(id: &str, s: &str) -> Vec<u8> {
    let mut p = vec![3u8]; // UTF-8
    p.extend(s.as_bytes());
    frame(id, &p)
}

fn id3(frames: Vec<Vec<u8>>) -> Vec<u8> {
    let body: Vec<u8> = frames.into_iter().flatten().collect();
    let mut tag = b"ID3".to_vec();
    tag.extend([4, 0, 0]);
    tag.extend(syncsafe(body.len()));
    tag.extend(body);
    tag
}

fn mp3(frames: u32, tag: Vec<u8>) -> Vec<u8> {
    let mut out = tag;
    // First frame: an Xing/Info header so players know the exact length.
    let mut first = vec![0u8; FRAME_LEN];
    first[..4].copy_from_slice(&FRAME_HEADER);
    first[36..40].copy_from_slice(b"Info");
    first[40..44].copy_from_slice(&3u32.to_be_bytes()); // frames + bytes present
    first[44..48].copy_from_slice(&(frames + 1).to_be_bytes());
    first[48..52].copy_from_slice(&((frames + 1) * FRAME_LEN as u32).to_be_bytes());
    out.extend(first);
    for _ in 0..frames {
        let mut f = vec![0u8; FRAME_LEN];
        f[..4].copy_from_slice(&FRAME_HEADER);
        out.extend(f);
    }
    out
}

/// A soft gradient cover, different per seed.
fn cover_png(seed: u32, size: u32) -> Vec<u8> {
    let hue = (seed * 47 % 360) as f32;
    let mut px = Vec::with_capacity((size * size * 3) as usize);
    for y in 0..size {
        for x in 0..size {
            let t = (x + y) as f32 / (2 * size) as f32;
            let h = (hue + 70.0 * t) % 360.0;
            let (s, v) = (0.75, 0.95 - 0.55 * t);
            let c = v * s;
            let xx = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
            let m = v - c;
            let (r, g, b) = match (h / 60.0) as u32 {
                0 => (c, xx, 0.0),
                1 => (xx, c, 0.0),
                2 => (0.0, c, xx),
                3 => (0.0, xx, c),
                4 => (xx, 0.0, c),
                _ => (c, 0.0, xx),
            };
            // A bright disc.
            let d = (((x as f32 - size as f32 * 0.4).powi(2) + (y as f32 - size as f32 * 0.6).powi(2)).sqrt() / (size as f32 * 0.3)).min(1.0);
            let k = (1.0 - d).powf(0.8) * 0.35;
            for ch in [r + m, g + m, b + m] {
                px.push(((ch * (1.0 - k) + k) * 255.0) as u8);
            }
        }
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, size, size);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&px).unwrap();
    out
}

fn wav_tone(path: &Path, hz: f32, seconds: u32) {
    let rate = 44_100u32;
    let frames = rate * seconds;
    let mut pcm = Vec::with_capacity(frames as usize * 4);
    for n in 0..frames {
        let t = n as f32 / rate as f32;
        let env = (t * 6.0).min(1.0) * ((seconds as f32 - t) * 6.0).clamp(0.0, 1.0);
        let s = ((2.0 * std::f32::consts::PI * hz * t).sin() * 0.25 * env * 32767.0) as i16;
        pcm.extend(s.to_le_bytes());
        pcm.extend(s.to_le_bytes());
    }
    let mut f = Vec::new();
    f.extend(b"RIFF");
    f.extend((36 + pcm.len() as u32).to_le_bytes());
    f.extend(b"WAVEfmt ");
    f.extend(16u32.to_le_bytes());
    f.extend(1u16.to_le_bytes());
    f.extend(2u16.to_le_bytes());
    f.extend(rate.to_le_bytes());
    f.extend((rate * 4).to_le_bytes());
    f.extend(4u16.to_le_bytes());
    f.extend(16u16.to_le_bytes());
    f.extend(b"data");
    f.extend((pcm.len() as u32).to_le_bytes());
    f.extend(pcm);
    fs::write(path, f).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(out) = args.first() else {
        eprintln!("usage: demo_library <out_dir> [albums=24] [tracks=8] [seconds=4] [art=1]");
        std::process::exit(2);
    };
    let albums: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(24);
    let tracks: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
    let seconds: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(4);
    let embed_art = args.get(4).is_none_or(|s| s != "0");
    let root = Path::new(out);
    let frames = (seconds * 44_100).div_ceil(SAMPLES_PER_FRAME);

    for a in 0..albums {
        let artist = ARTISTS[a as usize % ARTISTS.len()];
        let mut title = format!("{} {}", WORDS[a as usize % WORDS.len()], WORDS[(a as usize * 7 + 3) % WORDS.len()]);
        // The artist/word pairs repeat every 48 albums; a number keeps larger libraries distinct.
        if a >= 48 {
            title = format!("{title} {}", a / 48 + 1);
        }
        let dir = root.join(artist).join(&title);
        fs::create_dir_all(&dir).unwrap();
        let cover = cover_png(a, 400);
        let year = 1975 + (a * 3) % 50;
        let folder_cover = !embed_art || a % 5 == 4; // some albums have no embedded art, only cover.png
        if folder_cover {
            fs::write(dir.join("cover.png"), &cover).unwrap();
        }
        for t in 1..=tracks {
            let name = format!("{} {}", WORDS[(a as usize + t as usize * 5) % WORDS.len()], WORDS[(t as usize * 3 + 1) % WORDS.len()]);
            let mut frames_tag = vec![
                text("TIT2", &name),
                text("TPE1", artist),
                text("TALB", &title),
                text("TPE2", artist),
                text("TRCK", &format!("{t}/{tracks}")),
                text("TYER", &year.to_string()),
                text("TDRC", &year.to_string()),
            ];
            if !folder_cover {
                let mut p = vec![3u8];
                p.extend(b"image/png\0");
                p.push(3); // front cover
                p.push(0);
                p.extend(&cover);
                frames_tag.push(frame("APIC", &p));
            }
            let lyric_style = (a + t) % 4;
            if lyric_style == 1 {
                // Embedded unsynchronized lyrics (USLT).
                let mut p = vec![3u8];
                p.extend(b"eng\0");
                p.extend(LYRICS.join("\n").as_bytes());
                frames_tag.push(frame("USLT", &p));
            }
            let file = dir.join(format!("{t:02} {name}.mp3"));
            fs::write(&file, mp3(frames, id3(frames_tag))).unwrap();
            if lyric_style == 2 {
                // Sidecar synchronized lyrics.
                let per = (seconds as f32 / LYRICS.len() as f32).max(0.5);
                let lrc: String = LYRICS
                    .iter()
                    .enumerate()
                    .map(|(i, l)| {
                        let s = i as f32 * per;
                        format!("[{:02}:{:05.2}]{l}\n", (s / 60.0) as u32, s % 60.0)
                    })
                    .collect();
                fs::write(file.with_extension("lrc"), format!("[ti:{name}]\n[ar:{artist}]\n{lrc}")).unwrap();
            }
        }
    }

    // Something audible.
    let tones = root.join("Tones");
    fs::create_dir_all(&tones).unwrap();
    for (i, hz) in [261.63f32, 329.63, 392.0, 523.25].iter().enumerate() {
        wav_tone(&tones.join(format!("{:02} Sine {:.0} Hz.wav", i + 1, hz)), *hz, 6);
    }
    println!("wrote {} albums x {} tracks ({} s each) to {}", albums, tracks, seconds, root.display());
}
