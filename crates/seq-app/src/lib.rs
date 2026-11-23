//! Application: wires together the model, UI, engine, audio and MIDI.
//! Shared by the desktop binary (`main.rs`) and the Android library (`seq-android`).

#[cfg(target_os = "android")]
mod android_files;
mod close;
mod document;
pub mod files;
mod recovery;
mod update;

pub use eframe;
pub use files::Files;

use document::ProjectFile;
use eframe::egui;
use rtrb::RingBuffer;
use seq_edit::{History, PatternTake, Recorder, SlotUpdate};
use seq_engine::{Compiler, Engine, EngineCmd, EngineEvent, EngineHandle, MidiInMsg, MidiOutMsg, PlayMode, compile, offline};
use seq_io::audio::{self, AudioOut};
use seq_io::instruments::{build_slot, sampler_slot};
use seq_io::loader::SampleLoader;
use seq_io::midi::{self, MidiInConn, MidiInTx, MidiOutThread};
use seq_io::{export, project_file, samples};
use seq_i18n::{tr, trf};
use seq_model::*;
use seq_ui::{ButtonStyle, PlaybackInfo, SeqUi, Splash, UiAction};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Sample rate used for export when no device is open.
const EXPORT_SR: u32 = 48_000;
/// Snake hiss of the launch splash (source and conversion: `assets/audio/README.md`).
const HISS: &[u8] = include_bytes!("../assets/splash-hiss.wav");
/// Hiss level (-6 dB): under the default kit.
const HISS_GAIN: f32 = 0.5;

/// Where the splash hiss is at.
#[derive(Clone, Copy, PartialEq)]
enum Hiss {
    Waiting,
    Playing,
    Over,
}

/// Recording take in progress (see `App::recording_take`).
struct Take {
    /// Pattern mode: the pattern recorded into (`None` in Song mode).
    pattern: Option<PatternTake>,
    /// Song mode: a note fell outside every clip (said once per take).
    dropped: bool,
}

/// What waits for an answer about the unsaved changes.
enum Pending {
    New(usize),
    Open,
    /// Open this file (Android: a project of the app folder).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    OpenFile(ProjectFile),
    /// Close the window (desktop) or leave the app (Android back button).
    Quit,
}

/// Unsaved work shared with the Android main thread, which copies it when the app goes to
/// the background (`None`: nothing to keep).
#[cfg(target_os = "android")]
type Unsaved = Arc<Mutex<Option<(Project, Option<ProjectFile>)>>>;

struct App {
    project: Project,
    history: History,
    ui: SeqUi,
    /// Launch splash, dropped once it has faded out.
    splash: Option<Splash>,
    hiss: Hiss,
    file: Option<ProjectFile>,
    saved: Project,

    audio: Option<AudioOut>,
    handle: Option<EngineHandle>,
    sample_rate: u32,
    audio_choice: Option<String>,
    audio_errors_seen: u32,
    /// Background sample loading (and the sample cache).
    loader: SampleLoader,

    // State already sent to the engine (diffed every frame: cheap, the snapshots
    // share their unchanged channels and patterns).
    sent_project: Option<Project>,
    /// Recompiles only the patterns that changed.
    compiler: Compiler,
    sent_kinds: HashMap<ChannelId, ChannelKind>,
    sent_mode: Option<PlayMode>,
    sent_loop: Option<Option<(Tick, Tick)>>,
    sent_metronome: Option<bool>,
    sent_recording: Option<bool>,
    sent_count_in: Option<bool>,
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
    take: Option<Take>,
    confirm: Option<Pending>,
    /// Android: waits for the file picker of a project being saved before going ahead.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    after_save: Option<Pending>,
    /// Unsaved work found at launch, offered back.
    restore: Option<recovery::Recovery>,
    /// Desktop: the window may close (nothing to save, or the user answered).
    closing: bool,
    #[cfg(target_os = "android")]
    unsaved: Unsaved,
    title: String,
    files: Files,
    settings: files::Settings,
    /// Update check in progress (see `update.rs`).
    update_check: Option<Receiver<Result<Option<update::Release>, String>>>,
    /// Android: target channel of a WAV being picked (`Some(None)` = new channel).
    #[cfg(target_os = "android")]
    pending_sample: Option<Option<ChannelId>>,
    /// Android: why Save could not write to the project's document (Save as… took over).
    #[cfg(target_os = "android")]
    save_failed: Option<String>,
}

