//! Playlists: one `.m3u8` file per playlist in the data folder.
//!
//! Plain M3U8 (an `#EXTM3U` header, the name in `#PLAYLIST:`, one absolute path per line) keeps
//! playlists readable by other players and independent of the library index, so a rebuilt index
//! loses nothing. Entries whose file is not in the library are kept in the file and simply not
//! shown, so a drive that is unplugged today still has its songs in the playlist tomorrow.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Playlist {
    pub name: String,
    pub file: PathBuf,
}

pub struct Playlists {
    dir: PathBuf,
}

/// A file name for a playlist name: no characters Windows forbids, no reserved device names.
fn file_stem_for(name: &str) -> String {
    let mut s: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    s = s.trim().trim_end_matches('.').chars().take(100).collect();
    let upper = s.to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&upper.as_str())
        || (upper.len() == 4 && (upper.starts_with("COM") || upper.starts_with("LPT")) && upper.as_bytes()[3].is_ascii_digit());
    if s.is_empty() || reserved { format!("{s}_playlist") } else { s }
}

fn clean_name(name: &str) -> Result<String, String> {
    let n: String = name.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string();
    if n.is_empty() { Err("A playlist needs a name.".into()) } else { Ok(n.chars().take(120).collect()) }
}

/// `(name, paths)` of an M3U8 file. Relative entries are resolved against the file's folder.
fn read(file: &Path) -> Option<(String, Vec<String>)> {
    let text = std::fs::read_to_string(file).ok()?;
    let dir = file.parent().unwrap_or(Path::new(""));
    let mut name = file.file_stem()?.to_string_lossy().into_owned();
    let mut paths = Vec::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if let Some(n) = line.strip_prefix("#PLAYLIST:") {
            if !n.trim().is_empty() {
                name = n.trim().to_string();
            }
        } else if !line.is_empty() && !line.starts_with('#') {
            // Playlists from other players use either slash.
            let entry = if cfg!(windows) { line.replace('/', "\\") } else { line.to_string() };
            let p = Path::new(&entry);
            paths.push(if p.is_absolute() { entry.clone() } else { dir.join(p).to_string_lossy().into_owned() });
        }
    }
    Some((name, paths))
}

/// Written next to the target and renamed over it, so a crash never leaves half a playlist.
fn write(file: &Path, name: &str, paths: &[String]) -> Result<(), String> {
    let mut text = format!("#EXTM3U\n#PLAYLIST:{name}\n");
    for p in paths {
        text.push_str(p);
        text.push('\n');
    }
    let tmp = file.with_extension("m3u8.tmp");
    std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, file)).map_err(|e| format!("Couldn't save the playlist: {e}"))
}

impl Playlists {
    pub fn new(dir: PathBuf) -> Playlists {
        Playlists { dir }
    }

