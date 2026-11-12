//! Files on Android: project browser (`projects/*.ron`), name entered in a native dialog,
//! WAV/MIDI exports sent to the "Share" sheet, WAVs and projects imported with the
//! Android picker (copied into `samples/` or `projects/`).

use crate::{App, Pending};
use seq_i18n::{tr, trf};
use seq_io::platform::{self, PlatformResult};
use seq_model::ChannelId;
use seq_ui::{Browser, UiAction};
use std::path::PathBuf;

const PROJECTS: &str = "projects";
const EXPORTS: &str = "exports";
const SAMPLES: &str = "samples";

/// Safe file name from an entered name.
fn sanitize(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim_end_matches(".ron").trim().to_string();
    if s.is_empty() { tr!("projet", "project").into() } else { s }
}

impl App {
    fn projects_dir(&self) -> PathBuf {
        self.files.subdir(PROJECTS)
    }

    fn project_path(&self, name: &str) -> PathBuf {
        self.projects_dir().join(format!("{name}.ron"))
    }

    fn list_projects(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.projects_dir())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "ron"))
                    .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by_key(|s| s.to_lowercase());
        v
    }

    /// Base name of the current project (file name or "Untitled").
    fn current_stem(&self) -> String {
        self.file
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| tr!("Projet", "Project").into())
    }

    pub(crate) fn show_browser(&mut self) {
        self.ui.browser = Some(Browser::new(self.list_projects()));
    }

    fn refresh_browser(&mut self) {
        if self.ui.browser.is_some() {
            self.show_browser();
        }
    }

    /// Save as: native input dialog; continued in `poll_platform`.
    pub(crate) fn ask_save_name(&mut self) {
        platform::prompt(
            "save_as",
            tr!("Nom du projet", "Project name"),
            &self.current_stem(),
            tr!("Enregistrer", "Save"),
            tr!("Annuler", "Cancel"),
        );
    }

    pub(crate) fn do_export_wav(&mut self) {
        let path = self.files.subdir(EXPORTS).join(format!("{}.wav", self.current_stem()));
        if self.export_wav_to(&path) {
            platform::share(&path, "audio/wav", tr!("Partager l'export WAV", "Share WAV export"));
        }
    }

    pub(crate) fn do_export_midi(&mut self) {
        let path = self.files.subdir(EXPORTS).join(format!("{}.mid", self.current_stem()));
        if self.export_midi_to(&path) {
            platform::share(&path, "audio/midi", tr!("Partager l'export MIDI", "Share MIDI export"));
        }
    }

    pub(crate) fn do_load_sample(&mut self, target: Option<ChannelId>) {
        self.pending_sample = Some(target);
        platform::pick("sample", SAMPLES, &["audio/wav", "audio/x-wav", "audio/wave", "audio/*"]);
    }

    /// Opens `path`, after confirmation if the current project has unsaved changes.
    fn open_with_confirm(&mut self, path: PathBuf) {
        if self.is_dirty() {
            self.confirm = Some(Pending::OpenPath(path));
        } else {
            self.open_path(path);
        }
    }

    pub(crate) fn browser_action(&mut self, a: UiAction) {
        match a {
            UiAction::BrowserOpen(name) => {
                let path = self.project_path(&name);
                self.open_with_confirm(path);
            }
            UiAction::BrowserShare(name) => {
                platform::share(&self.project_path(&name), "application/octet-stream", tr!("Partager le projet", "Share project"));
            }
            UiAction::BrowserDelete(name) => {
                let path = self.project_path(&name);
                match std::fs::remove_file(&path) {
                    Ok(()) => {
                        if self.file.as_ref() == Some(&path) {
                            // The open project no longer has a file: it becomes "untitled" again.
                            self.file = None;
                            self.saved = seq_model::Project::empty();
                            self.update_file_label();
                        }
                        self.ui.status = trf!("Projet supprimé : {name}", "Project deleted: {name}");
                    }
                    Err(e) => self.ui.status = trf!("Suppression impossible : {e}", "Cannot delete: {e}"),
                }
                self.refresh_browser();
            }
            UiAction::BrowserSaveAs => self.ask_save_name(),
            UiAction::BrowserImport => platform::pick("project", PROJECTS, &["application/octet-stream", "*/*"]),
            _ => {}
        }
    }

    /// Results of Android operations (every frame).
    pub(crate) fn poll_platform(&mut self) {
        while let Some(PlatformResult { kind, value }) = platform::take_result() {
            let Some(value) = value else {
                if kind == "sample" {
                    self.pending_sample = None;
                }
                continue; // cancelled
            };
            match kind.as_str() {
                "save_as" => {
                    let path = self.project_path(&sanitize(&value));
                    self.save_to(path);
                    self.refresh_browser();
                }
                "sample" => {
                    let target = self.pending_sample.take().flatten();
                    self.load_sample_path(target, PathBuf::from(value));
                }
                "project" => {
                    let path = PathBuf::from(value);
                    if path.extension().is_some_and(|x| x == "ron") {
                        self.ui.browser = None;
                        self.open_with_confirm(path);
                    } else {
                        let _ = std::fs::remove_file(&path);
                        self.ui.status = tr!("Ce n'est pas un projet Sequensss (.ron)", "Not a Sequensss project (.ron)").into();
                    }
                    self.refresh_browser();
                }
                _ => {}
            }
        }
    }
}
