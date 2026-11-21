//! Folder scanning. Incremental (unchanged files are skipped by modification time and size),
//! streaming (one directory in memory at a time) and gentle: it never holds more than a batch of
//! tracks, and it never deletes anything it could not verify is really gone.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::*};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use super::{Library, Track, Upsert, tags};

/// `mp4`, `m4v`, `webm` and `mkv` are video files: their soundtrack is played, the picture is not.
/// `mka` and `weba` are the same containers holding only sound.
const EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "aac", "m4a", "m4b", "mp4", "m4v", "ogg", "oga", "opus", "aif", "aiff", "aifc", "mp2", "webm", "weba", "mkv", "mka",
];
const BATCH: usize = 100;
const MAX_DEPTH: usize = 32;

#[derive(Default)]
pub struct Progress {
    pub running: AtomicBool,
    /// Files looked at so far in the current scan.
    pub files: AtomicU32,
    /// Increases whenever new data was committed (views should reload).
    pub revision: AtomicU32,
    pub cancel: AtomicBool,
    pub error: Mutex<Option<String>>,
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || name == "$RECYCLE.BIN" || name == "System Volume Information"
}

/// What the index remembers of a file to tell whether it changed: modification time in whole
/// seconds, and size.
pub fn stamp(md: &std::fs::Metadata) -> (u64, u64) {
    (md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs()), md.len())
}

struct FileInfo {
    path: PathBuf,
    mtime: u64,
    size: u64,
}

/// Walks `root`, calling `per_dir` with the audio files of each directory. Returns false if the
/// root itself could not be read (e.g. a disconnected drive).
fn walk(root: &Path, cancel: &AtomicBool, per_dir: &mut dyn FnMut(Vec<FileInfo>)) -> bool {
    if std::fs::read_dir(root).is_err() {
        return false;
    }
    // A stack of paths (not open handles): only one directory is open at any moment.
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if cancel.load(Relaxed) {
            return true;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                let hidden = entry.file_name().to_str().is_none_or(skip_dir);
                if !hidden && depth < MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
            } else if kind.is_file() && is_audio(&path) {
                let Ok(md) = entry.metadata() else { continue };
                let (mtime, size) = stamp(&md);
                files.push(FileInfo { path, mtime, size });
            }
        }
        if !files.is_empty() {
            per_dir(files);
        }
    }
    true
}

struct Scan<'a> {
    lib: &'a Library,
    progress: &'a Progress,
    notify: &'a (dyn Fn() + Send),
    seen: Vec<bool>,
    pending: Vec<Upsert>,
    last_notify: Instant,
}

impl Scan<'_> {
    fn mark(&mut self, id: u32) {
        let id = id as usize;
        if id >= self.seen.len() {
            self.seen.resize(id + 1, false);
        }
        self.seen[id] = true;
    }

    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        match self.lib.apply(std::mem::take(&mut self.pending), false) {
            Ok(ids) => {
                for id in ids {
                    self.mark(id);
                }
                self.progress.revision.fetch_add(1, Relaxed);
                if self.last_notify.elapsed() > Duration::from_millis(400) {
                    (self.notify)();
                    self.last_notify = Instant::now();
                }
            }
            Err(e) => *self.progress.error.lock().unwrap() = Some(e),
        }
    }

    fn directory(&mut self, files: Vec<FileInfo>) {
        let paths: Vec<String> = files.iter().map(|f| f.path.to_string_lossy().into_owned()).collect();
        let known = self.lib.known(&paths);
        for ((file, path), known) in files.into_iter().zip(paths).zip(known) {
            self.progress.files.fetch_add(1, Relaxed);
            if self.progress.cancel.load(Relaxed) {
                return;
            }
            if let Some((id, mtime, size)) = known {
                if mtime == file.mtime && size == file.size {
                    self.mark(id);
                    continue;
                }
            }
            // New or changed: read its tags. An unreadable file is left out of the index.
            let Ok(meta) = tags::read(&file.path) else { continue };
            let track = Track {
                id: 0,
                path,
                title: meta.title,
                artist: meta.artist,
                album: meta.album,
                album_artist: meta.album_artist,
                track_no: meta.track_no,
                disc_no: meta.disc_no,
                year: meta.year,
                duration_ms: meta.duration_ms,
                mtime: file.mtime,
                size: file.size,
                flags: meta.flags,
                ..Default::default()
            };
            self.pending.push(Upsert { id: known.map(|k| k.0), track });
            if self.pending.len() >= BATCH {
                self.flush();
            }
        }
    }
}