/// Result of a save request.
#[derive(Clone, Copy, PartialEq)]
enum Saving {
    /// Written, or failed (see the status line).
    Done,
    /// Android: the file picker is choosing where (finished in `poll_platform`).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    Picking,
}

impl App {
    fn new(path: Option<PathBuf>, files: Files, settings: files::Settings) -> Self {
        let mut project = Project::from_kit(0);
        let mut file = None;
        let mut status = String::new();
        if let Some(p) = path {
            match project_file::load(&p) {
                Ok(pr) => {
                    project = pr;
                    file = Some(ProjectFile::Path(p));
                }
                Err(e) => status = trf!("Ouverture impossible : {e}", "Cannot open: {e}"),
            }
        }
        let mut ui = SeqUi::new(&project);
        ui.status = status;
        ui.check_updates = settings.check_updates;
        let mut app = App {
            history: History::new(&project, 200),
            saved: project.clone(),
            project,
            ui,
            splash: Some(Splash::new(update::CURRENT)),
            hiss: Hiss::Waiting,
            file,
            audio: None,
            handle: None,
            sample_rate: EXPORT_SR,
            audio_choice: None,
            audio_errors_seen: 0,
            loader: SampleLoader::new(2),
            sent_project: None,
            compiler: Compiler::default(),
            sent_kinds: HashMap::new(),
            sent_mode: None,
            sent_loop: None,
            sent_metronome: None,
            sent_recording: None,
            sent_count_in: None,
            sent_live: None,
            midi_in: None,
            midi_in_tx: Arc::new(Mutex::new(RingBuffer::<MidiInMsg>::new(1).0)),
            midi_in_choice: None,
            midi_out: MidiOutThread::spawn(),
            midi_gen: 0,
            levels: HashMap::new(),
            master: (0.0, 0.0),
            recorder: Recorder::default(),
            take: None,
            confirm: None,
            after_save: None,
            restore: None,
            closing: false,
            #[cfg(target_os = "android")]
            unsaved: Arc::new(Mutex::new(None)),
            title: String::new(),
            files,
            settings,
            update_check: None,
            #[cfg(target_os = "android")]
            pending_sample: None,
            #[cfg(target_os = "android")]
            save_failed: None,
        };
        // Android: the soft keyboard cannot type into egui, text goes through a system dialog.
        app.ui.native_text_input = cfg!(target_os = "android");
        // Android: exports can also go to the "Share" sheet.
        app.ui.share_exports = cfg!(target_os = "android");
        app.show_known_update();
        app.start_update_check();
        app.refresh_devices();
        app.start_audio();
        if cfg!(target_os = "android")
            && let Some(a) = &app.audio
        {
            app.ui.status = a.name.clone();
        }
        app.update_file_label();
        app.find_recovery();
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
        // Every instrument is rebuilt below (`sent_kinds` cleared), at the new sample rate.
        self.loader.cancel_all();

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
        self.sent_count_in = None;
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

    /// Splash hiss: starts with the eyes as soon as the engine is up (never once the splash
    /// is gone), fades out when the splash is skipped or over.
    fn splash_hiss(&mut self, hissing: bool) {
        match self.hiss {
            Hiss::Waiting if hissing && self.handle.is_some() => {
                self.hiss = Hiss::Over;
                match seq_io::samples::decode_wav(HISS) {
                    Ok(s) => {
                        let sample = Box::new(s.resampled(self.sample_rate));
                        self.send(EngineCmd::PlaySound { sample, gain: HISS_GAIN });
                        self.hiss = Hiss::Playing;
                    }
                    Err(e) => eprintln!("splash hiss: {e}"),
                }
            }
            Hiss::Playing if !hissing => {
                self.send(EngineCmd::StopSound);
                self.hiss = Hiss::Over;
            }
            _ => {}
        }
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
                    self.loader.cancel(id);
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
                match &ch.kind {
                    // WAV file not in the cache: decoded in the background (see `poll_samples`);
                    // the channel keeps its previous instrument until then.
                    ChannelKind::Sampler { source: source @ SampleSource::File(_) } if self.loader.cache().peek(source, self.sample_rate).is_none() => {
                        self.loader.request(id, source.clone(), self.sample_rate);
                    }
                    _ => {
                        self.loader.cancel(id);
                        match build_slot(ch, self.sample_rate, self.loader.cache()) {
                            Ok(kind) => {
                                h.send(EngineCmd::SetSlot(id, kind));
                            }
                            Err(e) => self.ui.status = trf!("{} : {e}", "{}: {e}", ch.name),
                        }
                    }
                }
            }
            self.sent_kinds.insert(id, ch.kind.clone());
        }

