//! Application: wires together the model, UI, engine, audio and MIDI.
//! Shared by the desktop binary (`main.rs`) and the Android library (`seq-android`).

#[cfg(target_os = "android")]
mod android_files;
pub mod files;

pub use eframe;
pub use files::Files;

use eframe::egui;
use rtrb::RingBuffer;
use seq_edit::{History, Recorder, SlotUpdate};
use seq_engine::{Engine, EngineCmd, EngineEvent, EngineHandle, MidiInMsg, MidiOutMsg, PlayMode, compile, offline};
use seq_io::audio::{self, AudioOut};
use seq_io::instruments::{SampleCache, build_slot};
use seq_io::midi::{self, MidiInConn, MidiInTx, MidiOutThread};
use seq_io::{export, project_file};
use seq_i18n::{tr, trf};
use seq_model::*;
use seq_ui::{PlaybackInfo, SeqUi, UiAction};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Sample rate used for export when no device is open.
const EXPORT_SR: u32 = 48_000;

enum Pending {
    New(usize),
    Open,
    /// Open this file (browser or Android import).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    OpenPath(PathBuf),
}

struct App {
    project: Project,
    history: History,
    ui: SeqUi,
    file: Option<PathBuf>,
    saved: Project,

    audio: Option<AudioOut>,
    handle: Option<EngineHandle>,
    sample_rate: u32,
    audio_choice: Option<String>,
    audio_errors_seen: u32,
    cache: SampleCache,

    // State already sent to the engine (diffed every frame).
    sent_project: Option<Project>,
    sent_kinds: HashMap<ChannelId, ChannelKind>,
    sent_mode: Option<PlayMode>,
    sent_loop: Option<Option<(Tick, Tick)>>,
    sent_metronome: Option<bool>,
    sent_recording: Option<bool>,
    sent_live: Option<Option<ChannelId>>,

    midi_in: Option<MidiInConn>,
    midi_in_tx: MidiInTx,
    midi_in_choice: Option<String>,
    midi_out: MidiOutThread,
    /// Last seen MIDI device generation (changes on every hot-plug, Android).
    midi_gen: u64,

    levels: HashMap<ChannelId, (f32, f32)>,
    master: (f32, f32),
    recorder: Recorder,
    confirm: Option<Pending>,
    title: String,
    files: Files,
    /// Android: target channel of a WAV being picked (`Some(None)` = new channel).
    #[cfg(target_os = "android")]
    pending_sample: Option<Option<ChannelId>>,
}

impl App {
    fn new(path: Option<PathBuf>, files: Files) -> Self {
        let mut project = Project::from_kit(0);
        let mut file = None;
        let mut status = String::new();
        if let Some(p) = path {
            match project_file::load(&p) {
                Ok(pr) => {
                    project = pr;
                    file = Some(p);
                }
                Err(e) => status = trf!("Ouverture impossible : {e}", "Cannot open: {e}"),
            }
        }
        let mut ui = SeqUi::new(&project);
        ui.status = status;
        let mut app = App {
            history: History::new(&project, 200),
            saved: project.clone(),
            project,
            ui,
            file,
            audio: None,
            handle: None,
            sample_rate: EXPORT_SR,
            audio_choice: None,
            audio_errors_seen: 0,
            cache: SampleCache::default(),
            sent_project: None,
            sent_kinds: HashMap::new(),
            sent_mode: None,
            sent_loop: None,
            sent_metronome: None,
            sent_recording: None,
            sent_live: None,
            midi_in: None,
            midi_in_tx: Arc::new(Mutex::new(RingBuffer::<MidiInMsg>::new(1).0)),
            midi_in_choice: None,
            midi_out: MidiOutThread::spawn(),
            midi_gen: 0,
            levels: HashMap::new(),
            master: (0.0, 0.0),
            recorder: Recorder::default(),
            confirm: None,
            title: String::new(),
            files,
            #[cfg(target_os = "android")]
            pending_sample: None,
        };
        app.refresh_devices();
        app.start_audio();
        if cfg!(target_os = "android")
            && let Some(a) = &app.audio
        {
            app.ui.status = a.name.clone();
        }
        app.update_file_label();
        app
    }

    // ------------------------------------------------------------------
    // Audio / MIDI
    // ------------------------------------------------------------------

