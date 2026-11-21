<p align="center"><img src="assets/icon/wisp-128.png" width="96" height="96" alt="Wisp Player's icon: a retro cassette player"></p>

# Wisp Player

A featherweight, gapless desktop audio player for local files, written in Rust. Free software under the GNU GPL v3, designed and written by Claude (see [Credits](#credits)).

**[Download the Windows installer](https://github.com/DividedsWhimsicalVHS/WispPlayer/releases/latest)**: it adds a desktop shortcut and a Start Menu entry, and needs no administrator rights.

* **Small:** about 10 MB of RAM (private working set) with a few hundred tracks loaded, about 13 MB with ten thousand. No WebView, no GPU context, no scripting runtime.
* **Glitch-free by construction:** the audio callback never allocates, locks or waits; the UI thread cannot starve it.
* **Gapless:** sample-exact track-to-track hand-over, including when files and device use different sample rates.
* **Apple Music–inspired UI:** artwork-tinted ambient backdrop, synced lyrics, rounded translucent surfaces, and **35 themes**, from Catppuccin and Nord to Winamp, Windows 95 and Liquid Glass, plus an Album Chameleon that takes its colours from the cover.
* **Playlists**, and a **tag editor** for songs that arrive without proper tags: Wisp flags them, you search for each one, pick the right match (album, cover and lyrics included) and only then is anything saved. Nothing is renamed, moved or retagged on its own.

On Windows the whole dependency tree is pure Rust: nothing is compiled from C, and nothing beyond the operating system's own audio, windowing and TLS APIs is needed at run time.

## Contents

1. [Features](#features)
2. [Themes](#themes)
3. [Install](#install)
4. [Build and run](#build-and-run)
5. [Keyboard shortcuts](#keyboard-shortcuts)
6. [Crates and why](#crates-and-why)
7. [Project layout](#project-layout)
8. [How it stays small](#how-it-stays-small)
9. [How it stays glitch-free](#how-it-stays-glitch-free)
10. [Measured results](#measured-results)
11. [Testing](#testing)
12. [Limitations](#limitations)
13. [Credits](#credits)
14. [Licensing](#licensing)

## Features

**Playback**

* MP3, FLAC, WAV, AAC (LC), OGG Vorbis, Opus, ALAC, AIFF, and the sound of **MP4, M4V, WebM and MKV video files** (and `.mka` / `.weba`, the same containers holding only sound): Wisp plays the soundtrack and shows no picture. A video is listed like any song, named from its tags or, failing that, from its file name; an untagged one is flagged like any untagged song and can be looked up in the tag editor.
* Gapless playback; seeking by clicking or dragging the progress bar; repeat off / all / one; shuffle.
* Smooth volume (perceptual curve, 6 ms smoothing, never a click), mute, 12 ms fades on pause, resume, seek and skip.
* ReplayGain track or album mode (REPLAYGAIN tags and Opus R128 headers), pre-amp, clip prevention.
* Mono is spread to both speakers, surround is folded down, stereo on a surround device uses the front pair.
* If the output device disappears (unplugged, sample-rate change by the OS) Wisp reopens it and resumes at the same position.
* A damaged file is skipped with a notice instead of stopping playback.

**Library**

* Add any number of folders; scanning runs in the background and is incremental (unchanged files are skipped, vanished files are removed, an unreadable folder is never mistaken for deleted files). A file whose sound Wisp can't decode (a film's Dolby Digital or 5.1 AAC soundtrack, say) is left out rather than listed as a song that fails to play.
* Songs, Albums and Artists views; search that ignores case and accents and matches words in any order; sort by title, album or recently added.
* Queue: play next, add to end, remove, clear (keeps the current track), save as a playlist.

**Playlists**

* Create, rename and delete playlists from the sidebar; add any song with the playlist button that appears when you point at it (or straight into a new playlist); remove songs; play or shuffle a playlist.
* Each playlist is a plain `.m3u8` file in the `Playlists` folder of Wisp's data folder, so other players can read it and it survives a rebuilt library index. Playlists from other players dropped into that folder show up too.

**Unidentified songs and the tag editor**

Wisp never renames, moves or retags anything by itself. While it scans, it only *notes* which songs leave something out, from the tags they already have (nothing is guessed from the file name at this point):

* the **artist** is missing, empty or "Unknown Artist";
* the **title** is missing or is really a file name (`track_01`, `AUDIO_294`, `01 - Song`, `Song.mp3`, `Artist_-_Song`, a download's `(Official Video)` or `[320kbps]`);
* there is **no cover inside the file**. A `cover.jpg` in the folder doesn't count.

*Settings → Library → Unidentified tracks* shows how many there are (`14 Unidentified`) and opens the list, where each such song is highlighted and says what it lacks. Tapping one opens the **track lookup & tag editor**:

* The search box starts with the file name cleaned of track numbers and upload tags (`04. Daft Punk - Around the World (Official Audio) [320kbps].mp3` becomes `Daft Punk - Around the World`), or the song's own partial title. Change it and press Enter to search again. A search is made when the editor opens and whenever you press Enter or *Search*; typing alone sends nothing.
* The matches come from MusicBrainz, best first: official studio albums before singles, compilations, live and unofficial releases, with recordings about as long as your file ahead of the rest. Each shows its cover (Cover Art Archive), title and artists, album, year and track number, and whether LRCLIB has synced (`.lrc`) or plain lyrics for it.
* Picking a match only fills the form: title, artist, album, album artist, year, track, cover and lyrics. Everything stays editable. The cover can be replaced with an image from your disk, and when a song has both kinds of lyrics you choose between synced and plain.
* **Save** writes the ID3v2 tags (title, artist, album, album artist, track, year, the front cover and the lyrics, which hold the `.lrc` text) straight into an **MP3**: every other frame in the file is kept as it was, the audio is never touched, and the file is written beside the original and renamed over it. **Other formats** (FLAC, M4A, MP4, WebM...) are not rewritten: the details go into Wisp's library, a chosen cover into its `Covers` folder, and the file stays exactly as it was. Either way a song leaves the list once nothing is left to fix; save it without an artist, say, and it stays listed.

MusicBrainz asks every application to identify itself with a way to contact its operator, and refuses anonymous requests at busy times. Put your email address or website in *Settings → Library → MusicBrainz contact*; it is sent with each search (as the User-Agent) and nowhere else.

**Lyrics**

* Synchronized `.lrc` files next to the song (all common timestamp spellings, repeated stamps, offsets), or lyrics embedded in the tags (the ones the tag editor writes included), or lyrics the tag editor kept in Wisp's library for a format it can't tag, or a plain `.txt`.
* Synced lyrics highlight the current line and scroll to follow it; unsynced lyrics are shown as plain text; a friendly message appears when there are none.

**Interface and customization**

* Ambient backdrop: a blurred copy of the album art, cross-faded between tracks.
* 35 themes (see [Themes](#themes)), chosen from a gallery of miniatures in Settings.
* Accent colour that follows the artwork (or the theme's own), or one of eight presets, or any `#RRGGBB`.
* List density (compact / comfortable / spacious), interface size 90 %–150 %, backdrop on/off.
* Audio output device, normalization mode, pre-amp, clip prevention, library folders, scan on start.
* The window opens fitted to the screen it appears on and remembers its size.

## Themes

*Settings → Theme* shows every theme as a miniature of the interface; click one and the whole window changes at once. The choice is remembered (`theme_id` in the settings file), and settings from earlier versions (Dark or Light) become Wisp Dark or Wisp Light.

| Group | Themes |
| --- | --- |
| Wisp | Wisp Dark, Wisp Light |
| Developer favorites | Catppuccin Mocha, Catppuccin Latte, Nord, Gruvbox Dark, Gruvbox Light, Tokyo Night, Rosé Pine, Rosé Pine Dawn, Dracula, Everforest, Solarized Dark, Solarized Light |
| Retro & nostalgic | Frutiger Aero, Winamp Classic (green), Winamp Amber, Vaporwave, iPod Classic LCD, iPod Classic Black, iPod Classic Silver, Teenage Engineering, Windows 95 |
| Modern glass & light | Album Chameleon, Liquid Glass, Liquid Glass Light, OLED Pitch Black, Swiss Editorial, Neumorphism Light, Neumorphism Dark |
| Moods & atmospheres | Lo-Fi Cafe, Cyberpunk, Synthwave Midnight, Forest Moss, High Contrast |

A theme is a palette plus a few style traits that cost nothing to draw: how round corners are (Nord and Gruvbox are nearly square, Neumorphism very soft), border width, the font and a separate font for titles (Consolas for Winamp, Georgia titles for Rosé Pine, Arial Black for Swiss Editorial), a glow around the accent (Tokyo Night, Vaporwave, Synthwave), a glassy highlight on buttons (Frutiger Aero, iPod), beveled edges (Winamp, Windows 95), the paired light and dark shadows of neumorphic surfaces, gradient window backgrounds, a sepia or lamp-light cast over the artwork, and whether the seek and volume bars are plain, accent-coloured or an accent gradient.

* **Accent.** With *Accent color* on Auto, a theme uses its own accent, except that Wisp Dark, Wisp Light, Liquid Glass and Album Chameleon follow the artwork as before. A preset or a custom `#RRGGBB` overrides the accent in every theme. Accent-coloured text (an artist name, the playing track) is nudged toward the text colour wherever the accent alone would be hard to read.
* **Ambient backdrop.** Themes that are meant to be flat (Nord, Solarized, Winamp, Windows 95, OLED, Neumorphism, High Contrast and similar) don't show the blurred artwork; Settings says so on the *Ambient backdrop* row. The others dim it with a theme-specific scrim chosen so text stays readable over any cover, from black to white.
* **Album Chameleon** builds its whole palette from the playing track: the cover's dominant hue tints the background, the text is light or dark depending on how bright the cover is, and the accent is the cover's colour, deepened or lightened until it stands out.
* **Legibility is tested, not eyeballed.** For every palette the unit tests compute WCAG contrast for text, secondary text and tertiary text on every surface they are drawn on (window, sidebar, cards, player bar, menus, both ends of a gradient), for the accent, for the label on accent buttons, and for text over artwork through the scrim; Album Chameleon is swept over a grid of cover colours. High Contrast is held to much stricter limits.

## Install

On Windows 10 or 11 (64-bit), download `WispPlayer-Setup-<version>.exe` from the [latest release](https://github.com/DividedsWhimsicalVHS/WispPlayer/releases/latest) and run it. The setup wizard

* installs Wisp Player for your account only, in `%LOCALAPPDATA%\Programs\Wisp Player` (you can pick another folder), so it needs no administrator rights;
* puts a **shortcut on your desktop** (the box is ticked; untick it if you don't want one) and an entry in the Start Menu, both with the cassette-player icon;
* offers to start Wisp Player when it finishes.

To remove it, use *Settings → Apps → Installed apps* (or the Start Menu entry's uninstaller). Your library, settings and playlists in `%LOCALAPPDATA%\Wisp` stay unless you answer yes when the uninstaller asks whether to delete them.

The installer is not code-signed (a certificate costs money), so Windows SmartScreen may say "Windows protected your PC" the first time: choose *More info*, then *Run anyway*. Each release lists the installer's SHA-256 checksum, so you can check the download with `Get-FileHash`.

## Build and run

### Windows

1. Install Rust (https://rustup.rs) with the default **MSVC** toolchain. Edition 2024 needs Rust 1.85 or newer (developed on 1.100).
2. Install the **Visual Studio Build Tools** with the *MSVC x64/x86 build tools* and a *Windows SDK* (the "Desktop development with C++" workload covers both). The GNU/MinGW toolchain is not supported.
3. Build and run:

```powershell
cargo run --release
```

The first release build takes a few minutes (fat LTO, one codegen unit); the result is a single `wisp.exe` of about 20 MB with no runtime dependencies. Its icon and version information are added by `build.rs` with `rc.exe` from the Windows SDK; without the SDK the program is still built, just without them. Prebuilt installers are on the [Releases page](https://github.com/DividedsWhimsicalVHS/WispPlayer/releases).

If your project path is long, a short build directory avoids Windows path-length trouble:

```powershell
$env:CARGO_TARGET_DIR = "C:\wisp-target"
```

### The installer

The setup program is made with [Inno Setup 6](https://jrsoftware.org/isinfo.php) from `installer\WispPlayer.iss`. After `cargo build --release`:

```powershell
iscc /DExePath=target\release\wisp.exe installer\WispPlayer.iss
```

writes `dist\WispPlayer-Setup-<version>.exe` (about 9 MB). Without `/DExePath` the script looks for `target\release\wisp.exe`.

### The icon

`assets\icon\wisp.svg` (the full design) and `wisp-small.svg` (a bolder one for 16–32 px) are the masters. `assets\icon\build-icons.ps1` turns them into `wisp.ico` and `wisp-128.png` with the Edge browser that ships with Windows; nothing else needs installing:

```powershell
powershell -ExecutionPolicy Bypass -File assets\icon\build-icons.ps1
```

### Linux and macOS

The code is written to be portable (cpal, winit and Slint are cross-platform) but only Windows has been built and tested. On Linux you will need the ALSA and OpenSSL development headers (`libasound2-dev`, `libssl-dev`; OpenSSL is what the system TLS means there), and the folder picker (`rfd`) needs a backend feature enabled in `Cargo.toml`.

### Try it without your own music

```powershell
cargo run --release --example demo_library -- demo-music 40 10 4
```

writes 400 tiny silent MP3s (tags, embedded and folder covers, `.lrc` and embedded lyrics) plus four audible sine-wave WAVs in `demo-music/Tones`. Arguments: `<dir> [albums] [tracks] [seconds] [art]`; pass `0` for `art` to leave out embedded covers when generating very large libraries. Then point Wisp at the folder in *Settings → Library*.

### Where things live

| What | Where |
| --- | --- |
| Settings (plain `key=value` text) and library index | `%LOCALAPPDATA%\Wisp` (Windows), `~/Library/Application Support/Wisp` (macOS), `$XDG_DATA_HOME/Wisp` (Linux) |
| Playlists (`.m3u8`) and covers chosen in the tag editor for files that can't hold one | `Playlists` and `Covers` inside that folder |
| Override the location | set `WISP_DATA_DIR` |
| Playback diagnostics | set `WISP_DEBUG=1`: prints track, position and underrun count every 2 s while playing, and a line when a library scan ends |

Settings are written atomically (temp file + rename), so a crash cannot leave a half-written file.

### Headless UI snapshots

```powershell
cargo run --features screenshot -- --screenshot out.png --scene np --theme catppuccin-mocha --size 1180x760
```

renders the interface with demo data (scenes: `songs`, `albums`, `artists`, `queue`, `settings`, `detail`, `unidentified`, `editor`, `np`, `np-queue`, `empty`, `toast`) using the software renderer, no window required. `--theme` takes any theme id from `src/themes.rs` (`wisp-dark` by default; `--light` is short for `wisp-light`).

## Keyboard shortcuts

| Key | Action |
| --- | --- |
| Space | Play / pause |
| ← / → | Seek back / forward 5 s |
| ↑ / ↓ | Volume ±5 % |
| M | Mute |
| Ctrl+F | Focus search |
| Esc | Close the tag editor, or Now Playing, or clear the search |

## Crates and why

| Crate | Role | Why this one |
| --- | --- | --- |
| `slint` (winit backend, **software renderer**) | UI | Native, declarative, no WebView. The CPU renderer means no GL/D3D/Vulkan context is ever created, which is where much of a typical toolkit's baseline RAM goes. Layout and drawing are retained and incremental: only changed regions are repainted. |
| `symphonia` | Demux and decode MP3, AAC, ALAC, FLAC, Vorbis, PCM/WAV, AIFF; demux MP4/M4A and Matroska/WebM; read ID3/APE tags | Pure Rust, streaming, allocation-light. |
| `opus-decoder` | Opus decode | Pure Rust; Symphonia only demuxes Opus. Correct but not optimized yet: see *Measured results*. |
| `rubato` | Sample-rate conversion | High-quality FFT resampler that can run as one continuous stream across tracks. |
| `cpal` | Audio output (WASAPI) | The standard cross-platform audio I/O layer. |
| `crossbeam-queue` | Lock-free ring buffers | The real-time hand-off between decoder and audio callback. |
| `redb` | Library index | Embedded, pure-Rust, copy-on-write B-tree with a small, bounded page cache. |
| `jpeg-decoder`, `png` | Artwork | JPEG is decoded at 1/2, 1/4 or 1/8 scale straight from the DCT coefficients; PNG streams through a row-by-row downscaler. A 4000×4000 cover costs a few hundred KB, transiently. |
| `rfd` | Folder and cover-image pickers | Native dialog, no UI toolkit pulled in. |
| `fastrand` | Shuffle | Tiny. |
| `ureq` (with `native-tls`) | HTTPS for MusicBrainz, the Cover Art Archive and LRCLIB | Small, blocking (it runs on its own thread anyway). TLS and certificates come from the operating system (SChannel on Windows), so no crypto library is compiled in. |
| `serde_json` | Reading MusicBrainz and LRCLIB answers | The standard. |

## Project layout

```
Cargo.toml            dependencies and the release profile (fat LTO, 1 codegen unit, stripped)
build.rs              compiles the Slint UI; puts the icon and version information into wisp.exe
LICENSE               GNU General Public License v3
THIRD-PARTY-LICENSES.md  the crates that go into the Windows build, with their licenses
assets/icon/          the cassette-player icon: SVG masters, wisp.ico, wisp-128.png and the script that makes them
installer/            Inno Setup script for the Windows installer
examples/
  demo_library.rs     generates a demo music library
src/
  main.rs             entry point; headless screenshot CLI (feature "screenshot")
  app.rs              controller: wires the UI to the player, library and settings
  app/editor.rs       controller of the tag editor
  settings.rs         key=value settings file, data directory
  themes.rs           the 35 themes: palettes, style traits, artwork-derived colours, contrast tests
  playlists.rs        playlists as .m3u8 files
  edit.rs             saving the tag editor's result: ID3 tags into an MP3, the library for other formats
  lookup/
    mod.rs            one search: MusicBrainz, then lyrics and covers as they arrive; stale answers dropped
    musicbrainz.rs    queries, reading answers, ranking the matches
    lrclib.rs         reading LRCLIB answers, choosing the lyrics of the right version
    net.rs            the three web services, MusicBrainz paced to its rate limit
    id3.rs            ID3v2 tag writer (merges into the existing tag, atomic)
  models.rs           lazy list models (rows are fetched from the index only when drawn)
  loader.rs           background artwork / lyrics loading
  art.rs              streaming cover decoding, backdrop blur, accent colour
  lyrics.rs           .lrc and plain-text lyrics
  library/            redb index, tag reading and flagging, incremental folder scan, file-name cleaning
  player/
    mod.rs            Player handle used by the UI
    engine.rs         engine thread: decode → mix → resample → chunks; gapless hand-over
    pipe.rs           the real-time boundary: chunk queues and the audio-callback renderer
    decode.rs         opening files, Symphonia/Opus behind one interface
    mixdown.rs        channel-layout conversion
    resample.rs       streaming resampler wrapper with exact output length
    replaygain.rs     ReplayGain / R128 tags
    queue.rs          queue, shuffle, repeat
    output.rs         cpal backend (behind a small trait so tests use a fake sound card)
    tests.rs          engine tests against a fake sound card
  demo.rs, shot.rs    headless screenshots (feature "screenshot")
  testdata/           small fixtures for the tests (MP4, WebM and Matroska files with a sine-wave soundtrack)
ui/
  app.slint           window, Now Playing overlay, keyboard handling
  state.slint         the single global `App` that holds UI state and callbacks
  theme.slint         the Theme global (colour and style tokens that themes.rs fills in), icons
  widgets.slint       buttons, slider, toggle, segmented control, virtualized list
  rows.slint          song, queue, album, artist and unidentified-song rows
  pages.slint         Songs / Albums / Artists / Queue / album detail / Unidentified tracks / Settings
  editor.slint        the track lookup & tag editor
  bar.slint           player bar
  lyrics.slint        lyrics view with follow-scroll
```

## How it stays small

* **Nothing is loaded that is not on screen.** The library index lives on disk (redb). A view such as "all songs" is just a `Vec<u32>` of track ids (4 bytes each, ~400 KB for 100 000 tracks); the text of a row is read from the index only when the list actually draws it.
* **Artwork is decoded lazily and at the size it is shown:** thumbnails for visible album cards, the current track's cover, a 40×40 blurred backdrop that the renderer stretches. Everything is kept in small caches and dropped when it leaves the screen. Covers are held as 3-byte RGB.
* **Metadata scanning never decodes audio** and streams one directory at a time; the scan keeps at most a small batch of records in memory.
* **A fixed audio budget.** Playback memory is a ring of 24 chunks of 20 ms (about 190 KB at 48 kHz stereo), allocated once.
* **No runtime baggage:** a single native process, software rendering, `opt-level = 3` with fat LTO and stripped symbols, `panic = "unwind"` kept only so a decoder bug on a corrupt file can be caught and skipped.
* The largest single item is the window's pixel buffer (about 4 bytes per pixel, so ~6 MB at 1770×900). That is the price of CPU rendering and scales with the window size.

## How it stays glitch-free

```
 UI thread ──commands──▶ engine thread ──chunks (lock-free)──▶ audio callback ──▶ device
     ▲                    decode · mix · resample                 fades · volume · gain
     └──── atomics: position, state, volume ◀──────────────────────────┘
```

* **The audio callback is wait-free.** It only pops finished chunks from a lock-free queue and applies fades, volume and gain. It never allocates, locks, or makes a system call; chunks circulate between a `full` and a `free` queue so nothing is freed on the audio thread either.
* **The decoder runs about 480 ms ahead.** That cushion absorbs scheduling hiccups and disk latency, and makes pause, seek and skip instant.
* **Chunks describe themselves** (track, position, ReplayGain). Track changes, the reported position and end-of-queue detection happen exactly at the chunk where the audio itself changes, so the UI can never drift from what you hear.
* **One conversion stream across tracks.** Consecutive tracks of the same format go through the same mixer and resampler, so a recording split into tracks is resampled as the continuous signal it is. The next file is opened 8 s before the current one ends.
* **Every transition is a short fade** (12 ms) and gain changes are smoothed (6 ms), so pause, resume, seek, skip, volume and mute never click.
* **Seeking and flushing use an epoch handshake** with the callback, so no stale audio leaks through after a seek.
* **The UI thread cannot stall audio.** UI work (including a full-library scan or artwork decoding) happens on other threads, and the shared state is a handful of atomics.
* **Failures degrade instead of stopping:** an undecodable file is reported and skipped; a decoder panic is caught; a lost output device is reopened at the same position; an unreadable library folder is not treated as deleted.

## Measured results

Release build, Windows 11, 1920×1080 display at 150 % scale, window about 1770×900 px, libraries of generated demo files (the audio is silence, so decoding cost is typical but nothing is audible). "Memory" is the *private working set* (what Task Manager shows in its Memory column) of the whole process; CPU is the share of one core.

| Situation | Memory | CPU |
| --- | --- | --- |
| Idle, 404-track library | **10.6 MB** (11.6 MB committed) | under 1 % |
| Playing a 4-minute MP3 (44.1 kHz on a 48 kHz device, so resampled), Songs page visible | 10.7 MB | **1.9 %** (UI 0.9 %, decoder/resampler 1.0 %) |
| …with Now Playing open (artwork backdrop, lyrics view) | 10.7 MB | 1.8 % |
| After playing four tracks with artwork through Now Playing | 12.7 MB | |
| 10 004-track library, idle, and after scrolling through it | **13.5 MB** (index file 8.5 MB) | |
| With the tag editor open after a search (HTTPS, JSON, eight cover thumbnails), 4-song library | 13.7–14.0 MB | |

**Opus is the expensive codec.** Decoding Opus (the sound of most WebM files, and of `.opus` files) costs about **37 % of one core** in the decoder thread for stereo at 48 kHz, where the whole player costs 1–3 % of a core on MP3, WAV or AAC. The pure-Rust Opus decoder is correct but not optimized yet. It plays with 0 underruns, but it is a lot of work for a battery: this machine decodes it at about 2.7 times real time, so a processor several times slower would not keep up.

**Video files.** A three-minute 720p H.264 + AAC MP4 of 196 MB (only its sound is decoded; the picture data is read past at playback speed) used 12.9 MB of private working set (15.9 MB committed) and 3.1 % of one core while playing, with **0 underruns** before and after a seek made by clicking the progress bar. A 65 MB WebM (three minutes of Opus beside a picture track that is read past) used 11.7 MB and played and seeked forward and back, also with 0 underruns.

Playback health, read from the program's own underrun counter (`WISP_DEBUG=1`): **0 underruns** in every run: 43 s of one long track, a 45 s hands-off run across ten consecutive tracks (WAV and MP3, resampled 44.1 → 48 kHz), and an interactive run with a track change, a seek, a skip and opening Now Playing. The counter only counts audio that ran dry in the middle of playback; the short wait while a file is opened after you press play, seek or skip is not counted.

**Themes cost nothing you can measure.** The same three-minute track (44.1 kHz on a 48 kHz device, Songs page visible, 1792×950 window) was played in the build before themes existed and in the current one under six themes. Memory stayed between 10.8 and 11.8 MB and CPU between 1.4 and 3.3 % of one core every time (before themes: 10.8–11.6 MB, 1.4–2.9 %). The themes with translucent layers and soft shadows, Frutiger Aero (3.3 %) and Neumorphism Light (3.1 %), are at the top of that range; Wisp Dark and Album Chameleon (1.4 %) at the bottom. Switching between two themes while a track played, and opening the settings gallery, produced **0 underruns**.

A first scan of 10 000 files took 20–30 s cold (bound by the disk; about 3 s of CPU) and about 1.5 s when the files were already cached. The interface stays usable while it runs.

These numbers come from one machine. What changes most with a different setup is the window size (the pixel buffer is the largest single item) and, for the library, the number of tracks (about 0.3 MB per thousand).

## Testing

```powershell
cargo test
```

runs 137 tests, including engine tests that drive the full pipeline against a **fake sound card** which captures every sample it is asked to play. They check, sample-exactly: gapless joins (native and resampled), that the reported position matches the audio actually heard through seeks, pause/resume without skipped or repeated audio, queue edits invalidating the prefetched next track, skipping unplayable files, recovery after losing the device, volume/mute without steps, and decoding of every supported container/bit-depth combination that can be generated without an encoder (WAV 8/16/24-bit, AIFF). Real video and download files are decoded too: an MP4 encoded by Windows' own H.264 and AAC encoders, and WebM and Matroska files carrying real Opus and Vorbis (from an encoder) or that same AAC and H.264, muxed by a small script, one of them shaped like a browser recording (no duration, no index). Each must decode to the sine wave it carries (checked by frequency, with the picture track ignored), seek forward and back, and be listed by a scan. Library, scanner, tag, lyrics, settings, resampler, mixdown, ReplayGain and queue logic have their own tests. So do the newer parts: what flags a song (a missing artist, a title that is only a file name, no embedded cover), the file-name cleaning that fills the search box, the ranking of MusicBrainz matches (reversed names, live versus studio, editions, singles, namesakes of another length), choosing the lyrics of the right version from LRCLIB, the ID3v2 writer (versions 2.3 and 2.4, unsynchronisation, extended headers, stacked tags, every other frame kept byte for byte, atomic replacement, read back through Symphonia), saving an edit into an MP3 or into the library, an index of an older layout being discarded, and playlists. The themes have their own: every palette must meet WCAG contrast ratios for text, accent and artwork-backed text (see [Themes](#themes)), and every theme id must be unique and resolvable.

`cargo test -- --ignored` additionally talks to the real services: it downloads an album cover from the Cover Art Archive and runs the editor's whole search for "Flashing Lights" on MusicBrainz and LRCLIB, printing the ranked matches and the lyrics it finds.

Also exercised end to end on a real Windows machine by driving the running program and reading its playback counters: the audio stream opens, the position advances in real time, track hand-overs, seeks and skips produce no underruns, the window fits the screen, interface scaling works live and across restarts, Now Playing shows artwork-derived colours and synced lyrics, and a 10 000-track library scans and scrolls. What these checks cannot do is *listen*: nobody has auditioned the sound by ear yet, so give it a try with music you know well.

## Limitations

* **Tested on Windows only.** Linux and macOS code paths compile in principle but have not been built or run.
* **FLAC and ALAC decoding relies on the upstream decoders and has not been exercised with real files in this project's tests** (they need an encoder to produce); WAV, AIFF, MP3, AAC, Opus and Vorbis have (the last three inside MP4, WebM and Matroska files). AAC means stereo or mono AAC-LC: HE-AAC (SBR) and 5.1 AAC are not supported by Symphonia.
* **Video files: only the sound plays.** There is no picture, and a video has no cover art unless it is tagged, was given one in the tag editor, or there is a `cover.jpg` in its folder. Soundtracks Wisp can't decode (Dolby Digital, DTS, 5.1 AAC, HE-AAC) are left out of the library. Only MP4, M4V, WebM and MKV are scanned: not MOV, AVI, FLV or TS. Matroska tags are read at the track level, the way MKVToolNix writes them; the whole-file title that ffmpeg and yt-dlp write belongs to the video, which Wisp does not use, so those files are named from their file name. Pictures attached to a Matroska file are not used as cover art. A browser recording (no index) can be seeked back only by reading the file again from the start, and shows no duration.
* Output uses the system mixer (shared mode). There is no exclusive/bit-perfect mode and no ASIO.
* No media-key or system media-overlay integration, no tray icon, no CUE sheets, no network streams or radio, no equalizer.
* Lookups work from the text you search for, not from the audio: there is no fingerprinting (AcoustID would need an application key). A song MusicBrainz doesn't know can still be filled in by hand.
* Without a MusicBrainz contact in Settings, MusicBrainz may refuse or delay a search at busy times; the editor says so when it happens.
* Only MP3 files are tagged in place. For other formats what you enter lives in Wisp's library, not in the file, so other players don't see it, and it is dropped if the file itself later changes and is read again.
* A song is flagged when it has no cover *inside the file*, so a library whose covers are only `cover.jpg` files in the album folders lists all of those songs until each is given a cover.
* Playlists can't be reordered by dragging yet.
* The interface is English only.
* Themes change colours, shapes, fonts and a few cheap effects (glow, gloss, bevels, soft shadows). They don't add anything that needs animation, textures or audio analysis: no spectrum visualizers or waveforms, perspective neon grids, film grain, bokeh or paper textures, cassette or vinyl animations, reflective cover floors, chamfered corners, custom title bars or status bars, mesh gradients, glitch effects, or readout-style labels such as `TRACK 04 // 03:42`. Fonts such as Consolas, Georgia and Bahnschrift are the ones that ship with Windows; where a font is missing the system default is used.
* The speed of a first scan depends on your disk; the interface stays responsive while it runs.

## Credits

Wisp Player was designed and written by **Claude**, an AI model made by Anthropic (Claude Sonnet 5.5), working with this repository's owner in a single long conversation: the player and its audio engine, the library and tag editor, the 35 themes, the cassette-player icon, the installer script and this documentation. The owner decided what to build, tried it and asked for changes; Claude wrote the code, tested it and built it. Anthropic doesn't maintain or endorse the project.

Standing on other people's work: [Slint](https://slint.dev) (interface), [Symphonia](https://github.com/pdeljanov/Symphonia) (decoding), [cpal](https://github.com/RustAudio/cpal) (audio output), [rubato](https://github.com/HEnquist/rubato) (resampling), [redb](https://www.redb.org) (library index), [MusicBrainz](https://musicbrainz.org), the [Cover Art Archive](https://coverartarchive.org) and [LRCLIB](https://lrclib.net) (what the tag editor looks songs up in), Google's Material Design icons, and every crate in [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md).

## Licensing

Wisp Player is free software, released under the **GNU General Public License, version 3** (see [LICENSE](LICENSE)): you may use, study, change and share it, and anything you distribute that is based on it must stay under the same license, with its source. It comes with no warranty.

The GPL is also what keeps this consistent with Slint, the interface toolkit, which is available under the GPL v3, under a royalty-free license that requires an attribution notice, or commercially: Wisp Player uses it under the GPL. The other dependencies are under permissive or weak-copyleft licenses compatible with that (MIT, Apache-2.0, MPL-2.0, Unicode and similar); they are listed in [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md). The cassette-player icon is part of this repository and under the same license. The interface icons are Material Design icons (Apache-2.0).
