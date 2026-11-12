//! `.ron` save / load.

use crate::{IoError, Result};
use seq_i18n::trf;
use seq_model::Project;
use std::path::Path;

pub fn save(project: &Project, path: &Path) -> Result<()> {
    let s = ron::ser::to_string_pretty(project, ron::ser::PrettyConfig::default())
        .map_err(|e| IoError::Ron(e.to_string()))?;
    // Atomic write: temporary file, then rename.
    let tmp = path.with_extension("ron.tmp");
    std::fs::write(&tmp, s)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Project> {
    let s = std::fs::read_to_string(path)?;
    let p: Project = ron::from_str(&s).map_err(|e| IoError::Ron(e.to_string()))?;
    if p.version > seq_model::FORMAT_VERSION {
        return Err(IoError::Other(trf!("format v{} trop récent", "format v{} too recent", p.version)));
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_round_trip() {
        let dir = std::env::temp_dir().join("sequensss-test-ron");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.ron");
        for kit in 0..seq_model::KITS.len() {
            let mut p = Project::from_kit(kit);
            p.arrangement.clips.push(seq_model::Clip { pattern: p.patterns[0].id, track: 2, start: 3840, len: 7680 });
            p.swing = 0.33;
            save(&p, &path).unwrap();
            assert_eq!(load(&path).unwrap(), p);
        }
    }

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sequensss-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn load_missing_file_is_file_error() {
        let path = tmp_dir("ron-missing").join("nope.ron");
        match load(&path) {
            Err(IoError::File(e)) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn load_corrupt_file_is_ron_error() {
        let dir = tmp_dir("ron-corrupt");
        let path = dir.join("bad.ron");
        std::fs::write(&path, "(version: 1, tempo_bpm: \"fast\"").unwrap();
        assert!(matches!(load(&path), Err(IoError::Ron(_))));
        // Truncated valid file.
        let full = ron::ser::to_string_pretty(&Project::from_kit(0), ron::ser::PrettyConfig::default()).unwrap();
        std::fs::write(&path, &full[..full.len() / 2]).unwrap();
        assert!(matches!(load(&path), Err(IoError::Ron(_))));
        // Binary garbage (invalid UTF-8).
        std::fs::write(&path, [0xffu8, 0xfe, 0x00, 0x81]).unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn load_rejects_newer_format() {
        let path = tmp_dir("ron-newer").join("p.ron");
        let mut p = Project::from_kit(1);
        p.version = seq_model::FORMAT_VERSION + 1;
        save(&p, &path).unwrap();
        assert!(matches!(load(&path), Err(IoError::Other(_))));
    }

    #[test]
    fn save_to_missing_directory_fails_cleanly() {
        let dir = tmp_dir("ron-nodir").join("does").join("not").join("exist");
        let path = dir.join("p.ron");
        assert!(matches!(save(&Project::from_kit(0), &path), Err(IoError::File(_))));
        assert!(!dir.exists());
    }

    #[test]
    fn save_is_atomic_and_leaves_no_tmp() {
        let dir = tmp_dir("ron-atomic");
        let path = dir.join("song.ron");
        save(&Project::from_kit(0), &path).unwrap();
        // Overwrite an existing file.
        let p = Project::from_kit(3);
        save(&p, &path).unwrap();
        assert_eq!(load(&path).unwrap(), p);
        let names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(names, vec!["song.ron".to_string()]);

        // A path without an extension works too.
        let bare = dir.join("bare");
        save(&p, &bare).unwrap();
        assert_eq!(load(&bare).unwrap(), p);
        assert!(!dir.join("bare.ron.tmp").exists());
        assert!(!dir.join("song.ron.tmp").exists());
    }
}
