//! Copy of the unsaved work, offered back at the next launch. Written when the app goes to the
//! background (Android, where it can be killed without warning) or quits without being able to
//! ask (desktop: Quit from the macOS menu or the Dock). Removed once the work is saved or
//! discarded.
//!
//! A project file whose first line, a RON comment, says when it was written and which file
//! the project belongs to.

use seq_i18n::{tr, trf};
use seq_io::project_file;
use seq_model::Project;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "recovery.ron";
const HEADER: &str = "// sequensss-recovery ";

/// Unsaved work found at launch.
#[derive(Debug, Clone, PartialEq)]
pub struct Recovery {
    pub project: Project,
    /// The project's own file (`None`: untitled).
    pub file: Option<PathBuf>,
    /// When the copy was written (Unix seconds).
    pub saved_at: u64,
}

/// Writes the copy (atomically: an interrupted write leaves the previous one).
pub fn write(path: &Path, project: &Project, file: Option<&Path>, saved_at: u64) -> seq_io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let meta = serde_json::json!({ "saved_at": saved_at, "file": file.map(|f| f.to_string_lossy()) });
    let text = format!("{HEADER}{meta}\n{}", project_file::to_text(project)?);
    project_file::write_atomic(path, &text)
}

/// The copy, if there is a readable one.
pub fn read(path: &Path) -> Option<Recovery> {
    let text = std::fs::read_to_string(path).ok()?;
    let project = project_file::from_text(&text).ok()?;
    let meta = text
        .lines()
        .next()
        .and_then(|l| l.strip_prefix(HEADER))
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .unwrap_or_default();
    Some(Recovery {
        project,
        file: meta["file"].as_str().map(PathBuf::from),
        saved_at: meta["saved_at"].as_u64().unwrap_or(0),
    })
}

/// Removes the copy (nothing to do if there is none).
pub fn remove(path: &Path) {
    if let Err(e) = std::fs::remove_file(path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("recovery: {e}");
    }
}

/// How long ago `then` was, for the restore question ("5 min ago").
pub fn ago(now: u64, then: u64) -> String {
    let s = now.saturating_sub(then);
    match s {
        0..60 => tr!("à l'instant", "just now").into(),
        60..3600 => trf!("il y a {} min", "{} min ago", s / 60),
        3600..86_400 => trf!("il y a {} h", "{} h ago", s / 3600),
        _ => match s / 86_400 {
            1 => tr!("hier", "yesterday").into(),
            d => trf!("il y a {d} jours", "{d} days ago"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sequensss-test-recovery-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn round_trip_keeps_the_project_its_file_and_the_time() {
        let dir = tmp_dir("round-trip");
        let path = dir.join(FILE_NAME);
        let mut p = Project::from_kit(1);
        p.tempo_bpm = 97.0;
        let file = dir.join("my \"song\"\nv2.ron");
        // Creates the directory.
        write(&path, &p, Some(&file), 1_700_000_000).unwrap();
        assert_eq!(read(&path), Some(Recovery { project: p.clone(), file: Some(file), saved_at: 1_700_000_000 }));
        // Still an ordinary project file.
        assert_eq!(project_file::load(&path).unwrap(), p);

        // Overwritten by the next copy (untitled project), without a temporary file left.
        write(&path, &Project::from_kit(0), None, 1_700_000_100).unwrap();
        let r = read(&path).unwrap();
        assert_eq!((r.project, r.file, r.saved_at), (Project::from_kit(0), None, 1_700_000_100));
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from(FILE_NAME)]);

        remove(&path);
        assert!(!path.exists());
        assert_eq!(read(&path), None);
        // Removing twice is fine.
        remove(&path);
    }

    #[test]
    fn unreadable_copy_is_ignored() {
        let dir = tmp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, "// sequensss-recovery {\"saved_at\": 5}\n(version: 1, tempo").unwrap();
        assert_eq!(read(&path), None);
        // A plain project file without the header line: untitled, time unknown.
        project_file::save(&Project::from_kit(2), &path).unwrap();
        assert_eq!(read(&path), Some(Recovery { project: Project::from_kit(2), file: None, saved_at: 0 }));
    }

    #[test]
    fn ago_reads_naturally() {
        seq_i18n::set(seq_i18n::Lang::En);
        let t = 1_000_000;
        assert_eq!(ago(t + 30, t), "just now");
        assert_eq!(ago(t - 30, t), "just now");
        assert_eq!(ago(t + 5 * 60 + 10, t), "5 min ago");
        assert_eq!(ago(t + 3 * 3600, t), "3 h ago");
        assert_eq!(ago(t + 86_400 + 5, t), "yesterday");
        assert_eq!(ago(t + 3 * 86_400, t), "3 days ago");
    }
}
