//! Files on Android. Projects are saved and opened anywhere with the system file picker
//! (Storage Access Framework): the chosen document stays the project's file, through a
//! persisted permission, so Save writes back to it. Projects kept in the app folder by
//! earlier versions (`projects/*.ron`) stay listed in the project browser. WAV / MIDI
//! exports go where the picker says, or to the "Share" sheet; WAVs imported as samples
//! are copied into `samples/`.

use crate::document::{self, ProjectFile};
use crate::{App, Pending, Saving, close};
use eframe::egui;
use seq_i18n::{tr, trf};
use seq_io::platform::{self, PlatformResult};
use seq_model::ChannelId;
use seq_ui::{Browser, EditTarget, UiAction};
use std::path::PathBuf;

const PROJECTS: &str = "projects";
const EXPORTS: &str = "exports";
const SAMPLES: &str = "samples";

/// Types offered by the picker (projects are plain files with the `.ron` extension).
const PROJECT_MIME: &str = "application/octet-stream";
const WAV_MIME: &str = "audio/x-wav";
const MIDI_MIME: &str = "audio/midi";

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

    /// Base name of the current project (file name or "Project").
    fn current_stem(&self) -> String {
        self.file.as_ref().map_or_else(|| tr!("Projet", "Project").into(), ProjectFile::stem)
    }

    /// Open…: the projects kept in the app folder, if any, otherwise the file picker.
    pub(crate) fn do_open(&mut self) {
        let projects = self.list_projects();
        if document::open_shows_app_list(projects.len()) {
            let current = match &self.file {
                Some(ProjectFile::Path(p)) if p.parent() == Some(self.projects_dir().as_path()) => {
                    p.file_stem().map(|s| s.to_string_lossy().to_string())
                }
                _ => None,
            };
            self.ui.browser = Some(Browser::new(projects, current));
        } else {
            self.pick_project();
        }
    }

    fn refresh_browser(&mut self) {
        let projects = self.list_projects();
        if let Some(b) = &mut self.ui.browser {
            b.projects = projects;
        }
    }

    /// System picker for a project to open where it is (continued in `poll_platform`).
    fn pick_project(&mut self) {
        platform::open_document("open", &["*/*"], true);
    }

    /// Save as…: the system picker chooses the folder and the name (continued in
    /// `poll_platform`).
    pub(crate) fn ask_save_as(&mut self) -> Saving {
        platform::create_document("save_as", &document::suggested_name(&self.current_stem(), "ron"), PROJECT_MIME, true);
        Saving::Picking
    }

    pub(crate) fn do_export_wav(&mut self) {
        platform::create_document("export_wav", &document::suggested_name(&self.current_stem(), "wav"), WAV_MIME, false);
    }

    pub(crate) fn do_export_midi(&mut self) {
        platform::create_document("export_midi", &document::suggested_name(&self.current_stem(), "mid"), MIDI_MIME, false);
    }

    /// Renders an export into `exports/` (the "Share" sheet reads it from there).
    fn export_file(&mut self, ext: &str, shown: Option<&str>) -> Option<PathBuf> {
        let path = self.files.subdir(EXPORTS).join(document::suggested_name(&self.current_stem(), ext));
        let shown = shown.map_or_else(|| path.display().to_string(), str::to_string);
        let ok = if ext == "wav" { self.export_wav_to(&path, &shown) } else { self.export_midi_to(&path, &shown) };
        ok.then_some(path)
    }

    /// Export chosen in the picker: rendered into `exports/`, then copied to the document.
    fn export_to_document(&mut self, ext: &str, uri: &str) {
        let name = platform::display_name(uri).unwrap_or_else(|| document::name_from_uri(uri));
        let Some(path) = self.export_file(ext, Some(&name)) else { return };
        if let Err(e) = platform::copy_to(&path, uri) {
            self.ui.status = trf!("Export impossible vers « {name} » : {e}", "Cannot export to “{name}”: {e}");
        }
        let _ = std::fs::remove_file(&path);
    }

    fn share_export(&mut self, ext: &str) {
        let Some(path) = self.export_file(ext, None) else { return };
        let (mime, title) = if ext == "wav" {
            ("audio/wav", tr!("Partager l'export WAV", "Share WAV export"))
        } else {
            ("audio/midi", tr!("Partager l'export MIDI", "Share MIDI export"))
        };
        platform::share(&path, mime, title);
    }

    pub(crate) fn do_load_sample(&mut self, target: Option<ChannelId>) {
        self.pending_sample = Some(target);
        platform::pick("sample", SAMPLES, &["audio/wav", "audio/x-wav", "audio/wave", "audio/*"]);
    }

    pub(crate) fn browser_action(&mut self, ctx: &egui::Context, a: UiAction) {
        match a {
            UiAction::BrowserOpen(name) => {
                let file = ProjectFile::Path(self.project_path(&name));
                let step = close::request(self.is_dirty());
                self.answer(ctx, Pending::OpenFile(file), step);
            }
            UiAction::BrowserShare(name) => {
                platform::share(&self.project_path(&name), PROJECT_MIME, tr!("Partager le projet", "Share project"));
            }
            UiAction::BrowserDelete(name) => {
                let path = self.project_path(&name);
                match std::fs::remove_file(&path) {
                    Ok(()) => {
                        if self.file == Some(ProjectFile::Path(path)) {
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
            // The browser is only reached through Open…, after the unsaved changes were dealt with.
            UiAction::BrowserPickFile => self.pick_project(),
            UiAction::ShareWav => self.share_export("wav"),
            UiAction::ShareMidi => self.share_export("mid"),
            UiAction::AskName(target, name) => {
                let title = match target {
                    EditTarget::Pattern(_) => tr!("Nom du pattern", "Pattern name"),
                    EditTarget::Channel(_) => tr!("Nom du channel", "Channel name"),
                };
                platform::prompt("name", title, &name, "OK", tr!("Annuler", "Cancel"));
            }
            _ => {}
        }
    }

    /// Results of Android operations (every frame).
    pub(crate) fn poll_platform(&mut self, ctx: &egui::Context) {
        while let Some(PlatformResult { kind, value }) = platform::take_result() {
            let Some(value) = value else {
                match kind.as_str() {
                    "sample" => self.pending_sample = None,
                    // No place chosen: what waited for the save does not happen.
                    "save_as" => {
                        self.after_save = None;
                        if let Some(e) = self.save_failed.take() {
                            self.ui.status = trf!("Non enregistré : {e}", "Not saved: {e}");
                        }
                    }
                    _ => {}
                }
                continue; // cancelled
            };
            match kind.as_str() {
                "back" => self.on_back(ctx),
                "save_as" => {
                    self.save_failed = None;
                    let name = platform::display_name(&value);
                    let _ = self.save_to(ProjectFile::uri(value, name));
                    if let Some(p) = self.after_save.take() {
                        let next = close::after_save(!self.is_dirty(), false);
                        self.answer(ctx, p, next);
                    }
                }
                // Asked through Open…, once the unsaved changes were dealt with (the picker
                // covers the app meanwhile).
                "open" => {
                    let name = platform::display_name(&value);
                    self.ui.browser = None;
                    self.open_file(ProjectFile::uri(value, name));
                }
                "export_wav" => self.export_to_document("wav", &value),
                "export_midi" => self.export_to_document("mid", &value),
                "name" => self.ui.set_edit_name(value),
                "sample" => {
                    let target = self.pending_sample.take().flatten();
                    self.load_sample_path(target, PathBuf::from(value));
                }
                _ => {}
            }
        }
    }
}
