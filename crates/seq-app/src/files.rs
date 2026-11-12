//! Files. Desktop: native dialogs (`rfd`).
//! Android: the app directory (`/sdcard/Android/data/<package>/files`) with the subdirectories
//! `projects/`, `exports/`, `samples/`; project browser, Android picker and "Share"
//! (see `android_files.rs`).

use std::path::PathBuf;

pub struct Files {
    #[cfg(target_os = "android")]
    dir: PathBuf,
}

#[cfg(not(target_os = "android"))]
impl Files {
    pub fn desktop() -> Self {
        Files {}
    }

    pub fn pick_open(&self, filter: &str, exts: &[&str], _default_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().add_filter(filter, exts).pick_file()
    }

    pub fn pick_save(&self, filter: &str, exts: &[&str], default_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().add_filter(filter, exts).set_file_name(default_name).save_file()
    }

    /// App settings: `%APPDATA%\Sequensss\settings.txt` (Windows),
    /// `$XDG_CONFIG_HOME` or `~/.config/sequensss/settings.txt` elsewhere.
    pub fn settings_path(&self) -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA")
            .map(|d| PathBuf::from(d).join("Sequensss"))
            .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(|d| PathBuf::from(d).join("sequensss")))
            .or_else(|| std::env::var_os("HOME").map(|d| PathBuf::from(d).join(".config").join("sequensss")))?;
        Some(base.join("settings.txt"))
    }
}

#[cfg(target_os = "android")]
impl Files {
    pub fn android(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Files { dir }
    }

    pub fn settings_path(&self) -> Option<PathBuf> {
        Some(self.dir.join("settings.txt"))
    }

    /// Subdirectory of the app directory (created if needed): "projects", "exports", "samples".
    pub fn subdir(&self, name: &str) -> PathBuf {
        let d = self.dir.join(name);
        let _ = std::fs::create_dir_all(&d);
        d
    }
}

/// Persistent settings (one `key=value` line per setting). Only the language for now.
pub struct Settings {
    pub lang: Option<seq_i18n::Lang>,
}

impl Settings {
    pub fn load(files: &Files) -> Settings {
        let text = files.settings_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let lang = text.lines().find_map(|l| l.strip_prefix("lang=")).and_then(seq_i18n::Lang::from_code);
        Settings { lang }
    }

    pub fn save(&self, files: &Files) -> std::io::Result<()> {
        let Some(path) = files.settings_path() else { return Ok(()) };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut text = String::new();
        if let Some(l) = self.lang {
            text.push_str(&format!("lang={}\n", l.code()));
        }
        std::fs::write(path, text)
    }
}