    fn refresh_devices(&mut self) {
        self.ui.devices.audio = audio::list_output_devices();
        self.ui.devices.midi_in = midi::list_in_ports();
        self.ui.devices.midi_out = midi::list_out_ports();
    }

    /// (Re)creates the engine and the audio stream. The engine state is rebuilt from the project.
    fn start_audio(&mut self) {
        self.audio = None;
        self.handle = None;
        let (in_tx, in_rx) = RingBuffer::<MidiInMsg>::new(1024);
        let (out_tx, out_rx) = RingBuffer::<MidiOutMsg>::new(4096);

        match audio::prepare_output(self.audio_choice.as_deref()) {
            Ok(prep) => {
                let sr = prep.sample_rate;
                let (mut engine, handle) = Engine::new(sr, Some(in_rx), Some(out_tx));
                engine.set_midi_out_waker(self.midi_out.waker());
                match audio::start_output(prep, engine) {
                    Ok(a) => {
                        self.sample_rate = a.sample_rate;
                        self.audio_errors_seen = 0;
                        self.audio = Some(a);
                        self.handle = Some(handle);
                    }
                    Err(e) => self.ui.status = trf!("Audio : {e}", "Audio: {e}"),
                }
            }
            Err(e) => self.ui.status = trf!("Audio : {e}", "Audio: {e}"),
        }
        self.midi_out.set_ring(out_rx);

        // Reconnect MIDI in to the new ring.
        if let Some(c) = self.midi_in.take() {
            c.close();
        }
        self.midi_in_tx = Arc::new(Mutex::new(in_tx));
        let choice = self.midi_in_choice.clone();
        self.connect_midi_in(choice);

        self.sent_project = None;
        self.sent_kinds.clear();
        self.sent_mode = None;
        self.sent_loop = None;
        self.sent_metronome = None;
        self.sent_recording = None;
        self.sent_live = None;

        let d = &mut self.ui.devices;
        d.audio_ok = self.audio.is_some();
        d.sample_rate = self.audio.as_ref().map(|a| a.sample_rate);
        d.current_audio = self.audio_choice.clone().or_else(|| self.audio.as_ref().map(|a| a.name.clone()));
    }

    fn connect_midi_in(&mut self, name: Option<String>) {
        if let Some(c) = self.midi_in.take() {
            c.close();
        }
        self.midi_in_choice = None;
        if let Some(name) = name {
            match midi::connect_input(&name, self.midi_in_tx.clone()) {
                Ok(c) => {
                    self.midi_in = Some(c);
                    self.midi_in_choice = Some(name);
                }
                Err(e) => self.ui.status = e.to_string(),
            }
        }
        self.ui.devices.current_in = self.midi_in_choice.clone();
    }

    fn send(&mut self, cmd: EngineCmd) {
        if let Some(h) = self.handle.as_mut() {
            h.send(cmd);
        }
    }

    /// Sends the engine whatever changed since the last frame.
    fn sync_engine(&mut self) {
        let Some(h) = self.handle.as_mut() else { return };

        // Instruments
        for u in seq_edit::slot_updates(&self.sent_kinds, &self.project.channels) {
            let id = match u {
                SlotUpdate::Remove(id) => {
                    h.send(EngineCmd::RemoveSlot(id));
                    self.sent_kinds.remove(&id);
                    continue;
                }
                SlotUpdate::SynthParams(id, p) => {
                    h.send(EngineCmd::SetSynthParams(id, p));
                    id
                }
                SlotUpdate::Rebuild(id) => id,
            };
            let Some(ch) = self.project.channel(id) else { continue };
            if matches!(u, SlotUpdate::Rebuild(_)) {
                match build_slot(ch, self.sample_rate, &mut self.cache) {
                    Ok(kind) => {
                        h.send(EngineCmd::SetSlot(id, kind));
                    }
                    Err(e) => self.ui.status = trf!("{} : {e}", "{}: {e}", ch.name),
                }
            }
            self.sent_kinds.insert(id, ch.kind.clone());
        }

        // Graph
        if self.sent_project.as_ref() != Some(&self.project) {
            h.send(EngineCmd::SwapGraph(Box::new(compile(&self.project))));
            self.sent_project = Some(self.project.clone());
        }

        // Transport
        let mode = if self.ui.song_mode { PlayMode::Song } else { PlayMode::Pattern(self.ui.pattern) };
        if self.sent_mode != Some(mode) {
            h.send(EngineCmd::SetMode(mode));
            self.sent_mode = Some(mode);
            self.recorder.clear();
        }
        if self.sent_loop != Some(self.ui.loop_region) {
            h.send(EngineCmd::SetLoop(self.ui.loop_region));
            self.sent_loop = Some(self.ui.loop_region);
        }
        if self.sent_metronome != Some(self.ui.metronome) {
            h.send(EngineCmd::SetMetronome(self.ui.metronome));
            self.sent_metronome = Some(self.ui.metronome);
        }
        if self.sent_recording != Some(self.ui.recording) {
            h.send(EngineCmd::SetRecording(self.ui.recording));
            self.sent_recording = Some(self.ui.recording);
            self.recorder.clear();
        }
        if self.sent_live != Some(self.ui.channel) {
            h.send(EngineCmd::SetLiveChannel(self.ui.channel));
            // The engine releases the held notes: their note-offs will not be recorded.
            self.recorder.clear();
            self.sent_live = Some(self.ui.channel);
        }
    }

