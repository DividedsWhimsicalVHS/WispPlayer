//! User preferences: a small `key=value` text file, written atomically (temp file + rename), so
//! a crash can never leave a half-written settings file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where Wisp keeps its library index and settings. `WISP_DATA_DIR` overrides it (tests, portable
/// installs).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("WISP_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| home().join("AppData").join("Local"))
    } else if cfg!(target_os = "macos") {
        home().join("Library").join("Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local").join("share"))
    };
    base.join("Wisp")
}

/// The user's music folder, used as the first library folder.
pub fn default_music_dir() -> Option<PathBuf> {
    let home = if cfg!(windows) { std::env::var_os("USERPROFILE") } else { std::env::var_os("HOME") }?;
    let dir = PathBuf::from(home).join("Music");
    dir.is_dir().then_some(dir)
}

pub struct Settings {
    map: BTreeMap<String, String>,
    path: PathBuf,
    dirty: bool,
}

impl Settings {
    pub fn load(dir: &Path) -> Settings {
        let path = dir.join("settings.txt");
        let mut map = BTreeMap::new();
        if let Ok(text) = std::fs::read_to_string(&path) {
            // Windows editors may save a byte-order mark; it would otherwise glue itself to the first key.
            for line in text.trim_start_matches('\u{feff}').lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    map.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }
        Settings { map, path, dirty: false }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }

    pub fn string(&self, key: &str, default: &str) -> String {
        self.get(key).unwrap_or(default).to_string()
    }

    pub fn f32(&self, key: &str, default: f32) -> f32 {
        self.get(key).and_then(|v| v.parse().ok()).filter(|v: &f32| v.is_finite()).unwrap_or(default)
    }

    pub fn i32(&self, key: &str, default: i32) -> i32 {
        self.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    pub fn bool(&self, key: &str, default: bool) -> bool {
        match self.get(key) {
            Some("1" | "true") => true,
            Some("0" | "false") => false,
            _ => default,
        }
    }

    pub fn set(&mut self, key: &str, value: impl ToString) {
        let value = value.to_string().replace(['\n', '\r'], " ");
        if self.map.get(key) != Some(&value) {
            self.map.insert(key.to_string(), value);
            self.dirty = true;
        }
    }

    /// Library folders. Stored `|`-separated: `|` cannot appear in a Windows path.
    pub fn folders(&self) -> Vec<PathBuf> {
        self.get("folders")
            .map(|v| v.split('|').filter(|s| !s.is_empty()).map(PathBuf::from).collect())
            .unwrap_or_default()
    }

    pub fn set_folders(&mut self, folders: &[PathBuf]) {
        let joined = folders.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join("|");
        self.set("folders", joined);
    }

    /// Writes the file if anything changed.
    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        let mut text = String::from("# Wisp settings\n");
        for (k, v) in &self.map {
            text.push_str(&format!("{k}={v}\n"));
        }
        let tmp = self.path.with_extension("tmp");
        let ok = std::fs::create_dir_all(self.path.parent().unwrap_or(Path::new(".")))
            .and_then(|_| std::fs::write(&tmp, text))
            .and_then(|_| std::fs::rename(&tmp, &self.path))
            .is_ok();
        if ok {
            self.dirty = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wisp-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn round_trips_typed_values_and_folders() {
        let dir = temp("rt");
        let mut s = Settings::load(&dir);
        s.set("volume", 0.65f32);
        s.set("muted", true);
        s.set("device", "wasapi:{abc}");
        s.set_folders(&[PathBuf::from(r"C:\Music"), PathBuf::from(r"D:\Lossless Albums")]);
        s.save();
        let s = Settings::load(&dir);
        assert_eq!(s.f32("volume", 0.0), 0.65);
        assert!(s.bool("muted", false));
        assert_eq!(s.string("device", ""), "wasapi:{abc}");
        assert_eq!(s.folders(), [PathBuf::from(r"C:\Music"), PathBuf::from(r"D:\Lossless Albums")]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_or_garbage_values_fall_back_to_defaults() {
        let dir = temp("garbage");
        std::fs::write(dir.join("settings.txt"), "volume=loud\n\n# comment\nmuted=maybe\nscale=125\nnot a pair\n").unwrap();
        let s = Settings::load(&dir);
        assert_eq!(s.f32("volume", 0.8), 0.8);
        assert!(!s.bool("muted", false));
        assert_eq!(s.i32("scale", 100), 125);
        assert_eq!(s.i32("density", 1), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_byte_order_mark_from_a_text_editor_is_ignored() {
        let dir = temp("bom");
        std::fs::write(dir.join("settings.txt"), "\u{feff}folders=C:\\Music\nvolume=0.3\n").unwrap();
        let s = Settings::load(&dir);
        assert_eq!(s.folders(), [PathBuf::from(r"C:\Music")]);
        assert_eq!(s.f32("volume", 0.8), 0.3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unchanged_settings_do_not_rewrite_the_file() {
        let dir = temp("clean");
        let mut s = Settings::load(&dir);
        s.save();
        assert!(!dir.join("settings.txt").exists(), "nothing to save");
        s.set("a", 1);
        s.save();
        assert!(dir.join("settings.txt").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