        // Graph
        if self.sent_project.as_ref() != Some(&self.project) {
            h.send(EngineCmd::SwapGraph(Box::new(self.compiler.compile(&self.project))));
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
        if self.sent_count_in != Some(self.ui.count_in) {
            h.send(EngineCmd::SetCountIn(self.ui.count_in));
            self.sent_count_in = Some(self.ui.count_in);
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

    /// Samples loaded in the background → engine. Results that no longer match the channel
    /// (source or sample rate changed meanwhile) are dropped.
    fn poll_samples(&mut self) {
        while let Some(l) = self.loader.poll() {
            let Some(ch) = self.project.channel(l.channel) else { continue };
            let current = matches!(&ch.kind, ChannelKind::Sampler { source } if *source == l.source);
            if !current || l.sample_rate != self.sample_rate {
                continue;
            }
            match l.result {
                Ok(s) => {
                    if let Some(h) = self.handle.as_mut() {
                        h.send(EngineCmd::SetSlot(l.channel, sampler_slot(s)));
                    }
                }
                Err(e) => self.ui.status = trf!("{} : {e}", "{}: {e}", ch.name),
            }
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
                self.loader.cache().clear();
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

    /// Note recorded from MIDI in: into the current pattern (Pattern mode), or into the clip
    /// under the playhead (Song mode, `tick` = song position).
    fn on_recorded(&mut self, channel: ChannelId, key: u8, vel: u8, tick: f64, time: f64, on: bool) {
        if !self.ui.recording {
            return;
        }
        let Some(rec) = self.recorder.event(channel, key, vel, tick, time, on) else { return };
        let grid = self.ui.record_grid();
        self.sync_take(true);
        let Some(take) = self.take.as_mut() else { return };
        if let Some(pt) = take.pattern.as_mut() {
            pt.add_note(&mut self.project, rec.channel, rec.to_note(grid));
        } else if !seq_edit::add_song_recorded_note(&mut self.project, self.ui.pattern, &rec, grid) && !take.dropped {
            take.dropped = true;
            self.ui.status = tr!(
                "Enregistrement : aucun clip sous la tête de lecture, notes ignorées",
                "Recording: no clip under the playhead, notes dropped"
            )
            .into();
        }
    }

    /// Recording take: the whole take is one undo step. In Pattern mode the engine does not
    /// loop (the pattern grows).
    fn recording_take(&self, playing: bool) -> bool {
        playing && self.ui.recording
    }

    /// Starts or ends the take (`active`: see `recording_take`). A switch of pattern or mode
    /// ends it and starts another one. At the end of a Pattern mode take, the bars added past
    /// the last recorded note go.
    fn sync_take(&mut self, active: bool) {
        let want = (!self.ui.song_mode).then_some(self.ui.pattern);
        if self.take.as_ref().is_some_and(|t| !active || t.pattern.map(|p| p.pattern) != want)
            && let Some(pt) = self.take.take().and_then(|t| t.pattern)
        {
            pt.finish(&mut self.project);
        }
        if active && self.take.is_none() {
            self.take = Some(Take { pattern: want.map(|p| PatternTake::start(&self.project, p)), dropped: false });
        }
    }

    // ------------------------------------------------------------------
    // Settings, update check
    // ------------------------------------------------------------------

    fn save_settings(&mut self) {
        if let Err(e) = self.settings.save(&self.files) {
            self.ui.status = trf!("Réglages non enregistrés : {e}", "Settings not saved: {e}");
        }
    }

    /// Shows the notice for the latest release found by an earlier check, if it still applies
    /// (no request: the check itself runs at most once a day).
    fn show_known_update(&mut self) {
        let s = &self.settings;
        self.ui.update = s
            .latest_release
            .as_ref()
            .filter(|(v, _)| s.check_updates && update::should_notify(v, update::CURRENT, s.skipped_version.as_deref()))
            .map(|(v, url)| seq_ui::UpdateNotice { version: v.clone(), url: url.clone() });
    }

    /// Starts the background check if it is on and due (at most once a day).
    fn start_update_check(&mut self) {
        let s = &self.settings;
        if self.update_check.is_none() && update::should_check(s.check_updates, s.last_update_check, update::unix_now()) {
            self.update_check = Some(update::spawn_check());
        }
    }

    /// Result of the check, if it arrived. Only a successful answer counts as the day's check:
    /// after a failure, the next launch tries again.
    fn poll_update_check(&mut self) {
        let Some(rx) = &self.update_check else { return };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("check thread stopped".into()),
        };
        self.update_check = None;
        match result {
            Ok(release) => {
                self.settings.last_update_check = Some(update::unix_now());
                self.settings.latest_release = release.map(|r| (r.version, r.url));
                if let Err(e) = self.settings.save(&self.files) {
                    update::debug_log(&e.to_string());
                }
                self.show_known_update();
            }
            Err(e) => update::debug_log(&e),
        }
    }

    // ------------------------------------------------------------------
    // Unsaved work (see `recovery.rs`)
    // ------------------------------------------------------------------

    /// Unsaved work left by the previous run: offered back once the splash is gone.
    fn find_recovery(&mut self) {
        let Some(path) = self.files.recovery_path() else { return };
        self.restore = recovery::read(&path);
        if self.restore.is_none() {
            recovery::remove(&path);
            self.keep_unsaved_in_background();
        }
    }

    /// Android: from now on, the unsaved work is copied when the app goes to the background
    /// (not while the previous copy is still offered back: it would be removed).
    fn keep_unsaved_in_background(&self) {
        #[cfg(target_os = "android")]
        if let Some(path) = self.files.recovery_path() {
            let unsaved = self.unsaved.clone();
            seq_io::platform::on_background(move || {
                let Ok(u) = unsaved.lock() else { return };
                match &*u {
                    Some((project, file)) => {
                        if let Err(e) = recovery::write(&path, project, file.as_ref(), update::unix_now()) {
                            eprintln!("recovery: {e}");
                        }
                    }
                    None => recovery::remove(&path),
                }
            });
        }
    }

    /// Android: shares the unsaved work with the main thread (see `Unsaved`).
    #[cfg(target_os = "android")]
    fn share_unsaved(&mut self) {
        let dirty = self.is_dirty();
        let Ok(mut u) = self.unsaved.lock() else { return };
        let same = match &*u {
            Some((p, f)) => dirty && *p == self.project && *f == self.file,
            None => !dirty,
        };
        if !same {
            *u = dirty.then(|| (self.project.clone(), self.file.clone()));
        }
    }

    fn remove_recovery(&self) {
        if let Some(path) = self.files.recovery_path() {
            recovery::remove(&path);
        }
    }

    /// "Restore unsaved work?" at launch.
    fn restore_dialog(&mut self, ctx: &egui::Context) {
        let Some(r) = &self.restore else { return };
        let name = r.file.as_ref().map_or_else(|| tr!("Sans titre", "Untitled").into(), ProjectFile::label);
        let when = recovery::ago(update::unix_now(), r.saved_at);
        let text = trf!(
            "Des modifications de « {name} » n'ont pas été enregistrées (copie gardée {when}). Les restaurer ?",
            "Changes to “{name}” were not saved (copy kept {when}). Restore them?"
        );
        let buttons = [(tr!("Restaurer", "Restore"), ButtonStyle::Main), (tr!("Abandonner", "Discard"), ButtonStyle::Danger)];
        let answer = seq_ui::question(ctx, "restore_dialog", tr!("TRAVAIL NON ENREGISTRÉ", "UNSAVED WORK"), &text, &buttons, None);
        match answer {
            Some(0) => {
                if let Some(r) = self.restore.take() {
                    // Compared with its file (untitled: the project it replaces, which
                    // "Don't save" goes back to), the restored work shows as unsaved.
                    let saved = r.file.as_ref().and_then(|f| f.load().ok()).unwrap_or_else(|| self.saved.clone());
                    self.set_project(r.project, r.file);
                    self.saved = saved;
                    self.ui.status = tr!("Travail restauré", "Work restored").into();
                }
            }
            Some(_) => {
                self.restore = None;
                self.remove_recovery();
            }
            None => return,
        }
        self.keep_unsaved_in_background();
    }

    // ------------------------------------------------------------------
    // Files
    // ------------------------------------------------------------------

    fn update_file_label(&mut self) {
        self.ui.file_label = self.file.as_ref().map_or_else(|| tr!("Sans titre", "Untitled").into(), ProjectFile::label);
    }

    fn set_project(&mut self, p: Project, file: Option<ProjectFile>) {
        self.send(EngineCmd::Stop);
        self.project = p;
        self.saved = self.project.clone();
        self.history.reset(&self.project);
        self.file = file;
        self.ui.reset_for(&self.project);
        self.recorder.clear();
        // The take was on the previous project.
        self.take = None;
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
        self.open_file(ProjectFile::Path(path));
    }

    fn open_file(&mut self, file: ProjectFile) {
        match file.load() {
            Ok(p) => {
                self.set_project(p, Some(file));
                self.ui.status = tr!("Projet ouvert", "Project opened").into();
            }
            Err(e) => self.ui.status = trf!("Ouverture impossible : {e}", "Cannot open: {e}"),
        }
    }

    /// Save (to the project's file) or Save as…. Android: when the document can no longer be
    /// written (permission revoked, file deleted), Save as… takes over.
    fn do_save(&mut self, save_as: bool) -> Saving {
        match (self.file.clone(), save_as) {
            (Some(f), false) => {
                #[cfg(target_os = "android")]
                if let Err(e) = self.save_to(f) {
                    self.ui.status = trf!("Enregistrement impossible : {e} — choisissez où enregistrer", "Cannot save: {e} — choose where to save");
                    // Said again if the picker is cancelled (it hides the status line).
                    self.save_failed = Some(e.to_string());
                    return self.ask_save_as();
                }
                #[cfg(not(target_os = "android"))]
                let _ = self.save_to(f);
                Saving::Done
            }
            _ => self.ask_save_as(),
        }
    }

    #[cfg(not(target_os = "android"))]
    fn ask_save_as(&mut self) -> Saving {
        if let Some(p) = self.files.pick_save(tr!("Projet Sequensss", "Sequensss project"), &["ron"], tr!("projet.ron", "project.ron")) {
            let p = if p.extension().is_none() { p.with_extension("ron") } else { p };
            let _ = self.save_to(ProjectFile::Path(p));
        }
        Saving::Done
    }

    /// Writes the project to `file`, which becomes its file.
    fn save_to(&mut self, file: ProjectFile) -> seq_io::Result<()> {
        match file.save(&self.project) {
            Ok(()) => {
                self.saved = self.project.clone();
                self.file = Some(file);
                self.update_file_label();
                self.remove_recovery();
                self.ui.status = tr!("Enregistré", "Saved").into();
                Ok(())
            }
            Err(e) => {
                self.ui.status = trf!("Enregistrement impossible : {e}", "Cannot save: {e}");
                Err(e)
            }
        }
    }

    fn export_mode(&self) -> PlayMode {
        if seq_edit::exports_song(&self.project) { PlayMode::Song } else { PlayMode::Pattern(self.ui.pattern) }
    }

    #[cfg(not(target_os = "android"))]
    fn do_export_wav(&mut self) {
        let Some(path) = self.files.pick_save("WAV", &["wav"], "export.wav") else { return };
        let path = if path.extension().is_none() { path.with_extension("wav") } else { path };
        self.export_wav_to(&path, &path.display().to_string());
    }

    /// Offline render to `path` (`shown`: where it goes, for the status line). Returns `true`
    /// if the file was written.
    fn export_wav_to(&mut self, path: &std::path::Path, shown: &str) -> bool {
        let sr = if self.audio.is_some() { self.sample_rate } else { EXPORT_SR };
        // Synchronous: every sample is needed (waits for the ones loading in the background).
        let mut slots = Vec::new();
        for ch in &self.project.channels {
            match build_slot(ch, sr, self.loader.cache()) {
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
                self.ui.status = trf!("Export WAV ({what}, {secs:.1} s) : {shown}", "WAV export ({what}, {secs:.1} s): {shown}");
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
        self.export_midi_to(&path, &path.display().to_string());
    }

    fn export_midi_to(&mut self, path: &std::path::Path, shown: &str) -> bool {
        match export::write_midi(path, &self.project, self.ui.pattern) {
            Ok(()) => {
                self.ui.status = trf!("Export MIDI : {shown}", "MIDI export: {shown}");
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
        // Only the header is checked here; the file is decoded in the background (`sync_engine`).
        if let Err(e) = samples::probe_wav(&path) {
            self.ui.status = trf!("Chargement impossible : {e}", "Cannot load: {e}");
            return;
        }
        let source = SampleSource::File(path);
        self.loader.cache().invalidate(&source);
        let kind = ChannelKind::Sampler { source };
        match target {
            Some(id) => {
                seq_edit::replace_kind(&mut self.project, id, kind);
                // Force a rebuild even if the path is unchanged (file modified).
                self.sent_kinds.remove(&id);
            }
            None => {
                match self.project.add_channel(&kind.default_name(), kind) {
                    Some(id) => self.ui.channel = Some(id),
                    None => self.ui.status = trf!("Maximum {MAX_CHANNELS} channels", "Maximum {MAX_CHANNELS} channels"),
                }
            }
        }
    }

    fn is_dirty(&self) -> bool {
        self.project != self.saved
    }

    fn handle_action(&mut self, ctx: &egui::Context, a: UiAction) {
        match a {
            UiAction::TogglePlay => {
                let playing = self.handle.as_ref().is_some_and(|h| h.shared.playing() || h.shared.count_in() > 0);
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
            UiAction::NewProject(k) => self.answer(ctx, Pending::New(k), close::request(self.is_dirty())),
            UiAction::Open => self.answer(ctx, Pending::Open, close::request(self.is_dirty())),
            UiAction::Save => {
                self.do_save(false);
            }
            UiAction::SaveAs => {
                self.do_save(true);
            }
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
                self.loader.cache().clear();
                self.start_audio();
                if self.audio.is_some() {
                    self.ui.status = tr!("Sortie audio changée", "Audio output changed").into();
                }
            }
            UiAction::SelectMidiIn(p) => self.connect_midi_in(p),
            UiAction::SetLanguage(lang) => {
                seq_i18n::set(lang);
                self.update_file_label(); // "Untitled"
                self.settings.lang = Some(lang);
                self.save_settings();
            }
            UiAction::SetTheme(t) => {
                seq_ui::theme::set(t);
                self.settings.theme = Some(t);
                self.save_settings();
            }
            UiAction::SetCheckUpdates(on) => {
                self.settings.check_updates = on;
                self.ui.check_updates = on;
                self.save_settings();
                self.show_known_update();
                self.start_update_check();
            }
            UiAction::SkipVersion(v) => {
                self.settings.skipped_version = Some(v);
                self.save_settings();
            }
            UiAction::OpenUrl(url) => open_url(ctx, &url),
            UiAction::RefreshDevices => {
                self.refresh_devices();
                self.ui.status = tr!("Listes rafraîchies", "Lists refreshed").into();
            }
            #[cfg(target_os = "android")]
            a @ (UiAction::BrowserOpen(_)
            | UiAction::BrowserShare(_)
            | UiAction::BrowserDelete(_)
            | UiAction::BrowserPickFile
            | UiAction::ShareWav
            | UiAction::ShareMidi
            | UiAction::AskName(..)) => self.browser_action(ctx, a),
            // Desktop: native dialogs, no browser nor share sheet; text is typed in the app.
            #[cfg(not(target_os = "android"))]
            UiAction::BrowserOpen(_)
            | UiAction::BrowserShare(_)
            | UiAction::BrowserDelete(_)
            | UiAction::BrowserPickFile
            | UiAction::ShareWav
            | UiAction::ShareMidi
            | UiAction::AskName(..) => {}
        }
    }

    /// "Save changes?" before `pending` (call every frame).
    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.confirm else { return };
        let name = &self.ui.file_label;
        let text = match pending {
            Pending::New(_) => trf!("Enregistrer les modifications de « {name} » avant de créer un nouveau projet ?", "Save changes to “{name}” before creating a new project?"),
            Pending::Open | Pending::OpenFile(_) => trf!("Enregistrer les modifications de « {name} » avant d'ouvrir un projet ?", "Save changes to “{name}” before opening a project?"),
            Pending::Quit => trf!("Enregistrer les modifications de « {name} » ?", "Save changes to “{name}”?"),
        };
        let buttons = [
            (tr!("Enregistrer", "Save"), ButtonStyle::Main),
            (tr!("Annuler", "Cancel"), ButtonStyle::Plain),
            (tr!("Ne pas enregistrer", "Don't save"), ButtonStyle::Danger),
        ];
        let title = tr!("MODIFICATIONS NON ENREGISTRÉES", "UNSAVED CHANGES");
        let answer = match seq_ui::question(ctx, "confirm_dialog", title, &text, &buttons, Some(1)) {
            Some(0) => close::Answer::Save,
            Some(2) => close::Answer::DontSave,
            Some(_) => close::Answer::Cancel,
            None => return,
        };
        let Some(pending) = self.confirm.take() else { return };
        self.answer(ctx, pending, close::answer(answer));
    }

    /// Carries on with `pending` according to `step`.
    fn answer(&mut self, ctx: &egui::Context, pending: Pending, step: close::Step) {
        match step {
            close::Step::Ask => self.confirm = Some(pending),
            close::Step::Proceed => self.proceed(ctx, pending),
            close::Step::Discard => {
                self.remove_recovery();
                if matches!(pending, Pending::Quit) && cfg!(target_os = "android") {
                    // The app stays in memory: back to the saved state (one undo step).
                    self.project = self.saved.clone();
                }
                self.proceed(ctx, pending);
            }
            close::Step::Save => {
                // Android, untitled: the file picker answers later (see `poll_platform`).
                let picking = self.do_save(false) == Saving::Picking;
                let next = close::after_save(!self.is_dirty(), picking);
                self.answer(ctx, pending, next);
            }
            close::Step::WaitForPicker => self.after_save = Some(pending),
            close::Step::Stay => {}
        }
    }

    fn proceed(&mut self, ctx: &egui::Context, pending: Pending) {
        match pending {
            Pending::New(k) => self.do_new(k),
            Pending::Open => self.do_open(),
            Pending::OpenFile(f) => self.open_file(f),
            Pending::Quit => self.quit(ctx),
        }
    }

    /// Desktop: closes the window. Android: leaves the app (it stays in memory).
    fn quit(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "android")]
        {
            let _ = ctx;
            seq_io::platform::leave();
        }
        #[cfg(not(target_os = "android"))]
        {
            self.closing = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Desktop: the window is asked to close (close button, Cmd+Q, Alt+F4).
    #[cfg(not(target_os = "android"))]
    fn on_close_request(&mut self, ctx: &egui::Context) {
        if self.closing || !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        match close::request(self.is_dirty()) {
            close::Step::Proceed => self.closing = true,
            step => {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.answer(ctx, Pending::Quit, step);
            }
        }
    }

    /// Android back button: closes the topmost dialog, menu or expanded panel; otherwise
    /// leaves the app, after asking about unsaved changes.
    #[cfg(target_os = "android")]
    pub(crate) fn on_back(&mut self, ctx: &egui::Context) {
        if self.restore.is_some() || self.splash.is_some() {
            return; // needs an answer / about to go
        }
        if self.confirm.take().is_some() || self.ui.close_overlay(ctx) {
            return;
        }
        let step = close::request(self.is_dirty());
        self.answer(ctx, Pending::Quit, step);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_engine();
        self.poll_update_check();
        #[cfg(target_os = "android")]
        self.poll_platform(ctx);
        #[cfg(not(target_os = "android"))]
        self.on_close_request(ctx);

        let (playing, playhead, pattern, count_in) = match &self.handle {
            Some(h) => {
                let pattern = h.shared.pattern();
                (h.shared.playing(), h.shared.playhead(), pattern, h.shared.count_in())
            }
            None => (false, 0.0, None, 0),
        };
        // The count-in shows as playing (Play pauses, i.e. cancels it).
        let running = playing || count_in > 0;
        // Before the commit below: the trim at the end of a take is part of its undo step.
        self.sync_take(self.recording_take(running));
        // Recording past the end of the pattern: it grows bar by bar as the playhead goes
        // (trimmed to the last recorded note at the end of the take).
        if let Some(pt) = self.take.as_ref().and_then(|t| t.pattern)
            && playing
            && pattern == Some(pt.pattern)
        {
            pt.grow_to_position(&mut self.project, playhead);
        }
        let info = PlaybackInfo { playing: running, playhead, count_in, levels: self.levels.clone(), master: self.master };
        self.ui.loading = self.loader.loading().collect();
        self.ui.can_undo = self.history.can_undo() || self.project != self.saved;
        self.ui.can_redo = self.history.can_redo();
        self.ui.dirty = self.is_dirty();

        // Before the UI: the tap or key that skips the splash must not reach it.
        if let Some(splash) = &mut self.splash {
            let up = splash.show(ctx);
            let hissing = splash.hissing();
            if !up {
                self.splash = None;
            }
            self.splash_hiss(hissing);
        }
        let actions = self.ui.show(ctx, &mut self.project, &info);
        for a in actions {
            self.handle_action(ctx, a);
        }
        self.confirm_dialog(ctx);
        if self.splash.is_none() {
            self.restore_dialog(ctx);
        }

        // A gesture lasts while any finger is down (not only the one egui maps to the pointer);
        // a recording take is committed when playback stops.
        let gesture = ctx.input(|i| i.pointer.any_down() || i.any_touches())
            || ctx.wants_keyboard_input()
            || self.recording_take(running);
        self.history.commit_if_changed(&self.project, gesture);
        #[cfg(target_os = "android")]
        self.share_unsaved();
        self.sync_engine();
        self.poll_samples();

        let title = format!("Sequensss — {}{}", self.ui.file_label, if self.ui.dirty { " *" } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        if running {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }

    /// Desktop: quitting without the close request (Quit from the macOS menu or the Dock)
    /// cannot be stopped; the unsaved work is kept for the next launch.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let Some(path) = self.files.recovery_path() else { return };
        if self.restore.is_some() {
            return; // still offered at the next launch
        }
        if self.is_dirty() && !self.closing {
            if let Err(e) = recovery::write(&path, &self.project, self.file.as_ref(), update::unix_now()) {
                eprintln!("recovery: {e}");
            }
        } else {
            recovery::remove(&path);
        }
    }
}

/// Opens a web page in the system browser (Android: through `PlatformBridge`, egui's
/// `open_url` does nothing under GameActivity).
fn open_url(ctx: &egui::Context, url: &str) {
    #[cfg(target_os = "android")]
    {
        let _ = ctx;
        seq_io::platform::open_url(url);
    }
    #[cfg(not(target_os = "android"))]
    ctx.open_url(egui::OpenUrl::new_tab(url));
}

/// Runs the application. `path`: project to open at startup; `locale`: system language
/// (`fr-FR`, `en`…), used if no language was chosen in the menu.
pub fn run(options: eframe::NativeOptions, path: Option<PathBuf>, files: Files, locale: Option<String>) -> eframe::Result<()> {
    let settings = files::Settings::load(&files);
    let lang = settings
        .lang
        .or_else(|| locale.as_deref().map(seq_i18n::Lang::from_locale))
        .unwrap_or(seq_i18n::Lang::Fr);
    seq_i18n::set(lang);
    seq_ui::theme::set(settings.theme.unwrap_or(seq_ui::theme::Theme::DEFAULT));
    eframe::run_native(
        "Sequensss",
        options,
        Box::new(move |cc| {
            SeqUi::install_theme(&cc.egui_ctx);
            Ok(Box::new(App::new(path, files, settings)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splash_hiss_decodes() {
        let s = seq_io::samples::decode_wav(HISS).unwrap();
        assert_eq!(s.sample_rate, 44_100);
        assert!((1.5..2.5).contains(&(s.len() as f32 / 44_100.0)));
        let peak = s.left.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.5 && peak * HISS_GAIN < 0.5, "{peak}");
        assert_eq!(s.resampled(48_000).len(), s.len() * 48_000 / 44_100);
    }
}