    fn poll_engine(&mut self) {
        let mut recorded = Vec::new();
        if let Some(h) = self.handle.as_mut() {
            while let Some(ev) = h.poll_event() {
                match ev {
                    EngineEvent::Levels { channel, peak_l, peak_r } => {
                        self.levels.insert(channel, (peak_l, peak_r));
                    }
                    EngineEvent::Master { peak_l, peak_r } => self.master = (peak_l, peak_r),
                    EngineEvent::Recorded { channel, key, vel, tick, time, on } => recorded.push((channel, key, vel, tick, time, on)),
                }
            }
            h.collect_garbage();
        }
        for (channel, key, vel, tick, time, on) in recorded {
            self.on_recorded(channel, key, vel, tick, time, on);
        }
        let new_errors = self.audio.as_ref().map(|a| a.error_count()).filter(|&n| n > self.audio_errors_seen);
        if let Some(n) = new_errors {
            self.audio_errors_seen = n;
            if cfg!(target_os = "android") {
                // Headphones plugged/unplugged, Bluetooth…: AAudio closes the stream, so reopen it
                // (the sample rate may change → sample cache cleared).
                self.cache = SampleCache::default();
                self.start_audio();
                if self.audio.is_some() {
                    self.ui.status = tr!("Sortie audio changée : flux rouvert", "Audio output changed: stream reopened").into();
                }
            } else {
                self.ui.status = tr!(
                    "Erreur du device audio (déconnecté ?) — choisis-le à nouveau dans MIDI & Audio",
                    "Audio device error (disconnected?) — select it again in MIDI & Audio"
                )
                .into();
            }
        }
        self.poll_midi_devices();
        // Project MIDI outputs → MIDI out thread (only acts if the config changes).
        self.midi_out.set_ports(&self.project.midi_outs);
        if let Some(e) = self.midi_out.take_error() {
            self.ui.status = e;
        }
        self.ui.devices.open_outs = self.midi_out.open.lock().map(|o| o.clone()).unwrap_or_default();
    }

    /// MIDI device plugged/unplugged (Android; never on desktop, where the generation stays 0):
    /// lists refreshed, outputs reopened, input reconnected — or, on Android, the first
    /// input connected automatically if none is chosen.
    fn poll_midi_devices(&mut self) {
        let generation = midi::devices_generation();
        if generation == self.midi_gen {
            return;
        }
        self.midi_gen = generation;
        self.refresh_devices();
        self.midi_out.reset();
        let available = &self.ui.devices.midi_in;
        let want = self
            .midi_in_choice
            .clone()
            .filter(|n| available.contains(n))
            .or_else(|| if cfg!(target_os = "android") { available.first().cloned() } else { None });
        if want.is_none() && self.midi_in_choice.is_none() {
            return;
        }
        self.connect_midi_in(want);
        if let Some(n) = &self.midi_in_choice {
            self.ui.status = trf!("MIDI in : {n}", "MIDI in: {n}");
        }
    }

