//! "Sequens Retro" egui interface: transport bar, timeline, channel rack,
//! piano roll, sound settings, MIDI & audio, all on one screen, each panel
//! can be expanded.
//!
//! Small screen (phone, < `COMPACT_W` × `COMPACT_H` points): compact mode, condensed top
//! bar, one panel at a time picked with tabs at the bottom.
//!
//! The UI edits the `Project` (source of truth) directly. Side effects
//! (playback, files, devices) go up to the app as [`UiAction`]s.
//! Playback state comes only from [`PlaybackInfo`] (sent by the engine).

use eframe::egui::{self, Align2, Color32, Key, Rect, Sense, Stroke, UiBuilder, pos2, vec2};
use seq_i18n::{tr, trf};
use seq_model::*;
use std::collections::{HashMap, HashSet};

mod browser;
mod compact;
mod icons;
mod midi_panel;
mod notice;
mod piano;
mod playlist;
mod props;
mod rack;
pub mod skin;
mod sound;
pub mod theme;
mod widgets;

use icons::Icon;
pub use browser::Browser;
pub use notice::UpdateNotice;
pub use piano::PianoState;
pub use playlist::PlaylistState;
use skin::Kind;
use theme::*;

#[derive(Debug, Clone, PartialEq)]
pub enum UiAction {
    TogglePlay,
    Stop,
    Seek(Tick),
    Preview { channel: ChannelId, key: u8, vel: u8, on: bool },
    NewProject(usize),
    Open,
    Save,
    SaveAs,
    ExportWav,
    ExportMidi,
    Undo,
    Redo,
    /// Pick a WAV: for an existing channel, or `None` to create a channel.
    LoadSample(Option<ChannelId>),
    SelectAudio(Option<String>),
    SelectMidiIn(Option<String>),
    RefreshDevices,
    /// Interface language (applied and remembered by the app).
    SetLanguage(seq_i18n::Lang),
    /// "Check for updates" setting (remembered by the app).
    SetCheckUpdates(bool),
    /// Open a web page (release notes) in the system browser.
    OpenUrl(String),
    /// Do not notify about this version again.
    SkipVersion(String),
    /// Project browser (Android): open / share / delete a project by name,
    /// save under a new name, import a file from the device.
    BrowserOpen(String),
    BrowserShare(String),
    BrowserDelete(String),
    BrowserSaveAs,
    BrowserImport,
}

/// "Language" section of the menus: one entry per language, each named in its own language.
fn language_items(ui: &mut egui::Ui) -> Option<seq_i18n::Lang> {
    skin::menu_caption(ui, tr!("LANGUE", "LANGUAGE"));
    let cur = seq_i18n::get();
    let mut chosen = None;
    for l in seq_i18n::Lang::ALL {
        if skin::menu_item(ui, l.native_name(), TEXT, l == cur).clicked() && l != cur {
            chosen = Some(l);
        }
    }
    chosen
}

