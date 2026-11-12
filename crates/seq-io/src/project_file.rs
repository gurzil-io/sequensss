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
}
