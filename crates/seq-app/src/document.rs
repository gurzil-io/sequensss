//! Where the open project lives: a file path (desktop, and the Android app folder of older
//! versions) or, on Android, a document chosen in the system file picker (Storage Access
//! Framework), kept as its `content://` URI with a persisted read / write permission.

use seq_i18n::tr;
use seq_io::{IoError, project_file};
use seq_model::Project;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectFile {
    Path(PathBuf),
    /// Android document and its display name (shown as the project's file name).
    Uri { uri: String, name: String },
}

impl ProjectFile {
    /// Android document; `name`: its display name, if the provider gave one.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn uri(uri: String, name: Option<String>) -> Self {
        let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| name_from_uri(&uri));
        ProjectFile::Uri { uri, name }
    }

    /// File name shown in the title and the menus.
    pub fn label(&self) -> String {
        match self {
            ProjectFile::Path(p) => p.file_name().map_or_else(|| p.to_string_lossy().to_string(), |f| f.to_string_lossy().to_string()),
            ProjectFile::Uri { name, .. } => name.clone(),
        }
    }

    /// The name without its `.ron` extension (suggested names for Save as… and exports).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn stem(&self) -> String {
        let label = self.label();
        match label.strip_suffix(".ron") {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => label,
        }
    }

    pub fn load(&self) -> seq_io::Result<Project> {
        match self {
            ProjectFile::Path(p) => project_file::load(p),
            ProjectFile::Uri { uri, .. } => project_file::from_text(&read_uri(uri)?),
        }
    }

    pub fn save(&self, project: &Project) -> seq_io::Result<()> {
        match self {
            ProjectFile::Path(p) => project_file::save(project, p),
            ProjectFile::Uri { uri, .. } => write_uri(uri, &project_file::to_text(project)?),
        }
    }
}

#[cfg(target_os = "android")]
fn read_uri(uri: &str) -> seq_io::Result<String> {
    seq_io::platform::read_text(uri).map_err(IoError::Other)
}

#[cfg(target_os = "android")]
fn write_uri(uri: &str, text: &str) -> seq_io::Result<()> {
    seq_io::platform::write_text(uri, text).map_err(IoError::Other)
}

#[cfg(not(target_os = "android"))]
fn read_uri(_uri: &str) -> seq_io::Result<String> {
    Err(no_uris())
}

#[cfg(not(target_os = "android"))]
fn write_uri(_uri: &str, _text: &str) -> seq_io::Result<()> {
    Err(no_uris())
}

#[cfg(not(target_os = "android"))]
fn no_uris() -> IoError {
    IoError::Other(tr!("document Android non disponible ici", "Android document not available here").into())
}

/// Name of a document when its provider gives none: the end of the URI, decoded
/// (`…/document/primary%3ADocuments%2FSong.ron` → `Song.ron`).
pub fn name_from_uri(uri: &str) -> String {
    let last = uri.trim_end_matches('/').rsplit('/').next().unwrap_or(uri);
    let decoded = percent_decode(last);
    let name = decoded.rsplit(['/', ':']).next().unwrap_or(&decoded).trim();
    if name.is_empty() { tr!("Document", "Document").into() } else { name.to_string() }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let hex = |c: u8| (c as char).to_digit(16);
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let (Some(h), Some(l)) = (b.get(i + 1).and_then(|&c| hex(c)), b.get(i + 2).and_then(|&c| hex(c)))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// File name suggested to the picker: `stem.ext`, without the characters file systems refuse.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn suggested_name(stem: &str, ext: &str) -> String {
    let s: String = stem.trim().chars().map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim();
    let s = s.strip_suffix(&format!(".{ext}")).unwrap_or(s).trim();
    let s = if s.is_empty() || s == "." { tr!("projet", "project") } else { s };
    format!("{s}.{ext}")
}

/// Android "Open…": the list of projects kept in the app folder by earlier versions when
/// there are some (with "Open a file…" at the top), otherwise the system picker straight away.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn open_shows_app_list(projects_in_app: usize) -> bool {
    projects_in_app > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_stems() {
        seq_i18n::set(seq_i18n::Lang::En);
        let p = ProjectFile::Path(PathBuf::from("/a/b/My song.ron"));
        assert_eq!((p.label(), p.stem()), ("My song.ron".to_string(), "My song".to_string()));
        let u = ProjectFile::uri("content://x/document/primary%3ADocuments%2FTest.ron".into(), Some("Test.ron".into()));
        assert_eq!((u.label(), u.stem()), ("Test.ron".to_string(), "Test".to_string()));
        // No display name: taken from the URI.
        let u = ProjectFile::uri("content://x/document/primary%3ADocuments%2FBeat%201.ron".into(), None);
        assert_eq!(u.label(), "Beat 1.ron");
        let u = ProjectFile::uri("content://x/document/primary%3ADocuments%2FBeat.ron".into(), Some("  ".into()));
        assert_eq!(u.label(), "Beat.ron");
        // A name without the extension stays as it is.
        let u = ProjectFile::uri("content://x/1".into(), Some("Groove".into()));
        assert_eq!(u.stem(), "Groove");
        assert_eq!(ProjectFile::uri("content://x/1".into(), Some(".ron".into())).stem(), ".ron");
    }

    #[test]
    fn names_from_uris() {
        seq_i18n::set(seq_i18n::Lang::En);
        assert_eq!(name_from_uri("content://com.android.externalstorage.documents/document/primary%3ADocuments%2FTest.ron"), "Test.ron");
        assert_eq!(name_from_uri("content://com.android.providers.downloads.documents/document/msf%3A1000000033"), "1000000033");
        assert_eq!(name_from_uri("content://p/document/caf%C3%A9.ron"), "café.ron");
        // Malformed escapes are kept.
        assert_eq!(name_from_uri("content://p/document/100%25 %zz.ron"), "100% %zz.ron");
        assert_eq!(name_from_uri("content://p/document/a%2"), "a%2");
        assert_eq!(name_from_uri("content://p/document/primary%3A"), "Document");
    }

    #[test]
    fn suggested_names() {
        seq_i18n::set(seq_i18n::Lang::En);
        assert_eq!(suggested_name("Song", "ron"), "Song.ron");
        assert_eq!(suggested_name(" Song.ron ", "ron"), "Song.ron");
        assert_eq!(suggested_name("a/b:c?", "wav"), "a_b_c_.wav");
        assert_eq!(suggested_name("", "mid"), "project.mid");
        assert_eq!(suggested_name(".ron", "ron"), "project.ron");
    }

    #[test]
    fn open_goes_to_the_picker_without_old_projects() {
        assert!(!open_shows_app_list(0));
        assert!(open_shows_app_list(3));
    }

    #[test]
    fn path_files_load_and_save() {
        let dir = std::env::temp_dir().join(format!("sequensss-test-document-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = ProjectFile::Path(dir.join("p.ron"));
        let p = Project::from_kit(1);
        f.save(&p).unwrap();
        assert_eq!(f.load().unwrap(), p);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