    /// Note recorded from MIDI in (Pattern mode only).
    fn on_recorded(&mut self, channel: ChannelId, key: u8, vel: u8, tick: f64, time: f64, on: bool) {
        if self.ui.song_mode || !self.ui.recording {
            return;
        }
        let Some(rec) = self.recorder.event(channel, key, vel, tick, time, on) else { return };
        let quantize = self.ui.quantize;
        let Some(pat) = self.project.pattern_mut(self.ui.pattern) else { return };
        pat.notes.entry(rec.channel).or_default().push(rec.to_note(pat.length, quantize));
    }

    // ------------------------------------------------------------------
    // Files
    // ------------------------------------------------------------------

    fn update_file_label(&mut self) {
        self.ui.file_label = self
            .file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| tr!("Sans titre", "Untitled").into());
    }

    fn set_project(&mut self, p: Project, file: Option<PathBuf>) {
        self.send(EngineCmd::Stop);
        self.project = p;
        self.saved = self.project.clone();
        self.history.reset(&self.project);
        self.file = file;
        self.ui.reset_for(&self.project);
        self.recorder.clear();
        self.levels.clear();
        self.update_file_label();
    }

    fn do_new(&mut self, kit: usize) {
        self.set_project(Project::from_kit(kit), None);
        self.ui.status = trf!("Nouveau projet (kit {})", "New project ({} kit)", KITS[kit].name);
    }

    #[cfg(not(target_os = "android"))]
    fn do_open(&mut self) {
        let Some(path) = self.files.pick_open(tr!("Projet Sequensss", "Sequensss project"), &["ron"], tr!("projet.ron", "project.ron")) else { return };
        self.open_path(path);
    }

    /// Android: opens the project browser.
    #[cfg(target_os = "android")]
    fn do_open(&mut self) {
        self.show_browser();
    }

    fn open_path(&mut self, path: PathBuf) {
        match project_file::load(&path) {
            Ok(p) => {
                self.set_project(p, Some(path));
                self.ui.status = tr!("Projet ouvert", "Project opened").into();
            }
            Err(e) => self.ui.status = trf!("Ouverture impossible : {e}", "Cannot open: {e}"),
        }
    }

    fn do_save(&mut self, save_as: bool) {
        let path = match (&self.file, save_as) {
            (Some(p), false) => p.clone(),
            #[cfg(target_os = "android")]
            _ => return self.ask_save_name(),
            #[cfg(not(target_os = "android"))]
            _ => {
                let Some(p) = self.files.pick_save(tr!("Projet Sequensss", "Sequensss project"), &["ron"], tr!("projet.ron", "project.ron")) else { return };
                if p.extension().is_none() { p.with_extension("ron") } else { p }
            }
        };
        self.save_to(path);
    }

    fn save_to(&mut self, path: PathBuf) {
        match project_file::save(&self.project, &path) {
            Ok(()) => {
                self.saved = self.project.clone();
                self.file = Some(path);
                self.update_file_label();
                self.ui.status = tr!("Enregistré", "Saved").into();
            }
            Err(e) => self.ui.status = trf!("Enregistrement impossible : {e}", "Cannot save: {e}"),
        }
    }

    fn export_mode(&self) -> PlayMode {
        if seq_edit::exports_song(&self.project) { PlayMode::Song } else { PlayMode::Pattern(self.ui.pattern) }
    }

    #[cfg(not(target_os = "android"))]
    fn do_export_wav(&mut self) {
        let Some(path) = self.files.pick_save("WAV", &["wav"], "export.wav") else { return };
        let path = if path.extension().is_none() { path.with_extension("wav") } else { path };
        self.export_wav_to(&path);
    }

    /// Offline render to `path`. Returns `true` if the file was written.
    fn export_wav_to(&mut self, path: &std::path::Path) -> bool {
        let sr = if self.audio.is_some() { self.sample_rate } else { EXPORT_SR };
        let mut slots = Vec::new();
        for ch in &self.project.channels {
            match build_slot(ch, sr, &mut self.cache) {
                Ok(k) => slots.push((ch.id, k)),
                Err(e) => self.ui.status = trf!("{} ignoré : {e}", "{} skipped: {e}", ch.name),
            }
        }
        let mode = self.export_mode();
        let audio = offline::render(compile(&self.project), slots, sr, mode, 2.0);
        match export::write_wav(path, &audio, sr) {
            Ok(()) => {
                let what = if mode == PlayMode::Song { tr!("morceau", "song") } else { tr!("pattern courant", "current pattern") };
                let secs = audio.len() as f64 / 2.0 / sr as f64;
                self.ui.status = trf!("Export WAV ({what}, {secs:.1} s) : {}", "WAV export ({what}, {secs:.1} s): {}", path.display());
                true
            }
            Err(e) => {
                self.ui.status = trf!("Export WAV impossible : {e}", "WAV export failed: {e}");
                false
            }
        }
    }

    #[cfg(not(target_os = "android"))]
    fn do_export_midi(&mut self) {
        let Some(path) = self.files.pick_save("MIDI", &["mid"], "export.mid") else { return };
        let path = if path.extension().is_none() { path.with_extension("mid") } else { path };
        self.export_midi_to(&path);
    }

    fn export_midi_to(&mut self, path: &std::path::Path) -> bool {
        match export::write_midi(path, &self.project, self.ui.pattern) {
            Ok(()) => {
                self.ui.status = trf!("Export MIDI : {}", "MIDI export: {}", path.display());
                true
            }
            Err(e) => {
                self.ui.status = trf!("Export MIDI impossible : {e}", "MIDI export failed: {e}");
                false
            }
        }
    }

    #[cfg(not(target_os = "android"))]
    fn do_load_sample(&mut self, target: Option<ChannelId>) {
        let Some(path) = self.files.pick_open("WAV", &["wav"], "sample.wav") else { return };
        self.load_sample_path(target, path);
    }

    fn load_sample_path(&mut self, target: Option<ChannelId>, path: PathBuf) {
        let source = SampleSource::File(path.clone());
        self.cache.invalidate(&source);
        if let Err(e) = self.cache.get(&source, self.sample_rate) {
            self.ui.status = trf!("Chargement impossible : {e}", "Cannot load: {e}");
            return;
        }
        let kind = ChannelKind::Sampler { source };
        match target {
            Some(id) => {
                if let Some(ch) = self.project.channel_mut(id) {
                    ch.kind = kind;
                }
                // Force a rebuild even if the path is unchanged (file modified).
                self.sent_kinds.remove(&id);
            }
            None => {
                let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("Sample".into());
                match self.project.add_channel(&name, kind) {
                    Some(id) => self.ui.channel = Some(id),
                    None => self.ui.status = trf!("Maximum {MAX_CHANNELS} channels", "Maximum {MAX_CHANNELS} channels"),
                }
            }
        }
    }

    fn is_dirty(&self) -> bool {
        self.project != self.saved
    }

    fn handle_action(&mut self, a: UiAction) {
        match a {
            UiAction::TogglePlay => {
                let playing = self.handle.as_ref().is_some_and(|h| h.shared.playing());
                self.send(if playing { EngineCmd::Pause } else { EngineCmd::Play });
                if self.handle.is_none() {
                    self.ui.status = tr!("Pas de sortie audio — voir Réglages", "No audio output — see Settings").into();
                }
            }
            UiAction::Stop => {
                self.send(EngineCmd::Stop);
                self.recorder.clear();
            }
            UiAction::Seek(t) => self.send(EngineCmd::Seek(t as f64)),
            UiAction::Preview { channel, key, vel, on } => self.send(EngineCmd::Preview { channel, key, vel, on }),
            UiAction::NewProject(k) => {
                if self.is_dirty() { self.confirm = Some(Pending::New(k)) } else { self.do_new(k) }
            }
            UiAction::Open => {
                if self.is_dirty() { self.confirm = Some(Pending::Open) } else { self.do_open() }
            }
            UiAction::Save => self.do_save(false),
            UiAction::SaveAs => self.do_save(true),
            UiAction::ExportWav => self.do_export_wav(),
            UiAction::ExportMidi => self.do_export_midi(),
            UiAction::Undo => {
                if !self.history.undo(&mut self.project) {
                    self.ui.status = tr!("Rien à annuler", "Nothing to undo").into();
                }
            }
            UiAction::Redo => {
                if !self.history.redo(&mut self.project) {
                    self.ui.status = tr!("Rien à rétablir", "Nothing to redo").into();
                }
            }
            UiAction::LoadSample(t) => self.do_load_sample(t),
            UiAction::SelectAudio(name) => {
                self.audio_choice = name;
                self.cache = SampleCache::default();
                self.start_audio();
                if self.audio.is_some() {
                    self.ui.status = tr!("Sortie audio changée", "Audio output changed").into();
                }
            }
            UiAction::SelectMidiIn(p) => self.connect_midi_in(p),
            UiAction::SetLanguage(lang) => {
                seq_i18n::set(lang);
                self.update_file_label(); // "Untitled"
                if let Err(e) = (files::Settings { lang: Some(lang) }).save(&self.files) {
                    self.ui.status = trf!("Réglages non enregistrés : {e}", "Settings not saved: {e}");
                }
            }
            UiAction::RefreshDevices => {
                self.refresh_devices();
                self.ui.status = tr!("Listes rafraîchies", "Lists refreshed").into();
            }
            #[cfg(target_os = "android")]
            a @ (UiAction::BrowserOpen(_)
            | UiAction::BrowserShare(_)
            | UiAction::BrowserDelete(_)
            | UiAction::BrowserSaveAs
            | UiAction::BrowserImport) => self.browser_action(a),
            // Desktop: native dialogs, no browser.
            #[cfg(not(target_os = "android"))]
            UiAction::BrowserOpen(_)
            | UiAction::BrowserShare(_)
            | UiAction::BrowserDelete(_)
            | UiAction::BrowserSaveAs
            | UiAction::BrowserImport => {}
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.confirm else { return };
        let what = match pending {
            Pending::New(_) => tr!("créer un nouveau projet", "create a new project"),
            Pending::Open | Pending::OpenPath(_) => tr!("ouvrir un projet", "open a project"),
        };
        let mut choice = None;
        egui::Window::new(tr!("Modifications non enregistrées", "Unsaved changes"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(trf!(
                    "Le projet a des modifications non enregistrées. Les perdre et {what} ?",
                    "The project has unsaved changes. Discard them and {what}?"
                ));
                ui.horizontal(|ui| {
                    if ui.button(tr!("Enregistrer d'abord", "Save first")).clicked() {
                        choice = Some(0);
                    }
                    if ui.button(tr!("Perdre les modifications", "Discard changes")).clicked() {
                        choice = Some(1);
                    }
                    if ui.button(tr!("Annuler", "Cancel")).clicked() {
                        choice = Some(2);
                    }
                });
            });
        if let Some(c) = choice {
            let pending = self.confirm.take();
            match c {
                0 => {
                    self.do_save(false);
                    if self.is_dirty() {
                        return;
                    }
                }
                2 => return,
                _ => {}
            }
            match pending {
                Some(Pending::New(k)) => self.do_new(k),
                Some(Pending::Open) => self.do_open(),
                Some(Pending::OpenPath(p)) => self.open_path(p),
                None => {}
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_engine();
        #[cfg(target_os = "android")]
        self.poll_platform();

        let (playing, playhead) = match &self.handle {
            Some(h) => (h.shared.playing(), h.shared.playhead()),
            None => (false, 0.0),
        };
        let info = PlaybackInfo { playing, playhead, levels: self.levels.clone(), master: self.master };
        self.ui.can_undo = self.history.can_undo() || self.project != self.saved;
        self.ui.can_redo = self.history.can_redo();
        self.ui.dirty = self.is_dirty();

        let actions = self.ui.show(ctx, &mut self.project, &info);
        for a in actions {
            self.handle_action(a);
        }
        self.confirm_dialog(ctx);

        let gesture = ctx.input(|i| i.pointer.any_down()) || ctx.wants_keyboard_input();
        self.history.commit_if_changed(&self.project, gesture);
        self.sync_engine();

        let title = format!("Sequensss — {}{}", self.ui.file_label, if self.ui.dirty { " *" } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        if playing {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }
}

/// Runs the application. `path`: project to open at startup; `locale`: system language
/// (`fr-FR`, `en`…), used if no language was chosen in the menu.
pub fn run(options: eframe::NativeOptions, path: Option<PathBuf>, files: Files, locale: Option<String>) -> eframe::Result<()> {
    let lang = files::Settings::load(&files)
        .lang
        .or_else(|| locale.as_deref().map(seq_i18n::Lang::from_locale))
        .unwrap_or(seq_i18n::Lang::Fr);
    seq_i18n::set(lang);
    eframe::run_native(
        "Sequensss",
        options,
        Box::new(move |cc| {
            SeqUi::install_theme(&cc.egui_ctx);
            Ok(Box::new(App::new(path, files)))
        }),
    )
}
