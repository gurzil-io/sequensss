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

/// Persistent settings (one `key=value` line per setting). Lines with unknown keys are kept
/// as they are, so a file written by a newer version survives a save.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub lang: Option<seq_i18n::Lang>,
    /// Check GitHub for a newer release at startup (on by default).
    pub check_updates: bool,
    /// Last successful update check (Unix seconds).
    pub last_update_check: Option<u64>,
    /// Version the user chose to skip (`x.y.z`).
    pub skipped_version: Option<String>,
    /// Latest release found by the last check (version, page URL): the notice comes back at
    /// every launch, without a request, until the app is updated or the version skipped.
    pub latest_release: Option<(String, String)>,
    other: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            lang: None,
            check_updates: true,
            last_update_check: None,
            skipped_version: None,
            latest_release: None,
            other: Vec::new(),
        }
    }
}

impl Settings {
    pub fn load(files: &Files) -> Settings {
        let text = files.settings_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        Settings::parse(&text)
    }

    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        let (mut latest_version, mut latest_url) = (None, None);
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let (key, value) = line.split_once('=').map(|(k, v)| (k.trim(), v.trim())).unwrap_or((line.trim(), ""));
            match key {
                "lang" => s.lang = seq_i18n::Lang::from_code(value),
                "check_updates" => s.check_updates = value != "0",
                "last_update_check" => s.last_update_check = value.parse().ok(),
                "skipped_version" => s.skipped_version = (!value.is_empty()).then(|| value.to_string()),
                "latest_version" => latest_version = (!value.is_empty()).then(|| value.to_string()),
                "latest_url" => latest_url = (!value.is_empty()).then(|| value.to_string()),
                _ => s.other.push(line.to_string()),
            }
        }
        s.latest_release = latest_version.zip(latest_url);
        s
    }

    pub fn to_text(&self) -> String {
        let mut text = String::new();
        if let Some(l) = self.lang {
            text.push_str(&format!("lang={}\n", l.code()));
        }
        text.push_str(&format!("check_updates={}\n", u8::from(self.check_updates)));
        if let Some(t) = self.last_update_check {
            text.push_str(&format!("last_update_check={t}\n"));
        }
        if let Some(v) = &self.skipped_version {
            text.push_str(&format!("skipped_version={v}\n"));
        }
        if let Some((v, url)) = &self.latest_release {
            text.push_str(&format!("latest_version={v}\nlatest_url={url}\n"));
        }
        for l in &self.other {
            text.push_str(l);
            text.push('\n');
        }
        text
    }

    pub fn save(&self, files: &Files) -> std::io::Result<()> {
        let Some(path) = files.settings_path() else { return Ok(()) };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seq_i18n::Lang;

    #[test]
    fn old_file_with_only_lang_loads() {
        let s = Settings::parse("lang=en\n");
        assert_eq!(s.lang, Some(Lang::En));
        assert!(s.check_updates);
        assert_eq!(s.last_update_check, None);
        assert_eq!(s.skipped_version, None);
        assert_eq!(s.latest_release, None);
    }

    #[test]
    fn round_trip_keeps_every_key() {
        let s = Settings {
            lang: Some(Lang::Fr),
            check_updates: false,
            last_update_check: Some(1_700_000_000),
            skipped_version: Some("0.2.0".into()),
            latest_release: Some(("0.2.1".into(), "https://github.com/gurzil-io/sequensss/releases/tag/v0.2.1".into())),
            other: Vec::new(),
        };
        assert_eq!(Settings::parse(&s.to_text()), s);
    }

    #[test]
    fn unknown_keys_survive_a_save() {
        let mut s = Settings::parse("future_key=42\nlang=fr\n# comment\n");
        s.lang = Some(Lang::En);
        s.check_updates = false;
        let text = s.to_text();
        assert!(text.contains("future_key=42\n"));
        assert!(text.contains("# comment\n"));
        let back = Settings::parse(&text);
        assert_eq!(back.lang, Some(Lang::En));
        assert!(!back.check_updates);
        assert_eq!(back.to_text(), text);
    }

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(Settings::parse(""), Settings::default());
    }
}