/// "Check for updates" switch of the menus; returns the new value when clicked.
fn update_toggle(ui: &mut egui::Ui, on: bool) -> Option<bool> {
    skin::menu_toggle(ui, tr!("Rechercher les mises à jour", "Check for updates"), on).clicked().then_some(!on)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelId {
    Timeline,
    Rack,
    Piano,
    Sound,
    Midi,
}

/// Below this screen size (egui points), compact mode. The desktop window has a
/// minimum size of 1180 × 700: it never switches to it.
const COMPACT_W: f32 = 1100.0;
const COMPACT_H: f32 = 600.0;
/// Usable width of the full top bar; below it (tablet, narrow window),
/// condensed bar above the grid.
const FULL_BAR_W: f32 = 1880.0;

/// Editor that receives edit commands (top bar, keyboard).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editor {
    Piano,
    Timeline,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditCmd {
    Copy,
    Paste,
    Delete,
    SelectAll,
    Deselect,
    Duplicate,
    /// Arrows: `dx` in snap steps, `dy` in rows (semitones / tracks), `big` = Shift.
    Arrow { dx: i32, dy: i32, big: bool },
}

/// What the engine sends back (read-only for the UI).
#[derive(Default)]
pub struct PlaybackInfo {
    pub playing: bool,
    pub playhead: f64,
    pub levels: HashMap<ChannelId, (f32, f32)>,
    pub master: (f32, f32),
}

#[derive(Default, Clone)]
pub struct DeviceInfo {
    pub audio: Vec<String>,
    pub midi_in: Vec<String>,
    pub midi_out: Vec<String>,
    pub current_audio: Option<String>,
    pub current_in: Option<String>,
    pub sample_rate: Option<u32>,
    pub audio_ok: bool,
    /// MIDI out ports actually open (indicator lights).
    pub open_outs: Vec<String>,
}

pub struct SeqUi {
    pub pattern: PatternId,
    pub channel: Option<ChannelId>,
    pub song_mode: bool,
    pub loop_region: Option<(Tick, Tick)>,
    pub metronome: bool,
    pub recording: bool,
    pub quantize: bool,
    pub status: String,
    pub file_label: String,
    pub dirty: bool,
    pub devices: DeviceInfo,
    pub can_undo: bool,
    pub can_redo: bool,
    pub focus: Option<PanelId>,
    /// Channels whose sample is loading (filled by the app every frame).
    pub loading: HashSet<ChannelId>,
    /// Compact mode (recomputed every frame) and the panel shown in that mode.
    compact: bool,
    pub tab: PanelId,
    /// Tab chosen during the frame, applied at the end (see `tab_bar`).
    pending_tab: Option<PanelId>,
    pub active_editor: Editor,
    edit_cmd: Option<(Editor, EditCmd)>,
    synth_open: Option<ChannelId>,
    arp_open: Option<ChannelId>,
    renaming_pattern: Option<(PatternId, String)>,
    renaming_channel: Option<(ChannelId, String)>,
    /// Channel menu showing the "Replace with" choices.
    replacing: Option<ChannelId>,
    step_paint: Option<(ChannelId, bool)>,
    /// Notes copied from a channel in the rack, at their positions in the pattern.
    /// Separate from the piano roll clipboard, which holds a selection relative to its start.
    channel_clipboard: Vec<Note>,
    /// "Preview" button held down: (channel, key) in progress.
    audition: Option<(ChannelId, u8)>,
    piano: PianoState,
    playlist: PlaylistState,
    /// Open project browser (filled by the app; Android).
    pub browser: Option<browser::Browser>,
    /// "Check for updates" setting (mirrored by the app).
    pub check_updates: bool,
    /// Newer version available (set by the app, cleared when closed).
    pub update: Option<UpdateNotice>,
    actions: Vec<UiAction>,
    shown_status: String,
    status_since: f64,
}

impl SeqUi {
    pub fn new(project: &Project) -> Self {
        SeqUi {
            pattern: project.patterns[0].id,
            channel: project.channels.first().map(|c| c.id),
            song_mode: false,
            loop_region: None,
            metronome: false,
            recording: false,
            quantize: true,
            status: String::new(),
            file_label: tr!("Sans titre", "Untitled").into(),
            dirty: false,
            devices: DeviceInfo::default(),
            can_undo: false,
            can_redo: false,
            focus: None,
            loading: HashSet::new(),
            compact: false,
            tab: PanelId::Rack,
            pending_tab: None,
            active_editor: Editor::Piano,
            edit_cmd: None,
            synth_open: None,
            arp_open: None,
            renaming_pattern: None,
            renaming_channel: None,
            replacing: None,
            step_paint: None,
            channel_clipboard: Vec::new(),
            audition: None,
            piano: PianoState::default(),
            playlist: PlaylistState::default(),
            browser: None,
            check_updates: true,
            update: None,
            actions: Vec::new(),
            shown_status: String::new(),
            status_since: 0.0,
        }
    }

    /// Installs fonts and theme (call once at startup).
    pub fn install_theme(ctx: &egui::Context) {
        theme::install(ctx);
    }

    /// After a new project / open.
    pub fn reset_for(&mut self, project: &Project) {
        let devices = std::mem::take(&mut self.devices);
        let status = std::mem::take(&mut self.status);
        let update = self.update.take();
        let (focus, tab, check_updates) = (self.focus, self.tab, self.check_updates);
        let clipboard = std::mem::take(&mut self.channel_clipboard);
        *self = SeqUi::new(project);
        self.channel_clipboard = clipboard;
        self.devices = devices;
        self.status = status;
        self.update = update;
        self.check_updates = check_updates;
        self.focus = focus;
        self.tab = tab;
    }

    fn act(&mut self, a: UiAction) {
        self.actions.push(a);
    }

    /// Edit command aimed at `editor`, if any.
    fn take_edit(&mut self, editor: Editor) -> Option<EditCmd> {
        match self.edit_cmd {
            Some((e, c)) if e == editor => {
                self.edit_cmd = None;
                Some(c)
            }
            _ => None,
        }
    }

    /// ⤢ button of a panel. Large screen: panel expanded within the grid. Compact mode:
    /// full screen (top bar and tabs hidden).
    fn toggle_focus(&mut self, p: PanelId) {
        if self.compact {
            self.tab = p;
            self.focus = if self.focus.is_some() { None } else { Some(p) };
        } else {
            self.focus = if self.focus == Some(p) { None } else { Some(p) };
        }
    }

    /// Full screen (compact mode): a single panel, with no bar or tabs.
    fn fullscreen(&self) -> bool {
        self.compact && self.focus.is_some()
    }

    /// Brings a panel on screen if it is not visible (tab in compact mode,
    /// expanded panel if a panel was already expanded).
    fn show_panel(&mut self, p: PanelId) {
        if self.compact {
            self.tab = p;
        } else if self.focus.is_some() {
            self.focus = Some(p);
        }
    }

    /// Target editor for edit commands, based on the panel visible in compact mode.
    fn editor_for_tab(&self) -> Editor {
        match self.tab {
            PanelId::Timeline => Editor::Timeline,
            _ => self.active_editor,
        }
    }

    /// Selection that stays valid even after undo / deletion.
    fn fix_selection(&mut self, project: &Project) {
        if project.pattern(self.pattern).is_none() {
            self.pattern = project.patterns[0].id;
        }
        if self.channel.is_none_or(|c| project.channel(c).is_none()) {
            self.channel = project.channels.first().map(|c| c.id);
        }
        for w in [&mut self.synth_open, &mut self.arp_open] {
            if w.is_some_and(|c| project.channel(c).is_none()) {
                *w = None;
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, project: &mut Project, info: &PlaybackInfo) -> Vec<UiAction> {
        self.fix_selection(project);
        let screen = ctx.screen_rect();
        self.compact = screen.width() < COMPACT_W || screen.height() < COMPACT_H;
        self.shortcuts(ctx);

        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG)).show(ctx, |ui| {
            let full = ui.max_rect();
            // Wooden side panels.
            let small_bar = self.compact || full.width() < FULL_BAR_W;
            let (wood_w, margin, gap) = if self.compact { (8.0, 12.0, 6.0) } else { (14.0, 22.0, 8.0) };
            let bar_h = if small_bar { 60.0 } else { 72.0 };
            let p = ui.painter();
            let wood = [(0.0, WOOD_DARK), (6.0 / 14.0, WOOD_MID), (1.0, WOOD)];
            skin::hgradient(p, Rect::from_min_size(full.min, vec2(wood_w, full.height())), &wood);
            let wood_r: Vec<(f32, Color32)> = wood.iter().rev().map(|&(t, c)| (1.0 - t, c)).collect();
            skin::hgradient(p, Rect::from_min_size(pos2(full.right() - wood_w, full.top()), vec2(wood_w, full.height())), &wood_r);

            let vpad = if self.compact { 6.0 } else { 10.0 };
            let inner = Rect::from_min_max(full.min + vec2(margin, vpad), full.max - vec2(margin, vpad));
            let fullscreen = self.fullscreen();
            let mut grid = inner;
            if !fullscreen {
                let bar = Rect::from_min_size(inner.min, vec2(inner.width(), bar_h));
                let mut bar_ui = ui.new_child(UiBuilder::new().max_rect(bar));
                if small_bar {
                    self.compact_top_bar(&mut bar_ui, bar, project, info);
                } else {
                    self.top_bar(&mut bar_ui, bar, project, info);
                }
                grid.min.y = bar.bottom() + gap;
            }
            if self.compact && !fullscreen {
                let tabs = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 50.0), inner.max);
                grid.max.y = tabs.top() - gap;
                let mut tab_ui = ui.new_child(UiBuilder::new().max_rect(tabs));
                self.tab_bar(&mut tab_ui, tabs);
            }
            for (id, rect) in self.layout(grid) {
                let mut pu = ui.new_child(UiBuilder::new().max_rect(rect).id_salt(("panel", id as u8)));
                pu.set_clip_rect(rect.expand(12.0).intersect(full));
                match id {
                    PanelId::Timeline => playlist::show(self, &mut pu, rect, project, info),
                    PanelId::Rack => rack::show(self, &mut pu, rect, project, info),
                    PanelId::Piano => piano::show(self, &mut pu, rect, project, info),
                    PanelId::Sound => sound::show(self, &mut pu, rect, project, info),
                    PanelId::Midi => midi_panel::show(self, &mut pu, rect, project),
                }
            }
        });

        if self.fullscreen() {
            self.floating_transport(ctx, info);
        }
        props::windows(self, ctx, project);
        self.browser_window(ctx);
        self.update_notice(ctx);
        self.toast(ctx);

        let pointer_down = ctx.input(|i| i.pointer.primary_down() || i.pointer.secondary_down());
        if !pointer_down {
            self.step_paint = None;
        }
        // Command not consumed (editor hidden): drop it.
        self.edit_cmd = None;
        if let Some(tab) = self.pending_tab.take() {
            self.tab = tab;
            match tab {
                PanelId::Piano => self.active_editor = Editor::Piano,
                PanelId::Timeline => self.active_editor = Editor::Timeline,
                _ => {}
            }
            ctx.request_repaint();
        }
        std::mem::take(&mut self.actions)
    }

    /// Panel rectangles (mockup grid, or a single expanded panel).
    fn layout(&self, grid: Rect) -> Vec<(PanelId, Rect)> {
        if self.compact {
            return vec![(self.tab, grid)];
        }
        if let Some(f) = self.focus {
            return vec![(f, grid)];
        }
        let gap = 8.0;
        let w = grid.width() - gap;
        let left_w = w * 900.0 / 1892.0;
        let h = grid.height() - 2.0 * gap;
        // Medium screen (tablet): taller bottom row (Sound / MIDI unreadable otherwise),
        // the timeline gives up some room. Large screen: mockup proportions.
        let (r1, r2) = if grid.width() < FULL_BAR_W - 44.0 { (h * 0.22, h * 0.43) } else { (h * 256.0 / 964.0, h * 452.0 / 964.0) };
        let y1 = grid.top() + r1 + gap;
        let y2 = y1 + r2 + gap;
        let x1 = grid.left() + left_w + gap;
        vec![
            (PanelId::Timeline, Rect::from_min_max(grid.min, pos2(grid.right(), grid.top() + r1))),
            (PanelId::Rack, Rect::from_min_max(pos2(grid.left(), y1), pos2(x1 - gap, y1 + r2))),
            (PanelId::Piano, Rect::from_min_max(pos2(x1, y1), pos2(grid.right(), y1 + r2))),
            (PanelId::Sound, Rect::from_min_max(pos2(grid.left(), y2), pos2(x1 - gap, grid.bottom()))),
            (PanelId::Midi, Rect::from_min_max(pos2(x1, y2), grid.max)),
        ]
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let ed = self.active_editor;
        let (space, undo, redo, save, cmds) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            let shift = i.modifiers.shift;
            let mut cmds = Vec::new();
            // On Windows, egui turns Ctrl+C / Ctrl+V into Copy / Paste events.
            if i.events.iter().any(|e| matches!(e, egui::Event::Copy)) || (cmd && i.key_pressed(Key::C)) {
                cmds.push(EditCmd::Copy);
            }
            if i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))) || (cmd && i.key_pressed(Key::V)) {
                cmds.push(EditCmd::Paste);
            }
            if i.key_pressed(Key::Delete) {
                cmds.push(EditCmd::Delete);
            }
            if cmd && i.key_pressed(Key::A) {
                cmds.push(EditCmd::SelectAll);
            }
            if i.key_pressed(Key::Escape) {
                cmds.push(EditCmd::Deselect);
            }
            if cmd && i.key_pressed(Key::D) {
                cmds.push(EditCmd::Duplicate);
            }
            for (k, dx, dy) in [(Key::ArrowLeft, -1, 0), (Key::ArrowRight, 1, 0), (Key::ArrowUp, 0, -1), (Key::ArrowDown, 0, 1)] {
                if i.key_pressed(k) {
                    cmds.push(EditCmd::Arrow { dx, dy, big: shift });
                }
            }
            (
                i.key_pressed(Key::Space),
                cmd && i.key_pressed(Key::Z) && !shift,
                cmd && (i.key_pressed(Key::Y) || (shift && i.key_pressed(Key::Z))),
                cmd && i.key_pressed(Key::S),
                cmds,
            )
        });
        if space {
            self.act(UiAction::TogglePlay);
        }
        if undo {
            self.act(UiAction::Undo);
        }
        if redo {
            self.act(UiAction::Redo);
        }
        if save {
            self.act(UiAction::Save);
        }
        if let Some(c) = cmds.into_iter().next() {
            self.edit_cmd = Some((ed, c));
        }
    }

    fn toast(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if self.status != self.shown_status {
            self.shown_status = self.status.clone();
            self.status_since = now;
        }
        let age = now - self.status_since;
        if self.shown_status.is_empty() || age > 5.0 {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let alpha = if age > 4.0 { (5.0 - age) as f32 } else { 1.0 };
        egui::Area::new(egui::Id::new("toast"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, if self.compact { -72.0 } else { -26.0 }))
            .interactable(false)
            .show(ctx, |ui| {
                let font = body(15.0);
                let g = ui.fonts(|f| f.layout_no_wrap(self.shown_status.clone(), font.clone(), TEXT));
                let (rect, _) = ui.allocate_exact_size(g.size() + vec2(36.0, 20.0), Sense::hover());
                let p = ui.painter();
                p.rect_filled(rect.translate(vec2(0.0, 6.0)), 10.0, Color32::from_black_alpha((110.0 * alpha) as u8));
                p.rect_filled(rect, 10.0, with_alpha(POPUP, (245.0 * alpha) as u8));
                p.rect_stroke(rect, 10.0, Stroke::new(1.0_f32, with_alpha(ORANGE, (200.0 * alpha) as u8)));
                skin::led(p, pos2(rect.left() + 14.0, rect.center().y), 4.0, with_alpha(ORANGE, (255.0 * alpha) as u8), true);
                p.text(pos2(rect.left() + 26.0, rect.center().y), Align2::LEFT_CENTER, &self.shown_status, font, with_alpha(TEXT, (255.0 * alpha) as u8));
            });
    }

    // ------------------------------------------------------------------
    // Top bar
    // ------------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut egui::Ui, rect: Rect, project: &mut Project, info: &PlaybackInfo) {
        let p = ui.painter().clone();
        skin::bevel(&p, rect, 10.0, BAR_TOP, BAR_BOT, EDGE, Some(BAR_HL));
        let mut row = ui.new_child(
            UiBuilder::new().max_rect(rect.shrink2(vec2(12.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let ui = &mut row;
        ui.spacing_mut().item_spacing.x = 6.0;

        // Logo
        let (logo, _) = ui.allocate_exact_size(vec2(196.0, 56.0), Sense::hover());
        p.text(pos2(logo.left(), logo.center().y - 9.0), Align2::LEFT_CENTER, "SEQUENS", michroma(24.0), ORANGE);
        let file = format!("RHYTHM COMPOSER · {}", self.file_label);
        let g = p.layout_no_wrap(file, medium(12.0), TEXT_DIM);
        let gw = g.size().x.min(logo.width() - 12.0);
        p.with_clip_rect(logo).galley(pos2(logo.left(), logo.center().y + 7.0), g, TEXT_DIM);
        if self.dirty {
            skin::led(&p, pos2(logo.left() + gw + 7.0, logo.center().y + 14.0), 3.5, ORANGE, false);
        }

        // File + save
        let file_btn = skin::icon_text_button(ui, 52.0, Kind::Cream, Icon::Menu, tr!("Fichier", "File"));
        self.file_menu(ui, &file_btn);
        if skin::icon_button(ui, vec2(52.0, 52.0), Kind::Cream, Icon::Save, 22.0).on_hover_text(tr!("Enregistrer (Ctrl+S)", "Save (Ctrl+S)")).clicked() {
            self.act(UiAction::Save);
        }
        groove(ui);

        // Transport
        let play_icon = if info.playing { Icon::Pause } else { Icon::Play };
        if skin::icon_button(ui, vec2(76.0, 56.0), Kind::Orange, play_icon, 26.0).on_hover_text(tr!("Lecture / pause (Espace)", "Play / pause (Space)")).clicked() {
            self.act(UiAction::TogglePlay);
        }
        if skin::icon_button(ui, vec2(56.0, 56.0), Kind::Cream, Icon::Stop, 20.0).on_hover_text("Stop").clicked() {
            self.act(UiAction::Stop);
        }
        let rec = self.recording;
        if skin::button(ui, vec2(56.0, 56.0), Kind::Cream, rec, |p, r, _| {
            if rec {
                skin::led(p, r.center(), 8.0, REC_RED, true);
            } else {
                icons::draw(p, r, 20.0, Icon::Record, REC_RED, 1.0);
            }
        })
        .on_hover_text(tr!("Enregistrer le MIDI in dans le pattern courant (mode PAT)", "Record MIDI in into the current pattern (PAT mode)"))
        .clicked()
        {
            self.recording = !self.recording;
        }
        let metro = self.metronome;
        if skin::button(ui, vec2(96.0, 56.0), Kind::Dark, false, |p, r, _| {
            let c = if metro { LCD_GREEN } else { TEXT_DIMMER };
            if metro {
                p.rect_stroke(r.shrink(1.0), 8.0, Stroke::new(1.0_f32, with_alpha(LCD_GREEN, 90)));
            }
            icons::draw(p, Rect::from_center_size(pos2(r.left() + 24.0, r.center().y), vec2(20.0, 20.0)), 20.0, Icon::Metronome, c, 1.8);
            p.text(pos2(r.left() + 40.0, r.center().y), Align2::LEFT_CENTER, tr!("Métro", "Metro"), body(15.0), c);
        })
        .on_hover_text(tr!("Métronome", "Metronome"))
        .clicked()
        {
            self.metronome = !self.metronome;
        }

        // Mode
        skin::segmented(ui, &mut self.song_mode, &[(false, None, "PAT"), (true, None, "SONG")], 46.0, 78.0, skin::SegStyle::Orange, cond(18.0));

        // Position
        let bar = project.bar_len().max(1) as f64;
        let beat = project.beat_len().max(1) as f64;
        let t = info.playhead.max(0.0);
        let b = (t / bar).floor();
        let be = ((t - b * bar) / beat).floor();
        let tk = t - b * bar - be * beat;
        let pos = format!("{:03}:{}:{:03}", b as u64 + 1, be as u64 + 1, tk as u64);
        skin::lcd(ui, vec2(196.0, 56.0), "POSITION", &pos, LCD_GREEN, lcd(40.0), Sense::hover());

        // BPM
        ui.spacing_mut().item_spacing.x = 3.0;
        if skin::button(ui, vec2(44.0, 56.0), Kind::Cream, false, |p, r, c| {
            p.text(r.center(), Align2::CENTER_CENTER, "−", body(24.0), c);
        })
        .clicked()
        {
            project.tempo_bpm = (project.tempo_bpm - 1.0).round().max(20.0);
        }
        let bpm = skin::lcd(ui, vec2(104.0, 56.0), "BPM", &format!("{:.1}", project.tempo_bpm), ORANGE_TOP, lcd(40.0), Sense::click_and_drag())
            .on_hover_text(tr!("Glisser verticalement pour régler (Maj : précis) · molette", "Drag vertically to adjust (Shift: fine) · wheel"));
        if bpm.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            let d = -bpm.drag_delta().y * if fine { 0.05 } else { 0.25 };
            project.tempo_bpm = ((project.tempo_bpm + d as f64) * 10.0).round() / 10.0;
        }
        if bpm.hovered() {
            let s = ui.input(|i| i.smooth_scroll_delta.y);
            if s != 0.0 {
                project.tempo_bpm += (s / 40.0) as f64;
            }
        }
        if skin::button(ui, vec2(44.0, 56.0), Kind::Cream, false, |p, r, c| {
            p.text(r.center(), Align2::CENTER_CENTER, "+", body(24.0), c);
        })
        .clicked()
        {
            project.tempo_bpm = (project.tempo_bpm + 1.0).round().min(300.0);
        }
        project.tempo_bpm = project.tempo_bpm.clamp(20.0, 300.0);
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add_space(6.0);

        // Swing
        ui.vertical(|ui| {
            ui.set_width(150.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            let (lr, _) = ui.allocate_exact_size(vec2(150.0, 14.0), Sense::hover());
            skin::caption(ui.painter(), lr.left_center(), Align2::LEFT_CENTER, "SWING", TEXT_DIM);
            ui.painter().text(lr.right_center(), Align2::RIGHT_CENTER, format!("{:.0} %", project.swing * 100.0), medium(12.0), TEXT);
            skin::metal_slider(ui, &mut project.swing, 150.0).on_hover_text(tr!("Double-clic : 0 %", "Double-click: 0 %"));
        });
        ui.add_space(4.0);

        // Time signature
        let sig = format!("{}/{}", project.time_sig.0, project.time_sig.1);
        let sig_btn = skin::button(ui, vec2(78.0, 56.0), Kind::Dark, false, |p, r, _| {
            skin::caption(p, pos2(r.left() + 14.0, r.top() + 14.0), Align2::LEFT_CENTER, tr!("MESURE", "TIME SIG."), LCD_LABEL);
            p.text(pos2(r.left() + 14.0, r.center().y + 8.0), Align2::LEFT_CENTER, &sig, mono(22.0), TEXT);
        });
        let sig_id = ui.make_persistent_id("sig_popup");
        if sig_btn.clicked() {
            ui.memory_mut(|m| m.toggle_popup(sig_id));
        }
        egui::popup_below_widget(ui, sig_id, &sig_btn, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(220.0);
            skin::menu_caption(ui, tr!("TEMPS PAR MESURE", "BEATS PER BAR"));
            let mut num = project.time_sig.0 as u32;
            if ui.add(egui::DragValue::new(&mut num).range(1..=16)).changed() {
                project.time_sig.0 = num as u8;
            }
            skin::menu_caption(ui, tr!("UNITÉ", "BEAT UNIT"));
            ui.horizontal(|ui| {
                for d in [2u8, 4, 8, 16] {
                    ui.selectable_value(&mut project.time_sig.1, d, d.to_string());
                }
            });
        });

        // Edit (right-aligned)
        skin::spacer_right(ui, 5.0 * 64.0 + 4.0 * 6.0 + 8.0 + 6.0 + 30.0 + 6.0);
        ui.add_enabled_ui(self.can_undo, |ui| {
            if skin::icon_caption_button(ui, vec2(64.0, 56.0), Icon::Undo, tr!("Annuler", "Undo")).on_hover_text("Ctrl+Z").clicked() {
                self.act(UiAction::Undo);
            }
        });
        ui.add_enabled_ui(self.can_redo, |ui| {
            if skin::icon_caption_button(ui, vec2(64.0, 56.0), Icon::Redo, tr!("Rétablir", "Redo")).on_hover_text("Ctrl+Y").clicked() {
                self.act(UiAction::Redo);
            }
        });
        ui.add_space(8.0);
        let target = match self.active_editor {
            Editor::Piano => "piano roll",
            Editor::Timeline => "timeline",
        };
        for (icon, label, cmd) in [(Icon::Copy, tr!("Copier", "Copy"), EditCmd::Copy), (Icon::Paste, tr!("Coller", "Paste"), EditCmd::Paste), (Icon::Trash, tr!("Suppr.", "Delete"), EditCmd::Delete)] {
            if skin::icon_caption_button(ui, vec2(64.0, 56.0), icon, label).on_hover_text(trf!("Agit sur : {target}", "Acts on: {target}")).clicked() {
                self.edit_cmd = Some((self.active_editor, cmd));
            }
        }

        // Master VU meter
        let (mr, _) = ui.allocate_exact_size(vec2(30.0, 52.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(mr, 6.0, LCD_BG);
        p.rect_stroke(mr, 6.0, Stroke::new(1.0_f32, LCD_EDGE));
        let inner = mr.shrink(4.0);
        skin::meter_bar(p, Rect::from_min_size(inner.min + vec2(1.5, 0.0), vec2(8.0, inner.height())), info.master.0);
        skin::meter_bar(p, Rect::from_min_size(inner.min + vec2(11.5, 0.0), vec2(8.0, inner.height())), info.master.1);
    }

    fn file_menu(&mut self, ui: &mut egui::Ui, btn: &egui::Response) {
        let id = ui.make_persistent_id("file_menu");
        if btn.clicked() {
            ui.memory_mut(|m| m.toggle_popup(id));
        }
        egui::popup_below_widget(ui, id, btn, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(260.0);
            skin::menu_caption(ui, tr!("NOUVEAU PROJET", "NEW PROJECT"));
            for (i, kit) in KITS.iter().enumerate() {
                if skin::menu_item(ui, &format!("Kit {}", kit.name), TEXT, false).clicked() {
                    self.act(UiAction::NewProject(i));
                }
            }
            skin::menu_separator(ui);
            for (label, a) in [
                (tr!("Ouvrir…", "Open…"), UiAction::Open),
                (tr!("Enregistrer          Ctrl+S", "Save                 Ctrl+S"), UiAction::Save),
                (tr!("Enregistrer sous…", "Save as…"), UiAction::SaveAs),
            ] {
                if skin::menu_item(ui, label, TEXT, false).clicked() {
                    self.act(a);
                }
            }
            skin::menu_separator(ui);
            if skin::menu_item(ui, tr!("Exporter en WAV…", "Export WAV…"), TEXT, false).clicked() {
                self.act(UiAction::ExportWav);
            }
            if skin::menu_item(ui, tr!("Exporter en MIDI…", "Export MIDI…"), TEXT, false).clicked() {
                self.act(UiAction::ExportMidi);
            }
            skin::menu_separator(ui);
            if let Some(l) = language_items(ui) {
                self.act(UiAction::SetLanguage(l));
            }
            skin::menu_separator(ui);
            if let Some(on) = update_toggle(ui, self.check_updates) {
                self.act(UiAction::SetCheckUpdates(on));
            }
        });
    }

    /// "Add a channel" menu (rack).
    fn add_channel_items(&mut self, ui: &mut egui::Ui, project: &mut Project) {
        skin::menu_caption(ui, tr!("SON DE BATTERIE INTÉGRÉ", "BUILT-IN DRUM SOUND"));
        if let Some(d) = drum_grid(ui, None) {
            let id = project.add_channel(d.name(), ChannelKind::Sampler { source: SampleSource::Builtin(d) });
            self.after_add(id);
            ui.memory_mut(|m| m.close_popup());
        }
        skin::menu_separator(ui);
        if skin::menu_item(ui, tr!("Sampler (fichier WAV)…", "Sampler (WAV file)…"), TEXT, false).clicked() {
            self.act(UiAction::LoadSample(None));
        }
        if skin::menu_item(ui, tr!("Synthé", "Synth"), TEXT, false).clicked() {
            let id = project.add_channel("Synth", ChannelKind::Synth(SynthParams::default()));
            if let Some(id) = id {
                project.channel_mut(id).unwrap().default_key = 48;
            }
            self.after_add(id);
        }
        if skin::menu_item(ui, "MIDI out", TEXT, false).clicked() {
            let id = project.add_channel("MIDI out", ChannelKind::MidiOut { channel: 0, out: 0 });
            self.after_add(id);
        }
    }

    fn after_add(&mut self, id: Option<ChannelId>) {
        match id {
            Some(id) => self.channel = Some(id),
            None => self.status = format!("Maximum {MAX_CHANNELS} channels"),
        }
    }
}

/// Built-in drum sounds, two per row (`current` highlighted). Returns the one clicked.
fn drum_grid(ui: &mut egui::Ui, current: Option<Drum>) -> Option<Drum> {
    let mut picked = None;
    egui::Grid::new("drum_grid").num_columns(2).spacing([4.0, 2.0]).show(ui, |ui| {
        for (i, d) in Drum::ALL.into_iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(150.0, 34.0), Sense::click());
            let p = ui.painter();
            if current == Some(d) {
                p.rect_filled(r, 6.0, TILE_SEL);
                p.rect_filled(Rect::from_min_size(r.left_top() + vec2(0.0, 7.0), vec2(3.0, r.height() - 14.0)), 1.5, ORANGE);
            } else if resp.hovered() {
                p.rect_filled(r, 6.0, hex(0x433a32));
            }
            p.text(pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, d.name(), body(14.5), TEXT);
            if resp.clicked() {
                picked = Some(d);
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
    picked
}

/// Engraved vertical separator.
fn groove(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(4.0, 44.0), Sense::hover());
    let p = ui.painter();
    p.line_segment([pos2(r.center().x, r.top()), pos2(r.center().x, r.bottom())], Stroke::new(1.0_f32, GROOVE));
    p.line_segment([pos2(r.center().x + 1.0, r.top()), pos2(r.center().x + 1.0, r.bottom())], Stroke::new(1.0_f32, BAR_HL));
}