    /// Every playlist, sorted by name (ignoring case and accents).
    pub fn list(&self) -> Vec<Playlist> {
        let mut out: Vec<Playlist> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("m3u8") || e.eq_ignore_ascii_case("m3u")))
            .filter_map(|file| read(&file).map(|(name, _)| Playlist { name, file }))
            .collect();
        out.sort_by_cached_key(|p| crate::library::fold(&p.name));
        out
    }

    pub fn paths(&self, p: &Playlist) -> Vec<String> {
        read(&p.file).map(|(_, paths)| paths).unwrap_or_default()
    }

    fn name_taken(&self, name: &str, except: Option<&Path>) -> bool {
        let folded = crate::library::fold(name);
        self.list().iter().any(|p| crate::library::fold(&p.name) == folded && Some(p.file.as_path()) != except)
    }

    /// A file for `name` that no other playlist uses.
    fn free_file(&self, name: &str) -> PathBuf {
        let stem = file_stem_for(name);
        (1..)
            .map(|n| self.dir.join(if n == 1 { format!("{stem}.m3u8") } else { format!("{stem} ({n}).m3u8") }))
            .find(|f| !f.exists())
            .expect("an unused file name")
    }

    pub fn create(&self, name: &str) -> Result<Playlist, String> {
        let name = clean_name(name)?;
        if self.name_taken(&name, None) {
            return Err(format!("There is already a playlist called \"{name}\"."));
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("Couldn't create the playlists folder: {e}"))?;
        let file = self.free_file(&name);
        write(&file, &name, &[])?;
        Ok(Playlist { name, file })
    }

    pub fn rename(&self, p: &Playlist, new_name: &str) -> Result<Playlist, String> {
        let name = clean_name(new_name)?;
        if self.name_taken(&name, Some(&p.file)) {
            return Err(format!("There is already a playlist called \"{name}\"."));
        }
        let paths = self.paths(p);
        let file = if file_stem_for(&name) == file_stem_for(&p.name) { p.file.clone() } else { self.free_file(&name) };
        write(&file, &name, &paths)?;
        if file != p.file {
            let _ = std::fs::remove_file(&p.file);
        }
        Ok(Playlist { name, file })
    }

    pub fn delete(&self, p: &Playlist) -> Result<(), String> {
        std::fs::remove_file(&p.file).map_err(|e| format!("Couldn't delete the playlist: {e}"))
    }

    /// Appends the songs that are not in the playlist yet. Returns how many were added.
    pub fn add(&self, p: &Playlist, paths: &[String]) -> Result<usize, String> {
        let mut all = self.paths(p);
        let before = all.len();
        for path in paths {
            if !all.contains(path) {
                all.push(path.clone());
            }
        }
        let added = all.len() - before;
        if added > 0 {
            write(&p.file, &p.name, &all)?;
        }
        Ok(added)
    }

    pub fn remove(&self, p: &Playlist, path: &str) -> Result<(), String> {
        let mut all = self.paths(p);
        all.retain(|x| x != path);
        write(&p.file, &p.name, &all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wisp-pl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// A song's path the way the platform writes an absolute one (`C:\...` or `/...`).
    fn abs(name: &str) -> String {
        std::env::temp_dir().join("wisp-pl-songs").join(name).to_string_lossy().into_owned()
    }

    #[test]
    fn create_add_remove_rename_delete() {
        let dir = temp("crud");
        let pls = Playlists::new(dir.clone());
        assert!(pls.list().is_empty(), "no folder yet is simply no playlists");
        let p = pls.create("Road Trip").unwrap();
        assert!(pls.create("road trip").is_err(), "names are unique ignoring case");
        assert!(pls.create("   ").is_err());
        assert_eq!(pls.add(&p, &[abs("a.mp3"), abs("b.flac")]).unwrap(), 2);
        assert_eq!(pls.add(&p, &[abs("a.mp3"), abs("c.mp3")]).unwrap(), 1, "already-present songs are skipped");
        assert_eq!(pls.paths(&p), [abs("a.mp3"), abs("b.flac"), abs("c.mp3")]);
        pls.remove(&p, &abs("b.flac")).unwrap();
        assert_eq!(pls.paths(&p), [abs("a.mp3"), abs("c.mp3")]);

        let p = pls.rename(&p, "Summer: Road/Trip?").unwrap();
        assert_eq!(p.name, "Summer: Road/Trip?", "the name keeps every character...");
        assert_eq!(p.file.file_name().unwrap(), "Summer_ Road_Trip_.m3u8", "...the file name stays legal");
        assert_eq!(pls.paths(&p), [abs("a.mp3"), abs("c.mp3")], "songs survive a rename");
        assert_eq!(pls.list(), [p.clone()]);

        pls.create("Another").unwrap();
        assert_eq!(pls.list().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Another", "Summer: Road/Trip?"]);
        pls.delete(&p).unwrap();
        assert_eq!(pls.list().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn foreign_m3u_files_are_read() {
        let dir = temp("foreign");
        let pls = Playlists::new(dir.clone());
        // A playlist from another player: extended info lines, a BOM, a relative path.
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Imported.m3u"), "\u{feff}#EXTM3U\n#EXTINF:123,Artist - Song\nsongs/x.mp3\n").unwrap();
        let imported = pls.list().into_iter().find(|p| p.name == "Imported").unwrap();
        assert_eq!(pls.paths(&imported), [dir.join("songs").join("x.mp3").to_string_lossy().into_owned()]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reserved_and_empty_file_names_are_avoided() {
        assert_eq!(file_stem_for("CON"), "CON_playlist");
        assert_eq!(file_stem_for("com1"), "com1_playlist");
        assert_eq!(file_stem_for("..."), "_playlist");
        assert_eq!(file_stem_for("Chill"), "Chill");
    }
}