fn run(lib: &Library, roots: &[PathBuf], progress: &Progress, notify: &(dyn Fn() + Send)) {
    let mut scan = Scan {
        lib,
        progress,
        notify,
        seen: vec![false; lib.next_id() as usize + 1],
        pending: Vec::new(),
        last_notify: Instant::now(),
    };
    let mut unreadable: Vec<String> = Vec::new();
    for root in roots {
        let ok = walk(root, &progress.cancel, &mut |files| scan.directory(files));
        if !ok {
            // Never treat an unavailable folder (unplugged drive) as "all files deleted".
            unreadable.push(root.to_string_lossy().into_owned());
        }
    }
    scan.flush();
    if !progress.cancel.load(Relaxed) {
        let stale = lib.stale_ids(&scan.seen, &unreadable);
        if let Err(e) = lib.remove(&stale) {
            *progress.error.lock().unwrap() = Some(e);
        }
    }
    if let Err(e) = lib.rebuild_albums() {
        *progress.error.lock().unwrap() = Some(e);
    }
    progress.revision.fetch_add(1, Relaxed);
}

/// Starts a background scan. Returns `false` if one is already running.
pub fn spawn_scan(
    lib: Arc<Library>,
    roots: Vec<PathBuf>,
    progress: Arc<Progress>,
    notify: Box<dyn Fn() + Send>,
) -> bool {
    if progress.running.swap(true, AcqRel) {
        return false;
    }
    progress.files.store(0, Relaxed);
    progress.cancel.store(false, Relaxed);
    *progress.error.lock().unwrap() = None;
    let spawned = std::thread::Builder::new().name("wisp-scan".into()).spawn({
        let progress = progress.clone();
        move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&lib, &roots, &progress, &*notify)));
            if result.is_err() {
                *progress.error.lock().unwrap() = Some("The library scan stopped unexpectedly.".into());
            }
            progress.running.store(false, Release);
            notify();
        }
    });
    if spawned.is_err() {
        progress.running.store(false, Release);
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Sort;

    fn wav(frames: usize) -> Vec<u8> {
        let pcm = vec![0u8; frames * 4];
        let mut f = Vec::new();
        f.extend(b"RIFF");
        f.extend((36 + pcm.len() as u32).to_le_bytes());
        f.extend(b"WAVEfmt ");
        f.extend(16u32.to_le_bytes());
        f.extend(1u16.to_le_bytes());
        f.extend(2u16.to_le_bytes());
        f.extend(44_100u32.to_le_bytes());
        f.extend((44_100u32 * 4).to_le_bytes());
        f.extend(4u16.to_le_bytes());
        f.extend(16u16.to_le_bytes());
        f.extend(b"data");
        f.extend((pcm.len() as u32).to_le_bytes());
        f.extend(&pcm);
        f
    }

    fn wait(progress: &Progress) {
        let t = Instant::now();
        while progress.running.load(Acquire) {
            assert!(t.elapsed() < Duration::from_secs(30), "scan did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn scans_incrementally_and_notices_changes_and_deletions() {
        let root = std::env::temp_dir().join(format!("wisp-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Album A")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::write(root.join("Album A").join("one.wav"), wav(44_100)).unwrap();
        std::fs::write(root.join("Album A").join("two.WAV"), wav(22_050)).unwrap();
        std::fs::write(root.join("notes.txt"), "not audio").unwrap();
        std::fs::write(root.join(".hidden").join("secret.wav"), wav(100)).unwrap();
        std::fs::write(root.join("broken.mp3"), vec![0xFFu8; 64]).unwrap();

        let lib = Arc::new(Library::open(&root.join("..").join(format!("wisp-scan-{}.redb", std::process::id()))).unwrap());
        let progress = Arc::new(Progress::default());
        let roots = vec![root.clone()];

        assert!(spawn_scan(lib.clone(), roots.clone(), progress.clone(), Box::new(|| {})));
        wait(&progress);
        assert_eq!(lib.track_count(), 2, "two audio files; hidden dirs, text and unreadable files are ignored");
        let titles: Vec<String> = lib.ordered_ids(Sort::Title).iter().map(|&i| lib.track(i).unwrap().title).collect();
        assert_eq!(titles, ["one", "two"]);
        assert_eq!(lib.track(lib.ordered_ids(Sort::Title)[0]).unwrap().duration_ms, 1000);
        assert_eq!(lib.album_count(), 1);
        let first_id = lib.ordered_ids(Sort::Title)[0];

        // Rescan with nothing changed: same ids, same data.
        assert!(spawn_scan(lib.clone(), roots.clone(), progress.clone(), Box::new(|| {})));
        wait(&progress);
        assert_eq!(lib.track_count(), 2);
        assert_eq!(lib.ordered_ids(Sort::Title)[0], first_id, "ids are stable across rescans");

        // A new file appears, one disappears, one changes.
        std::fs::write(root.join("three.wav"), wav(44_100)).unwrap();
        std::fs::remove_file(root.join("Album A").join("two.WAV")).unwrap();
        std::fs::write(root.join("Album A").join("one.wav"), wav(88_200)).unwrap();
        assert!(spawn_scan(lib.clone(), roots.clone(), progress.clone(), Box::new(|| {})));
        wait(&progress);
        let ids = lib.ordered_ids(Sort::Title);
        let titles: Vec<String> = ids.iter().map(|&i| lib.track(i).unwrap().title).collect();
        assert_eq!(titles, ["one", "three"]);
        assert_eq!(lib.track(first_id).unwrap().duration_ms, 2000, "the changed file was re-read");

        // A root that cannot be read (unplugged drive) must not wipe its tracks.
        let gone = vec![root.join("does-not-exist")];
        assert!(spawn_scan(lib.clone(), gone, progress.clone(), Box::new(|| {})));
        wait(&progress);
        assert_eq!(lib.track_count(), 0, "tracks outside every scanned root are removed...");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn video_files_are_listed_by_their_soundtrack() {
        let root = std::env::temp_dir().join(format!("wisp-scan3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let clip = include_bytes!("../testdata/sine-262hz-with-video.mp4");
        std::fs::write(root.join("Band - Song (Official Video).MP4"), clip).unwrap();
        std::fs::write(root.join("Other.m4v"), clip).unwrap();
        std::fs::write(root.join("Clip.mkv"), include_bytes!("../testdata/sine-262hz-aac-video.mkv")).unwrap();
        let opus = include_bytes!("../testdata/sine-262hz-opus.webm");
        std::fs::write(root.join("Singer - Webm Song.webm"), opus).unwrap();
        std::fs::write(root.join("Weba Song.weba"), opus).unwrap();
        std::fs::write(root.join("Mka Song.mka"), opus).unwrap();
        std::fs::write(root.join("Film.avi"), clip).unwrap(); // a container Wisp does not scan
        std::fs::write(root.join("Broken.mp4"), vec![0u8; 64]).unwrap(); // no soundtrack to read
        std::fs::write(root.join("Broken.webm"), vec![0u8; 64]).unwrap();
        let db = root.join("..").join(format!("wisp-scan3-{}.redb", std::process::id()));
        let lib = Arc::new(Library::open(&db).unwrap());
        let progress = Arc::new(Progress::default());
        assert!(spawn_scan(lib.clone(), vec![root.clone()], progress.clone(), Box::new(|| {})));
        wait(&progress);
        let titles: Vec<String> = lib.ordered_ids(Sort::Title).iter().map(|&i| lib.track(i).unwrap().title).collect();
        // Listed under their file names, as they are, and all of them flagged: nothing is guessed.
        assert_eq!(titles, ["Band - Song (Official Video)", "Clip", "Mka Song", "Other", "Singer - Webm Song", "Weba Song"]);
        let song = lib.track(lib.ordered_ids(Sort::Title)[0]).unwrap();
        assert_eq!(song.artist, "Unknown Artist");
        assert_eq!(song.flags, crate::library::flag::NO_ARTIST | crate::library::flag::BAD_TITLE | crate::library::flag::NO_COVER);
        assert!((900..1200).contains(&song.duration_ms), "{} ms", song.duration_ms);
        assert_eq!(lib.flagged_count(), 6);
        drop(lib);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn an_unreadable_root_is_not_treated_as_deleted() {
        let root = std::env::temp_dir().join(format!("wisp-scan2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.wav"), wav(44_100)).unwrap();
        let db = std::env::temp_dir().join(format!("wisp-scan2-{}.redb", std::process::id()));
        let lib = Arc::new(Library::open(&db).unwrap());
        let progress = Arc::new(Progress::default());
        assert!(spawn_scan(lib.clone(), vec![root.clone()], progress.clone(), Box::new(|| {})));
        wait(&progress);
        assert_eq!(lib.track_count(), 1);

        // The drive "disappears": scanning the same root now fails to read it.
        let moved = root.with_extension("offline");
        std::fs::rename(&root, &moved).unwrap();
        assert!(spawn_scan(lib.clone(), vec![root.clone()], progress.clone(), Box::new(|| {})));
        wait(&progress);
        assert_eq!(lib.track_count(), 1, "an unavailable folder keeps its tracks");
        let _ = std::fs::remove_dir_all(&moved);
        let _ = std::fs::remove_file(&db);
    }
}
