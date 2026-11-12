//! Real-time engine: transport, scheduler, instruments, mix.
//!
//! Timeline: we keep an origin (`s0`, song position in ticks) and a frame counter
//! since that origin. The position at frame `f` is `s0 + f / spt`, wrapped into
//! the loop region (the pattern in Pattern mode, the loop or the whole song in Song mode).
//! Any tempo / region change "rebases" the origin, with no drift.
//!
//! Note-offs are not in the graph: the engine generates them from the note's length
//! (active notes table). A graph swap therefore cannot leave a stuck note.

use crate::MAX_BLOCK;
use crate::arp::ArpState;
use crate::clock::samples_per_tick;
use crate::graph::{ChannelParams, CompiledPattern, PlaybackGraph};
use crate::instrument::Instrument;
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use seq_model::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayMode {
    Pattern(PatternId),
    Song,
}

pub enum SlotKind {
    Audio(Box<dyn Instrument>),
    /// Notes sent to the MIDI out thread; the MIDI channel comes from `ChannelParams`.
    Midi,
}

pub enum EngineCmd {
    Play,
    Pause,
    /// Stop + return to the start of the region.
    Stop,
    Seek(f64),
    SetMode(PlayMode),
    /// Loop in Song mode (None = the whole song).
    SetLoop(Option<(Tick, Tick)>),
    SwapGraph(Box<PlaybackGraph>),
    /// Adds or replaces a channel's instrument (built off the audio thread).
    SetSlot(ChannelId, SlotKind),
    RemoveSlot(ChannelId),
    SetSynthParams(ChannelId, SynthParams),
    Preview { channel: ChannelId, key: u8, vel: u8, on: bool },
    /// Channel played by MIDI in.
    SetLiveChannel(Option<ChannelId>),
    SetRecording(bool),
    SetMetronome(bool),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EngineEvent {
    Levels { channel: ChannelId, peak_l: f32, peak_r: f32 },
    /// Master output peaks (after soft clip).
    Master { peak_l: f32, peak_r: f32 },
    /// Note played via MIDI in while recording. `tick` = position within the region;
    /// `time` = monotonic time in ticks (does not wrap at the loop end), for note lengths.
    Recorded { channel: ChannelId, key: u8, vel: u8, tick: f64, time: f64, on: bool },
}

/// Objects to free off the audio thread.
pub enum Garbage {
    Graph(#[allow(dead_code)] Box<PlaybackGraph>),
    Slot(#[allow(dead_code)] SlotKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiInMsg {
    pub bytes: [u8; 3],
    pub len: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MidiOutMsg {
    /// Index of the MIDI output (`Project::midi_outs`).
    pub port: u8,
    pub bytes: [u8; 3],
    /// Send delay from reception by the MIDI thread (audio output latency + offset).
    pub delay_secs: f64,
}

/// State read by the UI without locks (atomic values).
pub struct Shared {
    playhead_bits: AtomicU64,
    playing: AtomicBool,
}

impl Shared {
    pub fn playhead(&self) -> f64 {
        f64::from_bits(self.playhead_bits.load(Ordering::Relaxed))
    }
    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
}

/// UI side of the engine.
pub struct EngineHandle {
    pub cmds: Producer<EngineCmd>,
    pub events: Consumer<EngineEvent>,
    pub garbage: Consumer<Garbage>,
    pub shared: Arc<Shared>,
}

impl EngineHandle {
    /// `false` if the queue is full (command dropped, freed here on the UI side).
    pub fn send(&mut self, cmd: EngineCmd) -> bool {
        self.cmds.push(cmd).is_ok()
    }

    /// Frees the objects sent back by the audio thread.
    pub fn collect_garbage(&mut self) {
        while let Ok(g) = self.garbage.pop() {
            drop(g);
        }
    }

    pub fn poll_event(&mut self) -> Option<EngineEvent> {
        self.events.pop().ok()
    }
}

struct Slot {
    id: ChannelId,
    kind: SlotKind,
    params: ChannelParams,
    peak_l: f32,
    peak_r: f32,
    arp: ArpState,
}

#[derive(Clone, Copy)]
struct Pending {
    offset: usize,
    chan: u16,
    key: u8,
    vel: u8,
    /// End of the note, in ticks relative to the origin.
    end: f64,
}

#[derive(Clone, Copy)]
struct Active {
    slot: u16,
    key: u8,
    end: f64,
    /// Live note (MIDI in / preview): no scheduled end.
    live: bool,
}

const METRO: u16 = u16::MAX;
const MAX_PENDING: usize = 4096;
const MAX_ACTIVE: usize = 1024;

/// Metronome click.
struct Click {
    phase: f32,
    inc: f32,
    env: f32,
    decay: f32,
    remaining: usize,
    len: usize,
}

impl Click {
    fn new(sr: f32) -> Self {
        Click { phase: 0.0, inc: 0.0, env: 0.0, decay: (-1.0 / (0.012 * sr)).exp(), remaining: 0, len: (0.03 * sr) as usize }
    }
    fn trigger(&mut self, accent: bool, sr: f32) {
        self.inc = if accent { 1600.0 } else { 1100.0 } / sr;
        // Phase 1/4: the first sample is non-zero (sharp attack, detectable in tests).
        self.phase = 0.25;
        self.env = if accent { 1.0 } else { 0.7 };
        self.remaining = self.len;
    }
    fn process(&mut self, out: &mut [f32]) {
        for o in out.iter_mut() {
            if self.remaining == 0 {
                *o = 0.0;
                continue;
            }
            *o = (self.phase * std::f32::consts::TAU).sin() * self.env;
            self.phase = (self.phase + self.inc) % 1.0;
            self.env *= self.decay;
            self.remaining -= 1;
        }
    }
}

pub struct Engine {
    sr: f64,
    graph: Option<Box<PlaybackGraph>>,
    slots: Vec<Slot>,
    /// Graph channel index → slot index.
    chan_slot: Vec<Option<u16>>,
    bufs_l: Vec<Vec<f32>>,
    bufs_r: Vec<Vec<f32>>,
    click_buf: Vec<f32>,
    mix_l: Vec<f32>,
    mix_r: Vec<f32>,
    pending: Vec<Pending>,
    active: Vec<Active>,

    mode: PlayMode,
    loop_region: Option<(Tick, Tick)>,
    playing: bool,
    one_shot: bool,
    s0: f64,
    frames: u64,
    spt: f64,
    /// Monotonic clock (frames) at the start of the current block, playing or not: drives the arpeggios.
    clock: u64,

    metronome: bool,
    click: Click,
    live_channel: Option<ChannelId>,
    recording: bool,
    level_frames: usize,
    master_peak: (f32, f32),

    latency: f64,
    cb_frame: usize,

    cmds: Consumer<EngineCmd>,
    events: Producer<EngineEvent>,
    garbage: Producer<Garbage>,
    midi_in: Option<Consumer<MidiInMsg>>,
    midi_out: Option<Producer<MidiOutMsg>>,
    shared: Arc<Shared>,
}

impl Engine {
    pub fn new(
        sample_rate: u32,
        midi_in: Option<Consumer<MidiInMsg>>,
        midi_out: Option<Producer<MidiOutMsg>>,
    ) -> (Engine, EngineHandle) {
        let (cmd_tx, cmd_rx) = RingBuffer::new(1024);
        let (ev_tx, ev_rx) = RingBuffer::new(4096);
        let (gb_tx, gb_rx) = RingBuffer::new(256);
        let shared = Arc::new(Shared { playhead_bits: AtomicU64::new(0f64.to_bits()), playing: AtomicBool::new(false) });
        let sr = sample_rate.max(1) as f64;
        let engine = Engine {
            sr,
            graph: None,
            slots: Vec::with_capacity(MAX_CHANNELS),
            chan_slot: Vec::with_capacity(MAX_CHANNELS),
            bufs_l: vec![vec![0.0; MAX_BLOCK]; MAX_CHANNELS],
            bufs_r: vec![vec![0.0; MAX_BLOCK]; MAX_CHANNELS],
            click_buf: vec![0.0; MAX_BLOCK],
            mix_l: vec![0.0; MAX_BLOCK],
            mix_r: vec![0.0; MAX_BLOCK],
            pending: Vec::with_capacity(MAX_PENDING),
            active: Vec::with_capacity(MAX_ACTIVE),
            mode: PlayMode::Song,
            loop_region: None,
            playing: false,
            one_shot: false,
            s0: 0.0,
            frames: 0,
            spt: samples_per_tick(sr, 120.0),
            clock: 0,
            metronome: false,
            click: Click::new(sr as f32),
            live_channel: None,
            recording: false,
            level_frames: 0,
            master_peak: (0.0, 0.0),
            latency: 0.0,
            cb_frame: 0,
            cmds: cmd_rx,
            events: ev_tx,
            garbage: gb_tx,
            midi_in,
            midi_out,
            shared: shared.clone(),
        };
        let handle = EngineHandle { cmds: cmd_tx, events: ev_rx, garbage: gb_rx, shared };
        (engine, handle)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sr as u32
    }

    /// Offline rendering: no looping, playback stops at the end of the region.
    pub fn set_one_shot(&mut self, one_shot: bool) {
        self.one_shot = one_shot;
    }

    /// Current position (ticks).
    pub fn position(&self) -> f64 {
        if self.playing { self.song_at(self.frames as f64 / self.spt) } else { self.s0 }
    }

    /// Audio callback. `out` is interleaved with `channels` channels.
    pub fn process(&mut self, out: &mut [f32], channels: usize, latency_secs: f64) {
        let channels = channels.max(1);
        self.latency = latency_secs;
        self.cb_frame = 0;
        while let Ok(cmd) = self.cmds.pop() {
            self.handle_cmd(cmd);
        }
        self.drain_midi_in();

        let total = out.len() / channels;
        let mut done = 0;
        while done < total {
            let n = (total - done).min(MAX_BLOCK);
            self.cb_frame = done;
            self.render_block(n);
            for i in 0..n {
                let o = (done + i) * channels;
                if channels == 1 {
                    out[o] = 0.5 * (self.mix_l[i] + self.mix_r[i]);
                } else {
                    out[o] = self.mix_l[i];
                    out[o + 1] = self.mix_r[i];
                    for c in 2..channels {
                        out[o + c] = 0.0;
                    }
                }
            }
            done += n;
        }
        for x in out[total * channels..].iter_mut() {
            *x = 0.0;
        }
        self.shared.playhead_bits.store(self.position().to_bits(), Ordering::Relaxed);
        self.shared.playing.store(self.playing, Ordering::Relaxed);
    }

    pub fn handle_cmd(&mut self, cmd: EngineCmd) {
        match cmd {
            EngineCmd::Play => {
                if !self.playing {
                    self.normalize_origin();
                    self.playing = true;
                    self.frames = 0;
                }
            }
            EngineCmd::Pause => {
                if self.playing {
                    let pos = self.position();
                    self.kill_seq_notes();
                    self.flush_all_arps();
                    self.playing = false;
                    self.s0 = pos;
                    self.frames = 0;
                }
            }
            EngineCmd::Stop => {
                self.kill_seq_notes();
                self.flush_all_arps();
                self.playing = false;
                self.s0 = self.region_start();
                self.frames = 0;
            }
            EngineCmd::Seek(t) => {
                self.kill_seq_notes();
                self.flush_all_arps();
                self.s0 = t.max(0.0);
                self.frames = 0;
                self.normalize_origin();
            }
            EngineCmd::SetMode(m) => {
                if m != self.mode {
                    self.kill_seq_notes();
                    self.flush_all_arps();
                    self.mode = m;
                    self.s0 = self.region_start();
                    self.frames = 0;
                }
            }
            EngineCmd::SetLoop(l) => {
                self.rebase();
                self.loop_region = l.filter(|(a, b)| b > a);
                self.normalize_origin();
            }
            EngineCmd::SwapGraph(g) => {
                self.rebase();
                self.spt = samples_per_tick(self.sr, g.tempo_bpm);
                if let Some(old) = self.graph.replace(g) {
                    self.trash(Garbage::Graph(old));
                }
                self.refresh_slots();
                self.normalize_origin();
            }
            EngineCmd::SetSlot(id, kind) => {
                if let Some(i) = self.slot_index(id) {
                    self.kill_slot_notes(i);
                    self.flush_arp(i);
                    let old = std::mem::replace(&mut self.slots[i].kind, kind);
                    self.trash(Garbage::Slot(old));
                } else if self.slots.len() < MAX_CHANNELS {
                    self.slots.push(Slot {
                        id,
                        kind,
                        params: ChannelParams::silent(id),
                        peak_l: 0.0,
                        peak_r: 0.0,
                        arp: ArpState::default(),
                    });
                } else {
                    self.trash(Garbage::Slot(kind));
                }
                self.refresh_slots();
            }
            EngineCmd::RemoveSlot(id) => {
                if let Some(i) = self.slot_index(id) {
                    self.kill_slot_notes(i);
                    self.flush_arp(i);
                    let last = self.slots.len() - 1;
                    let slot = self.slots.swap_remove(i);
                    self.trash(Garbage::Slot(slot.kind));
                    if i != last {
                        for a in self.active.iter_mut().filter(|a| a.slot as usize == last) {
                            a.slot = i as u16;
                        }
                    }
                    self.refresh_slots();
                }
            }
            EngineCmd::SetSynthParams(id, p) => {
                if let Some(i) = self.slot_index(id) {
                    if let SlotKind::Audio(inst) = &mut self.slots[i].kind {
                        inst.set_synth_params(&p);
                    }
                }
            }
            EngineCmd::Preview { channel, key, vel, on } => {
                if let Some(i) = self.slot_index(channel) {
                    if on { self.live_on(i, key, vel) } else { self.live_off(i, key) }
                }
            }
            EngineCmd::SetLiveChannel(c) => {
                if c != self.live_channel {
                    self.kill_live_notes();
                    self.live_channel = c;
                }
            }
            EngineCmd::SetRecording(r) => self.recording = r,
            EngineCmd::SetMetronome(m) => self.metronome = m,
        }
    }

    // ------------------------------------------------------------------
    // Timeline
    // ------------------------------------------------------------------

    fn region(&self) -> Option<(f64, f64)> {
        let g = self.graph.as_ref()?;
        match self.mode {
            PlayMode::Pattern(id) => {
                let p = &g.patterns[g.pattern_index(id)?];
                Some((0.0, p.length as f64))
            }
            PlayMode::Song => {
                if let Some((a, b)) = self.loop_region {
                    return Some((a as f64, b as f64));
                }
                (g.song_length > 0).then_some((0.0, g.song_length as f64))
            }
        }
    }

    fn region_start(&self) -> f64 {
        self.region().map(|r| r.0).unwrap_or(0.0)
    }

    /// Song position `d` ticks after the origin.
    fn song_at(&self, d: f64) -> f64 {
        let u = self.s0 + d;
        match self.region() {
            Some((l0, l1)) if u >= l1 && !self.one_shot => {
                let p = l1 - l0;
                if p <= 0.0 { l0 } else { l0 + (u - l1) % p }
            }
            _ => u,
        }
    }

    /// Moves the origin to the current position (before a tempo/region change).
    fn rebase(&mut self) {
        if !self.playing {
            return;
        }
        let d = self.frames as f64 / self.spt;
        self.s0 = self.song_at(d);
        for a in self.active.iter_mut().filter(|a| !a.live) {
            a.end -= d;
        }
        self.frames = 0;
    }

    fn normalize_origin(&mut self) {
        if self.s0 < 0.0 || !self.s0.is_finite() {
            self.s0 = 0.0;
        }
        if let Some((l0, l1)) = self.region() {
            if self.s0 >= l1 {
                self.s0 = l0;
            }
        }
    }

    // ------------------------------------------------------------------
    // Slots & notes
    // ------------------------------------------------------------------

    fn slot_index(&self, id: ChannelId) -> Option<usize> {
        self.slots.iter().position(|s| s.id == id)
    }

    fn refresh_slots(&mut self) {
        self.chan_slot.clear();
        for si in 0..self.slots.len() {
            let id = self.slots[si].id;
            let new = self
                .graph
                .as_ref()
                .and_then(|g| g.channels.iter().find(|c| c.id == id).copied())
                .unwrap_or(ChannelParams::silent(id));
            let old = self.slots[si].params;
            if old.arp.enabled != new.arp.enabled {
                // Arpeggiator toggle: silence everything using the old routing.
                self.kill_slot_notes(si);
                self.flush_arp(si);
            } else if old.arp != new.arp {
                self.slots[si].arp.mark_dirty();
            }
            self.slots[si].params = new;
        }
        if let Some(g) = self.graph.as_ref() {
            for c in g.channels.iter().take(MAX_CHANNELS) {
                let idx = self.slots.iter().position(|s| s.id == c.id).map(|i| i as u16);
                self.chan_slot.push(idx);
            }
        }
    }

    fn trash(&mut self, g: Garbage) {
        if let Err(PushError::Full(g)) = self.garbage.push(g) {
            // Queue full: leak rather than free on the audio thread.
            std::mem::forget(g);
        }
    }

    fn midi_send(&mut self, port: u8, bytes: [u8; 3], at: usize) {
        let delay_secs = self.latency + (self.cb_frame + at) as f64 / self.sr;
        if let Some(tx) = self.midi_out.as_mut() {
            let _ = tx.push(MidiOutMsg { port, bytes, delay_secs });
        }
    }

    /// Incoming note for a channel (sequencer, MIDI in, preview): choke, then arpeggiator
    /// or instrument.
    fn send_on(&mut self, si: usize, key: u8, vel: u8, at: usize) {
        let choke = self.slots[si].params.choke;
        if choke != 0 {
            for j in 0..self.slots.len() {
                if j != si && self.slots[j].params.choke == choke {
                    self.kill_slot_notes(j);
                    self.flush_arp(j);
                    if let SlotKind::Audio(inst) = &mut self.slots[j].kind {
                        inst.all_off();
                    }
                }
            }
        }
        if self.slots[si].params.arp.enabled {
            self.arp_press(si, key, vel, at);
        } else {
            self.raw_on(si, key, vel, at);
        }
    }

    fn send_off(&mut self, si: usize, key: u8, at: usize) {
        if self.slots[si].params.arp.enabled {
            let p = self.slots[si].params.arp;
            self.slots[si].arp.release(key, &p);
        } else {
            self.raw_off(si, key, at);
        }
    }

    /// Note sent directly to the instrument / MIDI output.
    fn raw_on(&mut self, si: usize, key: u8, vel: u8, at: usize) {
        let params = self.slots[si].params;
        match &mut self.slots[si].kind {
            SlotKind::Audio(inst) => inst.note_on(key, vel),
            SlotKind::Midi => {
                // Same rule as the audio mix: muted, or another channel is soloed.
                let any_solo = self.slots.iter().any(|s| s.params.solo);
                let audible = !params.mute && (!any_solo || params.solo);
                if let (Some(ch), true) = (params.midi_channel, audible) {
                    self.midi_send(params.midi_port, [0x90 | ch, key & 0x7f, vel.clamp(1, 127)], at);
                    // No audio: the channel meter shows MIDI activity.
                    let v = vel as f32 / 127.0;
                    let s = &mut self.slots[si];
                    s.peak_l = s.peak_l.max(v);
                    s.peak_r = s.peak_r.max(v);
                }
            }
        }
    }

    fn raw_off(&mut self, si: usize, key: u8, at: usize) {
        let params = self.slots[si].params;
        match &mut self.slots[si].kind {
            SlotKind::Audio(inst) => inst.note_off(key),
            SlotKind::Midi => {
                if let Some(ch) = params.midi_channel {
                    self.midi_send(params.midi_port, [0x80 | ch, key & 0x7f, 0], at);
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Arpeggiator
    // ------------------------------------------------------------------

    /// Block-relative frame of an engine clock instant (in frames).
    fn clock_frame(&self, t: f64) -> i64 {
        (t - self.clock as f64).ceil() as i64
    }

    fn arp_press(&mut self, si: usize, key: u8, vel: u8, at: usize) {
        let p = self.slots[si].params.arp;
        if !self.slots[si].arp.press(key, vel, &p) {
            return;
        }
        // Start: immediately, or on the next step of the song grid (sync).
        let now = (self.clock + at as u64) as f64;
        let mut wait = 0.0;
        if p.sync && self.playing {
            let pos = self.song_at((self.frames + at as u64) as f64 / self.spt);
            let r = p.rate.max(1) as f64;
            let m = pos.rem_euclid(r);
            if m >= 1.0 && r - m >= 1.0 {
                wait = (r - m) * self.spt;
            }
        }
        let a = &mut self.slots[si].arp;
        a.running = true;
        a.step = 0;
        a.next = now + wait;
    }

    /// Silences the arpeggio's notes and forgets held notes.
    fn flush_arp(&mut self, si: usize) {
        while self.slots[si].arp.sounding_n > 0 {
            let (key, _) = self.slots[si].arp.sounding[0];
            self.slots[si].arp.remove_sounding(0);
            self.raw_off(si, key, 0);
        }
        self.slots[si].arp.reset_held();
    }

    fn flush_all_arps(&mut self) {
        for si in 0..self.slots.len() {
            self.flush_arp(si);
        }
    }

    fn fire_arps(&mut self, cur: usize) {
        for si in 0..self.slots.len() {
            let mut i = 0;
            while i < self.slots[si].arp.sounding_n {
                let (key, end) = self.slots[si].arp.sounding[i];
                if self.clock_frame(end) <= cur as i64 {
                    self.slots[si].arp.remove_sounding(i);
                    self.raw_off(si, key, cur);
                } else {
                    i += 1;
                }
            }
            let p = self.slots[si].params.arp;
            let a = &self.slots[si].arp;
            if p.enabled && a.running && a.held_n > 0 && self.clock_frame(a.next) <= cur as i64 {
                self.arp_step(si, cur);
            }
        }
    }

    fn arp_step(&mut self, si: usize, cur: usize) {
        let p = self.slots[si].params.arp;
        let now = (self.clock + cur as u64) as f64;
        // Step length in frames (at the current tempo).
        let rate = p.rate.max(1) as f64 * self.spt;
        let mut notes = [(0u8, 0u8); 16];
        let mut k = 0;
        let off;
        {
            let a = &mut self.slots[si].arp;
            a.rebuild_if_needed(&p);
            off = a.next + (rate * p.gate.clamp(0.05, 1.0) as f64).max(1.0);
            if p.direction == ArpDirection::Random && a.step % p.repeat.max(1) as u64 == 0 {
                a.last_rand = seq_model::arp::xorshift(&mut a.rng);
            }
            match a.pool.index(a.step, &p, a.last_rand) {
                Some(i) => {
                    notes[0] = a.pool.keys[i];
                    k = 1;
                }
                None => {
                    for j in 0..a.pool.n.min(notes.len()) {
                        notes[j] = a.pool.keys[j];
                        k += 1;
                    }
                }
            }
            a.step += 1;
            a.next += rate;
            if a.next <= now {
                a.next = now + rate;
            }
        }
        for &(key, vel) in &notes[..k] {
            if let Some(i) = self.slots[si].arp.sounding[..self.slots[si].arp.sounding_n].iter().position(|s| s.0 == key) {
                self.slots[si].arp.remove_sounding(i);
                self.raw_off(si, key, cur);
            }
            self.raw_on(si, key, vel, cur);
            self.slots[si].arp.push_sounding(key, off);
        }
    }

    fn next_arp_after(&self, cur: usize, n: usize) -> usize {
        let mut next = n as i64;
        let cur = cur as i64;
        for s in &self.slots {
            for &(_, end) in &s.arp.sounding[..s.arp.sounding_n] {
                let f = self.clock_frame(end);
                if f > cur && f < next {
                    next = f;
                }
            }
            if s.params.arp.enabled && s.arp.running && s.arp.held_n > 0 {
                let f = self.clock_frame(s.arp.next);
                if f > cur && f < next {
                    next = f;
                }
            }
        }
        next as usize
    }

    fn remove_active_where(&mut self, at: usize, pred: impl Fn(&Active) -> bool) {
        let mut i = 0;
        while i < self.active.len() {
            let a = self.active[i];
            if pred(&a) {
                self.active.swap_remove(i);
                self.send_off(a.slot as usize, a.key, at);
            } else {
                i += 1;
            }
        }
    }

    fn kill_seq_notes(&mut self) {
        self.remove_active_where(0, |a| !a.live);
    }

    fn kill_live_notes(&mut self) {
        self.remove_active_where(0, |a| a.live);
    }

    fn kill_slot_notes(&mut self, si: usize) {
        self.remove_active_where(0, |a| a.slot as usize == si);
    }

    fn push_active(&mut self, a: Active) {
        if self.active.len() < self.active.capacity() {
            self.active.push(a);
        } else {
            // Table full: cut immediately rather than allocate.
            self.send_off(a.slot as usize, a.key, 0);
        }
    }

    fn live_on(&mut self, si: usize, key: u8, vel: u8) {
        self.remove_active_where(0, |a| a.live && a.slot as usize == si && a.key == key);
        self.send_on(si, key, vel, 0);
        self.push_active(Active { slot: si as u16, key, end: f64::INFINITY, live: true });
    }

    fn live_off(&mut self, si: usize, key: u8) {
        self.remove_active_where(0, |a| a.live && a.slot as usize == si && a.key == key);
    }

    fn drain_midi_in(&mut self) {
        loop {
            let Some(m) = self.midi_in.as_mut().and_then(|rx| rx.pop().ok()) else { break };
            if m.len < 3 {
                continue;
            }
            let status = m.bytes[0] & 0xF0;
            let (key, vel) = (m.bytes[1] & 0x7f, m.bytes[2] & 0x7f);
            let on = match status {
                0x90 if vel > 0 => true,
                0x80 | 0x90 => false,
                _ => continue,
            };
            let Some(ch) = self.live_channel else { continue };
            let Some(si) = self.slot_index(ch) else { continue };
            if on { self.live_on(si, key, vel) } else { self.live_off(si, key) }
            if self.recording && self.playing {
                let tick = self.position();
                let time = self.clock as f64 / self.spt;
                let _ = self.events.push(EngineEvent::Recorded { channel: ch, key, vel, tick, time, on });
            }
        }
    }

    // ------------------------------------------------------------------
    // Rendering
    // ------------------------------------------------------------------

    fn render_block(&mut self, n: usize) {
        self.pending.clear();
        if self.playing {
            self.gather(n);
            self.pending.sort_unstable_by_key(|p| (p.offset, p.chan, p.key));
        }
        let mut cur = 0;
        let mut pi = 0;
        while cur < n {
            if self.playing {
                self.fire_offs(cur);
                while pi < self.pending.len() && self.pending[pi].offset <= cur {
                    let p = self.pending[pi];
                    self.fire_on(p, cur);
                    pi += 1;
                }
            }
            // After the note-ons: a chord played at the same instant is complete on the 1st step.
            self.fire_arps(cur);
            let mut next = n;
            if pi < self.pending.len() {
                next = next.min(self.pending[pi].offset);
            }
            if self.playing {
                next = next.min(self.next_off_after(cur, n));
            }
            next = next.min(self.next_arp_after(cur, n));
            if next <= cur {
                next = cur + 1;
            }
            self.render_range(cur, next);
            cur = next;
        }
        self.mix(n);
        self.clock += n as u64;
        if self.playing {
            self.frames += n as u64;
        }
    }

    fn off_offset(&self, end: f64) -> i64 {
        (end * self.spt).ceil() as i64 - self.frames as i64
    }

    fn fire_offs(&mut self, cur: usize) {
        let mut i = 0;
        while i < self.active.len() {
            let a = self.active[i];
            if !a.live && self.off_offset(a.end) <= cur as i64 {
                self.active.swap_remove(i);
                self.send_off(a.slot as usize, a.key, cur);
            } else {
                i += 1;
            }
        }
    }

    fn next_off_after(&self, cur: usize, n: usize) -> usize {
        let mut next = n as i64;
        for a in self.active.iter().filter(|a| !a.live) {
            let o = self.off_offset(a.end);
            if o > cur as i64 && o < next {
                next = o;
            }
        }
        next as usize
    }

    fn fire_on(&mut self, p: Pending, cur: usize) {
        if p.chan == METRO {
            self.click.trigger(p.key == 1, self.sr as f32);
            return;
        }
        let Some(Some(si)) = self.chan_slot.get(p.chan as usize).copied() else { return };
        let si = si as usize;
        // Retrigger of the same key: cut the old one first.
        self.remove_active_where(cur, |a| !a.live && a.slot as usize == si && a.key == p.key);
        self.send_on(si, p.key, p.vel, cur);
        self.push_active(Active { slot: si as u16, key: p.key, end: p.end, live: false });
    }

    fn render_range(&mut self, a: usize, b: usize) {
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if let SlotKind::Audio(inst) = &mut slot.kind {
                inst.process(&mut self.bufs_l[i][a..b], &mut self.bufs_r[i][a..b]);
            }
        }
        self.click.process(&mut self.click_buf[a..b]);
    }

    fn mix(&mut self, n: usize) {
        let any_solo = self.slots.iter().any(|s| s.params.solo);
        self.mix_l[..n].fill(0.0);
        self.mix_r[..n].fill(0.0);
        for (i, s) in self.slots.iter_mut().enumerate() {
            if matches!(s.kind, SlotKind::Midi) {
                continue;
            }
            let p = s.params;
            let audible = !p.mute && (!any_solo || p.solo);
            // Constant-power pan law (-3 dB at center).
            let theta = (p.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
            let (gl, gr) = if audible { (p.volume * theta.cos(), p.volume * theta.sin()) } else { (0.0, 0.0) };
            let (bl, br) = (&self.bufs_l[i][..n], &self.bufs_r[i][..n]);
            for j in 0..n {
                let l = bl[j] * gl;
                let r = br[j] * gr;
                self.mix_l[j] += l;
                self.mix_r[j] += r;
                s.peak_l = s.peak_l.max(l.abs());
                s.peak_r = s.peak_r.max(r.abs());
            }
        }
        for j in 0..n {
            let c = self.click_buf[j] * 0.5;
            self.mix_l[j] = soft_clip(self.mix_l[j] + c);
            self.mix_r[j] = soft_clip(self.mix_r[j] + c);
            self.master_peak.0 = self.master_peak.0.max(self.mix_l[j].abs());
            self.master_peak.1 = self.master_peak.1.max(self.mix_r[j].abs());
        }
        self.level_frames += n;
        if self.level_frames as f64 >= self.sr / 30.0 {
            self.level_frames = 0;
            let (peak_l, peak_r) = self.master_peak;
            let _ = self.events.push(EngineEvent::Master { peak_l, peak_r });
            self.master_peak = (0.0, 0.0);
            for s in self.slots.iter_mut() {
                let _ = self.events.push(EngineEvent::Levels { channel: s.id, peak_l: s.peak_l, peak_r: s.peak_r });
                s.peak_l = 0.0;
                s.peak_r = 0.0;
            }
        }
    }

    /// Collects the note-ons that fall within block `[frames, frames + n)`.
    fn gather(&mut self, n: usize) {
        let Some((l0, l1)) = self.region() else { return };
        let spt = self.spt;
        let f = self.frames as f64;
        // An event at Δ ticks lands on frame ceil(Δ·spt): it is in the block iff
        // Δ ∈ ((f-1)/spt, (f+n-1)/spt].
        let ulo = self.s0 + (f - 1.0) / spt;
        let uhi = self.s0 + (f + n as f64 - 1.0) / spt;
        let graph = self.graph.take();
        if let Some(g) = graph.as_deref() {
            let pat_idx = match self.mode {
                PlayMode::Pattern(id) => g.pattern_index(id),
                PlayMode::Song => None,
            };
            let mut ctx = Gather {
                g,
                song_mode: self.mode == PlayMode::Song,
                pat_idx,
                metronome: self.metronome,
                spt,
                frames: self.frames,
                n,
                out: &mut self.pending,
            };
            // Linear part, before the end of the region.
            if ulo < l1 {
                ctx.query(Window { lo: ulo, hi: uhi, mn: f64::NEG_INFINITY, mx: l1, base: -self.s0 });
            }
            // Repetitions of the region.
            if !self.one_shot && uhi >= l1 {
                let p = l1 - l0;
                if p > 0.0 {
                    let kmin = ((ulo - l1) / p).floor().max(0.0) as i64;
                    let kmax = ((uhi - l1) / p).floor() as i64;
                    for k in kmin..=kmax.min(kmin + 64) {
                        let uk = l1 + k as f64 * p;
                        ctx.query(Window { lo: ulo - uk + l0, hi: uhi - uk + l0, mn: l0, mx: l1, base: uk - l0 - self.s0 });
                    }
                }
            }
        }
        self.graph = graph;
    }
}

/// Query window in song ticks: t ∈ (lo, hi] ∩ [mn, mx), Δ = t + base.
#[derive(Clone, Copy)]
struct Window {
    lo: f64,
    hi: f64,
    mn: f64,
    mx: f64,
    base: f64,
}

struct Gather<'a> {
    g: &'a PlaybackGraph,
    song_mode: bool,
    pat_idx: Option<usize>,
    metronome: bool,
    spt: f64,
    frames: u64,
    n: usize,
    out: &'a mut Vec<Pending>,
}

impl Gather<'_> {
    fn push(&mut self, t: f64, end_t: f64, w: &Window, chan: u16, key: u8, vel: u8) {
        if self.out.len() >= self.out.capacity() {
            return;
        }
        let d = t + w.base;
        let frame = (d * self.spt).ceil() as i64 - self.frames as i64;
        let offset = frame.clamp(0, self.n as i64 - 1) as usize;
        self.out.push(Pending { offset, chan, key, vel, end: end_t + w.base });
    }

    fn query(&mut self, w: Window) {
        if self.metronome {
            let bl = self.g.beat_len as f64;
            let mut b = (((w.lo / bl).floor() + 1.0).max((w.mn / bl).ceil())).max(0.0) as i64;
            loop {
                let t = b as f64 * bl;
                if t > w.hi || t >= w.mx {
                    break;
                }
                if t > w.lo && t >= w.mn {
                    let accent = b % self.g.beats_per_bar.max(1) as i64 == 0;
                    self.push(t, t, &w, METRO, accent as u8, 0);
                }
                b += 1;
            }
        }
        if self.song_mode {
            let g = self.g;
            for clip in &g.song {
                let cs = clip.start as f64;
                if cs > w.hi {
                    break;
                }
                let cl = clip.len as f64;
                let pat = &g.patterns[clip.pattern as usize];
                let llo = w.lo - cs;
                let lhi = w.hi - cs;
                let lmin = (w.mn - cs).max(0.0);
                let lmax = cl.min(w.mx - cs);
                if lhi < lmin || llo >= lmax {
                    continue;
                }
                let plen = pat.length.max(1) as f64;
                let r0 = (llo.max(lmin) / plen).floor().max(0.0) as i64;
                let r1 = (lhi.min(lmax) / plen).floor() as i64;
                let trunc = (cs + cl).min(w.mx);
                for r in r0..=r1.min(r0 + 256) {
                    let off = r as f64 * plen;
                    self.query_pattern(pat, cs + off, llo - off, lhi - off, lmin - off, lmax - off, trunc, &w);
                }
            }
        } else if let Some(pi) = self.pat_idx {
            let pat = &self.g.patterns[pi];
            self.query_pattern(pat, 0.0, w.lo, w.hi, w.mn, w.mx, w.mx, &w);
        }
    }

    /// Pattern events whose local tick is in (lo, hi] ∩ [mn, mx).
    #[allow(clippy::too_many_arguments)]
    fn query_pattern(&mut self, pat: &CompiledPattern, offset: f64, lo: f64, hi: f64, mn: f64, mx: f64, trunc: f64, w: &Window) {
        let evs = &pat.events;
        let start = evs.partition_point(|e| (e.tick as f64) <= lo);
        for e in &evs[start..] {
            let lt = e.tick as f64;
            if lt > hi {
                break;
            }
            if lt < mn || lt >= mx {
                continue;
            }
            let t = offset + lt;
            let end_t = (t + e.len as f64).min(trunc);
            self.push(t, end_t, w, e.channel, e.key, e.vel);
        }
    }
}

fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.9 { x } else { x.signum() * (0.9 + 0.1 * ((a - 0.9) / 0.1).tanh()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::tick_to_frame;
    use crate::graph::compile;
    use std::sync::Mutex;

    type Log = Arc<Mutex<Vec<(u64, bool, u8)>>>;

    /// Instrument that records the exact frame of every note-on/off.
    struct Probe {
        log: Log,
        t: u64,
    }
    impl Instrument for Probe {
        fn note_on(&mut self, key: u8, _vel: u8) {
            self.log.lock().unwrap().push((self.t, true, key));
        }
        fn note_off(&mut self, key: u8) {
            self.log.lock().unwrap().push((self.t, false, key));
        }
        fn all_off(&mut self) {}
        fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
            l.fill(0.0);
            r.fill(0.0);
            self.t += l.len() as u64;
        }
    }

    fn project_with_steps(steps: &[usize], len_steps: u64) -> (Project, ChannelId, PatternId) {
        let mut p = Project::empty();
        let ch = p.add_channel("probe", ChannelKind::Synth(SynthParams::default())).unwrap();
        let pat = p.patterns[0].id;
        let pt = p.pattern_mut(pat).unwrap();
        pt.length = len_steps * STEP;
        pt.notes.insert(ch, steps.iter().map(|&s| Note { start: s as u64 * STEP, len: STEP, key: 60, velocity: 100 }).collect());
        (p, ch, pat)
    }

    fn engine_with_probe(p: &Project, ch: ChannelId, sr: u32, mode: PlayMode) -> (Engine, EngineHandle, Log) {
        let (mut e, h) = Engine::new(sr, None, None);
        let log: Log = Arc::default();
        e.handle_cmd(EngineCmd::SetSlot(ch, SlotKind::Audio(Box::new(Probe { log: log.clone(), t: 0 }))));
        e.handle_cmd(EngineCmd::SwapGraph(Box::new(compile(p))));
        e.handle_cmd(EngineCmd::SetMode(mode));
        e.handle_cmd(EngineCmd::Play);
        (e, h, log)
    }

    fn run(e: &mut Engine, frames: usize, block_sizes: &[usize]) {
        let mut done = 0;
        let mut i = 0;
        while done < frames {
            let n = block_sizes[i % block_sizes.len()].min(frames - done);
            let mut buf = vec![0.0; n * 2];
            e.process(&mut buf, 2, 0.0);
            done += n;
            i += 1;
        }
    }

    fn ons(log: &Log) -> Vec<u64> {
        log.lock().unwrap().iter().filter(|x| x.1).map(|x| x.0).collect()
    }

    #[test]
    fn events_on_exact_frames_any_block_size() {
        for &sr in &[44_100u32, 48_000] {
            for blocks in [&[512usize][..], &[333], &[1], &[100, 1000, 7]] {
                let (p, ch, pat) = project_with_steps(&[0, 3, 4, 8, 15], 16);
                let (mut e, _h, log) = engine_with_probe(&p, ch, sr, PlayMode::Pattern(pat));
                let spt = samples_per_tick(sr as f64, 120.0);
                let one_loop = tick_to_frame((16 * STEP) as f64, spt) as usize;
                run(&mut e, one_loop, blocks);
                let expected: Vec<u64> = [0u64, 3, 4, 8, 15].iter().map(|s| tick_to_frame((s * STEP) as f64, spt)).collect();
                assert_eq!(ons(&log), expected, "sr {sr} blocks {blocks:?}");
            }
        }
    }

    #[test]
    fn pattern_loops_without_duplicates() {
        let (p, ch, pat) = project_with_steps(&[0, 8], 16);
        let (mut e, _h, log) = engine_with_probe(&p, ch, 48_000, PlayMode::Pattern(pat));
        let loop_frames = 16 * STEP as usize * 25;
        run(&mut e, loop_frames * 3, &[500]);
        let half = loop_frames as u64 / 2;
        let expected: Vec<u64> = (0..6).map(|i| i * half).collect();
        assert_eq!(ons(&log), expected);
    }

    #[test]
    fn note_offs_follow_length() {
        let (p, ch, pat) = project_with_steps(&[0, 1], 16);
        let (mut e, _h, log) = engine_with_probe(&p, ch, 48_000, PlayMode::Pattern(pat));
        run(&mut e, 20_000, &[256]);
        let l = log.lock().unwrap().clone();
        let step = STEP * 25;
        // Note 2 starts exactly where note 1 ends: off before on.
        assert_eq!(&l[..4], &[(0, true, 60), (step, false, 60), (step, true, 60), (2 * step, false, 60)]);
    }

    #[test]
    fn graph_swap_keeps_timing() {
        let (p, ch, pat) = project_with_steps(&[0, 4, 8, 12], 16);
        let (mut e, _h, log) = engine_with_probe(&p, ch, 48_000, PlayMode::Pattern(pat));
        let step = STEP * 25;
        run(&mut e, (step * 5) as usize + 17, &[256]);
        // Swap in the same graph (an edit) midway: no duplicates, no shift.
        e.handle_cmd(EngineCmd::SwapGraph(Box::new(compile(&p))));
        // Up to just before the end of the loop.
        run(&mut e, (step * 11) as usize - 18, &[300]);
        assert_eq!(ons(&log), vec![0, 4 * step, 8 * step, 12 * step]);
    }

    #[test]
    fn song_mode_clip_repeats_and_truncates() {
        let (mut p, ch, pat) = project_with_steps(&[0, 2], 4); // 4-step pattern
        // 5-step clip starting at step 8: notes at steps 8, 10, 12 (repeat), then cut.
        p.arrangement.clips.push(Clip { pattern: pat, track: 0, start: 8 * STEP, len: 5 * STEP });
        let (mut e, _h, log) = engine_with_probe(&p, ch, 48_000, PlayMode::Song);
        e.set_one_shot(true);
        let step = STEP * 25;
        run(&mut e, (step * 16) as usize, &[256]);
        let l = log.lock().unwrap().clone();
        let on: Vec<u64> = l.iter().filter(|x| x.1).map(|x| x.0).collect();
        assert_eq!(on, vec![8 * step, 10 * step, 12 * step]);
        // The last note (step 12, 1 step long) ends at step 13 = end of the clip.
        assert_eq!(l.last().unwrap(), &(13 * step, false, 60));
    }

    #[test]
    fn stop_releases_notes() {
        let (mut p, ch, pat) = project_with_steps(&[0], 16);
        p.pattern_mut(pat).unwrap().notes.get_mut(&ch).unwrap()[0].len = 8 * STEP;
        let (mut e, _h, log) = engine_with_probe(&p, ch, 48_000, PlayMode::Pattern(pat));
        run(&mut e, 1000, &[256]);
        e.handle_cmd(EngineCmd::Stop);
        let l = log.lock().unwrap().clone();
        assert_eq!(l.len(), 2);
        assert!(!l[1].1);
    }

    #[test]
    fn metronome_clicks_on_beats() {
        for &(sr, beat) in &[(48_000u32, 24_000usize), (44_100, 22_050)] {
            let mut p = Project::empty();
            p.patterns[0].length = 4 * PPQ as u64;
            let pat = p.patterns[0].id;
            let (mut e, _h) = Engine::new(sr, None, None);
            e.handle_cmd(EngineCmd::SwapGraph(Box::new(compile(&p))));
            e.handle_cmd(EngineCmd::SetMode(PlayMode::Pattern(pat)));
            e.handle_cmd(EngineCmd::SetMetronome(true));
            e.handle_cmd(EngineCmd::Play);
            let frames = beat * 4;
            let mut out = vec![0.0f32; frames * 2];
            for chunk in out.chunks_mut(2 * 401) {
                e.process(chunk, 2, 0.0);
            }
            let left: Vec<f32> = out.iter().step_by(2).copied().collect();
            let onsets: Vec<usize> = (0..frames).filter(|&i| left[i] != 0.0 && (i == 0 || left[i - 1] == 0.0)).collect();
            assert_eq!(onsets, vec![0, beat, 2 * beat, 3 * beat], "sr {sr}");
        }
    }

    fn arp_project(arp: ArpParams, chord: &[u8], len_steps: u64) -> (Project, ChannelId, PatternId) {
        let mut p = Project::empty();
        let ch = p.add_channel("arp", ChannelKind::Synth(SynthParams::default())).unwrap();
        p.channel_mut(ch).unwrap().arp = arp;
        let pat = p.patterns[0].id;
        let pt = p.pattern_mut(pat).unwrap();
        pt.length = 16 * STEP;
        pt.notes.insert(ch, chord.iter().map(|&k| Note { start: 0, len: len_steps * STEP, key: k, velocity: 100 }).collect());
        (p, ch, pat)
    }

    #[test]
    fn arp_up_on_exact_frames_with_gate() {
        let arp = ArpParams { enabled: true, rate: STEP, gate: 0.5, ..Default::default() };
        let (p, ch, pat) = arp_project(arp, &[67, 60, 64], 4);
        let (mut e, _h, log) = engine_with_probe(&p, ch, 48_000, PlayMode::Pattern(pat));
        let step = STEP * 25; // 6000 frames
        run(&mut e, (step * 6) as usize, &[333]);
        let l = log.lock().unwrap().clone();
        let h = step / 2;
        assert_eq!(
            l,
            vec![
                (0, true, 60),
                (h, false, 60),
                (step, true, 64),
                (step + h, false, 64),
                (2 * step, true, 67),
                (2 * step + h, false, 67),
                (3 * step, true, 60),
                (3 * step + h, false, 60),
            ]
        );
    }

    #[test]
    fn arp_octaves_and_chord_direction() {
        let arp = ArpParams { enabled: true, rate: STEP, gate: 1.0, octaves: 2, ..Default::default() };
        let (p, ch, pat) = arp_project(arp, &[60], 4);
        let (mut e, _h, log) = engine_with_probe(&p, ch, 48_000, PlayMode::Pattern(pat));
        run(&mut e, (STEP * 25 * 4) as usize, &[512]);
        let keys: Vec<u8> = log.lock().unwrap().iter().filter(|x| x.1).map(|x| x.2).collect();
        assert_eq!(keys, vec![60, 72, 60, 72]);

        let arp = ArpParams { enabled: true, rate: 2 * STEP, direction: ArpDirection::Chord, chord: ArpChord::Minor, ..Default::default() };
        let (p, ch, pat) = arp_project(arp, &[57], 4);
        let (mut e, _h, log) = engine_with_probe(&p, ch, 48_000, PlayMode::Pattern(pat));
        run(&mut e, (STEP * 25 * 4) as usize, &[512]);
        let on: Vec<(u64, u8)> = log.lock().unwrap().iter().filter(|x| x.1).map(|x| (x.0, x.2)).collect();
        let s2 = 2 * STEP * 25;
        assert_eq!(on, vec![(0, 57), (0, 60), (0, 64), (s2, 57), (s2, 60), (s2, 64)]);
    }

    #[test]
    fn arp_live_preview_and_stop_silences() {
        let arp = ArpParams { enabled: true, rate: STEP, gate: 0.5, ..Default::default() };
        let (p, ch, _pat) = arp_project(arp, &[], 1);
        let (mut e, _h) = Engine::new(48_000, None, None);
        let log: Log = Arc::default();
        e.handle_cmd(EngineCmd::SetSlot(ch, SlotKind::Audio(Box::new(Probe { log: log.clone(), t: 0 }))));
        e.handle_cmd(EngineCmd::SwapGraph(Box::new(compile(&p))));
        // Stopped: the arpeggio still runs on a live note.
        e.handle_cmd(EngineCmd::Preview { channel: ch, key: 60, vel: 100, on: true });
        e.handle_cmd(EngineCmd::Preview { channel: ch, key: 63, vel: 100, on: true });
        run(&mut e, 13_000, &[256]);
        let on: Vec<u8> = log.lock().unwrap().iter().filter(|x| x.1).map(|x| x.2).collect();
        assert_eq!(on, vec![60, 63, 60]);
        e.handle_cmd(EngineCmd::Stop);
        run(&mut e, 20_000, &[256]);
        let l = log.lock().unwrap().clone();
        assert_eq!(l.iter().filter(|x| x.1).count(), 3, "no more notes after Stop");
        assert_eq!(l.iter().filter(|x| !x.1).count(), 3, "all notes released");
    }

    #[test]
    fn arp_toggle_off_releases_notes() {
        let arp = ArpParams { enabled: true, rate: STEP, gate: 1.0, ..Default::default() };
        let (mut p, ch, pat) = arp_project(arp, &[60, 64], 16);
        let (mut e, _h, log) = engine_with_probe(&p, ch, 48_000, PlayMode::Pattern(pat));
        run(&mut e, 3000, &[256]);
        p.channel_mut(ch).unwrap().arp.enabled = false;
        e.handle_cmd(EngineCmd::SwapGraph(Box::new(compile(&p))));
        run(&mut e, 30_000, &[256]);
        let l = log.lock().unwrap().clone();
        let ons = l.iter().filter(|x| x.1).count();
        let offs = l.iter().filter(|x| !x.1).count();
        assert_eq!(ons, 1);
        assert_eq!(offs, 1);
    }

    #[test]
    fn choke_group_cuts_other_channel() {
        struct Counter(Arc<Mutex<u32>>);
        impl Instrument for Counter {
            fn note_on(&mut self, _: u8, _: u8) {}
            fn note_off(&mut self, _: u8) {}
            fn all_off(&mut self) {
                *self.0.lock().unwrap() += 1;
            }
            fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
                l.fill(0.0);
                r.fill(0.0);
            }
        }
        let mut p = Project::empty();
        let a = p.add_channel("a", ChannelKind::Synth(SynthParams::default())).unwrap();
        let b = p.add_channel("b", ChannelKind::Synth(SynthParams::default())).unwrap();
        p.channel_mut(a).unwrap().choke = 1;
        p.channel_mut(b).unwrap().choke = 1;
        let pat = p.patterns[0].id;
        p.pattern_mut(pat).unwrap().notes.insert(a, vec![Note { start: 0, len: STEP, key: 60, velocity: 100 }]);
        let cut = Arc::new(Mutex::new(0));
        let (mut e, _h) = Engine::new(48_000, None, None);
        e.handle_cmd(EngineCmd::SetSlot(a, SlotKind::Audio(Box::new(Counter(Arc::new(Mutex::new(0)))))));
        e.handle_cmd(EngineCmd::SetSlot(b, SlotKind::Audio(Box::new(Counter(cut.clone())))));
        e.handle_cmd(EngineCmd::SwapGraph(Box::new(compile(&p))));
        e.handle_cmd(EngineCmd::SetMode(PlayMode::Pattern(pat)));
        e.handle_cmd(EngineCmd::Play);
        let mut buf = vec![0.0; 512];
        e.process(&mut buf, 2, 0.0);
        assert_eq!(*cut.lock().unwrap(), 1);
    }

    // ------------------------------------------------------------------
    // Mix
    // ------------------------------------------------------------------

    /// Instrument that outputs a constant value on both sides.
    struct Dc(f32);
    impl Instrument for Dc {
        fn note_on(&mut self, _: u8, _: u8) {}
        fn note_off(&mut self, _: u8) {}
        fn all_off(&mut self) {}
        fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
            l.fill(self.0);
            r.fill(self.0);
        }
    }

    /// One `Dc` channel per value; `edit` adjusts the channels before compiling.
    /// Returns the first output frame (left, right), engine stopped.
    fn mix_frame(values: &[f32], edit: impl Fn(&mut [Channel])) -> (f32, f32) {
        let mut p = Project::empty();
        for _ in values {
            p.add_channel("dc", ChannelKind::Synth(SynthParams::default())).unwrap();
        }
        edit(&mut p.channels);
        let (mut e, _h) = Engine::new(48_000, None, None);
        for (c, &v) in p.channels.iter().zip(values) {
            e.handle_cmd(EngineCmd::SetSlot(c.id, SlotKind::Audio(Box::new(Dc(v)))));
        }
        e.handle_cmd(EngineCmd::SwapGraph(Box::new(compile(&p))));
        let mut out = vec![0.0; 64 * 2];
        e.process(&mut out, 2, 0.0);
        assert!(out.chunks(2).all(|f| f == &out[..2]), "constant input, constant output");
        (out[0], out[1])
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn mix_volume_and_constant_power_pan() {
        let set = |vol: f32, pan: f32| move |c: &mut [Channel]| {
            c[0].volume = vol;
            c[0].pan = pan;
        };
        let h = std::f32::consts::FRAC_1_SQRT_2;
        // Center: -3 dB on each side.
        let (l, r) = mix_frame(&[0.5], set(0.8, 0.0));
        assert!(close(l, 0.4 * h) && close(r, 0.4 * h), "{l} {r}");
        // Hard left / hard right: full level on one side only.
        let (l, r) = mix_frame(&[0.5], set(0.8, -1.0));
        assert!(close(l, 0.4) && close(r, 0.0), "{l} {r}");
        let (l, r) = mix_frame(&[0.5], set(0.8, 1.0));
        assert!(close(l, 0.0) && close(r, 0.4), "{l} {r}");
        // Constant power at any position.
        for pan in [-0.7f32, -0.2, 0.3, 0.9] {
            let (l, r) = mix_frame(&[0.5], set(1.0, pan));
            assert!(close(l * l + r * r, 0.25), "pan {pan}: {l} {r}");
            assert_eq!(pan < 0.0, l > r);
        }
        // Out-of-range pan is clamped.
        let (l, r) = mix_frame(&[0.5], set(1.0, -3.0));
        assert!(close(l, 0.5) && close(r, 0.0), "{l} {r}");
        // Volume is linear.
        let (l, _) = mix_frame(&[0.5], set(0.25, -1.0));
        assert!(close(l, 0.125));
    }

    #[test]
    fn mix_mute_and_solo() {
        // Three channels hard left with distinct levels: the sum tells which ones are audible.
        let vals = [0.1, 0.01, 0.001];
        let left = |mute: [bool; 3], solo: [bool; 3]| {
            mix_frame(&vals, move |c: &mut [Channel]| {
                for (i, ch) in c.iter_mut().enumerate() {
                    ch.volume = 1.0;
                    ch.pan = -1.0;
                    ch.mute = mute[i];
                    ch.solo = solo[i];
                }
            })
            .0
        };
        const N: [bool; 3] = [false; 3];
        assert!(close(left(N, N), 0.111));
        assert!(close(left([false, true, false], N), 0.101));
        assert!(close(left(N, [true, false, false]), 0.1));
        assert!(close(left(N, [true, false, true]), 0.101));
        // Mute wins over solo; the other channels stay silenced by the solo.
        assert!(close(left([true, false, false], [true, false, false]), 0.0));
    }

    #[test]
    fn master_soft_clip_bounds_output() {
        for v in [1.0f32, -1.0] {
            let (l, r) = mix_frame(&[v; 20], |c| c.iter_mut().for_each(|ch| ch.volume = 1.0));
            assert!(l.abs() <= 1.0 && r.abs() <= 1.0, "{l} {r}");
            assert!(l.abs() > 0.99 && l.signum() == v.signum(), "{l}");
        }
        // Below the knee (0.9) the signal is untouched.
        let (l, _) = mix_frame(&[0.85], |c| {
            c[0].volume = 1.0;
            c[0].pan = -1.0;
        });
        assert!(close(l, 0.85));
        // Just above the knee: compressed but continuous and monotonic.
        assert!(soft_clip(0.9) == 0.9 && soft_clip(0.95) > 0.9 && soft_clip(0.95) < 0.95);
        assert!(soft_clip(0.95) < soft_clip(1.5) && soft_clip(1.5) < soft_clip(100.0));
        assert_eq!(soft_clip(-0.95), -soft_clip(0.95));
    }

    // ------------------------------------------------------------------
    // MIDI in & recording
    // ------------------------------------------------------------------

    fn midi(on: bool, key: u8, vel: u8) -> MidiInMsg {
        MidiInMsg { bytes: [if on { 0x90 } else { 0x80 }, key, vel], len: 3 }
    }

    /// Engine (stopped) with a MIDI input and a probe on `ch`.
    fn midi_in_engine(p: &Project, ch: ChannelId) -> (Engine, EngineHandle, Log, Producer<MidiInMsg>) {
        let (tx, rx) = RingBuffer::new(64);
        let (mut e, h) = Engine::new(48_000, Some(rx), None);
        let log: Log = Arc::default();
        e.handle_cmd(EngineCmd::SetSlot(ch, SlotKind::Audio(Box::new(Probe { log: log.clone(), t: 0 }))));
        e.handle_cmd(EngineCmd::SwapGraph(Box::new(compile(p))));
        e.handle_cmd(EngineCmd::SetMode(PlayMode::Pattern(p.patterns[0].id)));
        (e, h, log, tx)
    }

    fn recorded(h: &mut EngineHandle) -> Vec<(ChannelId, u8, u8, f64, bool)> {
        let mut v = Vec::new();
        while let Some(ev) = h.poll_event() {
            if let EngineEvent::Recorded { channel, key, vel, tick, on, .. } = ev {
                v.push((channel, key, vel, tick, on));
            }
        }
        v
    }

    #[test]
    fn midi_in_plays_live_channel_while_stopped() {
        let (p, ch, _) = project_with_steps(&[], 16);
        let (mut e, _h, log, mut tx) = midi_in_engine(&p, ch);
        e.handle_cmd(EngineCmd::SetLiveChannel(Some(ch)));
        tx.push(midi(true, 60, 100)).unwrap();
        run(&mut e, 256, &[256]);
        tx.push(midi(true, 64, 90)).unwrap();
        // Note-on with velocity 0 = note-off.
        tx.push(MidiInMsg { bytes: [0x90, 60, 0], len: 3 }).unwrap();
        run(&mut e, 256, &[256]);
        tx.push(midi(false, 64, 0)).unwrap();
        run(&mut e, 256, &[256]);
        assert_eq!(*log.lock().unwrap(), vec![(0, true, 60), (256, true, 64), (256, false, 60), (512, false, 64)]);
        // Changing the live channel releases held notes.
        tx.push(midi(true, 67, 100)).unwrap();
        run(&mut e, 256, &[256]);
        e.handle_cmd(EngineCmd::SetLiveChannel(None));
        assert_eq!(log.lock().unwrap().last(), Some(&(1024, false, 67)));
    }

    #[test]
    fn midi_in_ignored_without_live_channel() {
        let (p, ch, _) = project_with_steps(&[], 16);
        let (mut e, _h, log, mut tx) = midi_in_engine(&p, ch);
        e.handle_cmd(EngineCmd::Play);
        tx.push(midi(true, 60, 100)).unwrap();
        tx.push(midi(false, 60, 0)).unwrap();
        run(&mut e, 256, &[256]);
        assert!(log.lock().unwrap().is_empty());
        // Live channel without an instrument: ignored as well.
        e.handle_cmd(EngineCmd::SetLiveChannel(Some(ChannelId(999))));
        tx.push(midi(true, 62, 100)).unwrap();
        run(&mut e, 256, &[256]);
        assert!(log.lock().unwrap().is_empty());
        // Non-note messages and short messages are ignored on a valid live channel.
        e.handle_cmd(EngineCmd::SetLiveChannel(Some(ch)));
        tx.push(MidiInMsg { bytes: [0xB0, 1, 64], len: 3 }).unwrap();
        tx.push(MidiInMsg { bytes: [0x90, 60, 100], len: 2 }).unwrap();
        run(&mut e, 256, &[256]);
        assert!(log.lock().unwrap().is_empty());
    }

    #[test]
    fn recording_needs_recording_and_playing() {
        let (p, ch, _) = project_with_steps(&[], 16);
        let (mut e, mut h, _log, mut tx) = midi_in_engine(&p, ch);
        e.handle_cmd(EngineCmd::SetLiveChannel(Some(ch)));
        // Recording armed but stopped: nothing.
        e.handle_cmd(EngineCmd::SetRecording(true));
        tx.push(midi(true, 60, 100)).unwrap();
        tx.push(midi(false, 60, 0)).unwrap();
        run(&mut e, 256, &[256]);
        assert!(recorded(&mut h).is_empty());
        // Playing but not recording: nothing.
        e.handle_cmd(EngineCmd::SetRecording(false));
        e.handle_cmd(EngineCmd::Play);
        tx.push(midi(true, 60, 100)).unwrap();
        tx.push(midi(false, 60, 0)).unwrap();
        run(&mut e, 250, &[250]);
        assert!(recorded(&mut h).is_empty());
        // Both: note-on and note-off, at the position of the callback (250 frames = 10 ticks).
        e.handle_cmd(EngineCmd::SetRecording(true));
        tx.push(midi(true, 64, 90)).unwrap();
        tx.push(midi(false, 64, 0)).unwrap();
        run(&mut e, 256, &[256]);
        assert_eq!(recorded(&mut h), vec![(ch, 64, 90, 10.0, true), (ch, 64, 0, 10.0, false)]);
    }

    #[test]
    fn recorded_tick_wraps_at_loop_boundary() {
        let (p, ch, _) = project_with_steps(&[], 16); // 3840 ticks = 96000 frames
        let (mut e, mut h, _log, mut tx) = midi_in_engine(&p, ch);
        e.handle_cmd(EngineCmd::SetLiveChannel(Some(ch)));
        e.handle_cmd(EngineCmd::SetRecording(true));
        e.handle_cmd(EngineCmd::Play);
        run(&mut e, 120_000, &[500]); // 1.25 loops
        tx.push(midi(true, 60, 100)).unwrap();
        run(&mut e, 96_000 - 120_000 % 96_000, &[500]); // up to the end of loop 2
        tx.push(midi(false, 60, 0)).unwrap();
        run(&mut e, 100, &[100]);
        assert_eq!(recorded(&mut h), vec![(ch, 60, 100, 960.0, true), (ch, 60, 0, 0.0, false)]);
    }

    #[test]
    fn recorded_time_does_not_wrap() {
        // Held for 1.5 loops: the position wraps, the time keeps the real duration.
        let (p, ch, _) = project_with_steps(&[], 16); // 3840 ticks = 96000 frames
        let (mut e, mut h, _log, mut tx) = midi_in_engine(&p, ch);
        e.handle_cmd(EngineCmd::SetLiveChannel(Some(ch)));
        e.handle_cmd(EngineCmd::SetRecording(true));
        e.handle_cmd(EngineCmd::Play);
        let mut times = Vec::new();
        for (on, frames) in [(true, 144_000), (false, 100)] {
            tx.push(midi(on, 60, if on { 100 } else { 0 })).unwrap();
            run(&mut e, frames, &[500]);
            while let Some(ev) = h.poll_event() {
                if let EngineEvent::Recorded { time, .. } = ev {
                    times.push(time);
                }
            }
        }
        assert_eq!(times.len(), 2);
        assert!((times[1] - times[0] - 5760.0).abs() < 1.0, "{times:?}");
    }

    // ------------------------------------------------------------------
    // MIDI out
    // ------------------------------------------------------------------

    /// One MIDI out channel per `(midi channel, out)`, each with `notes`.
    fn midi_out_project(chans: &[(u8, u8)], notes: &[Note]) -> (Project, Vec<ChannelId>) {
        let mut p = Project::empty();
        let pat = p.patterns[0].id;
        let ids: Vec<ChannelId> = chans
            .iter()
            .map(|&(channel, out)| p.add_channel("m", ChannelKind::MidiOut { channel, out }).unwrap())
            .collect();
        for &id in &ids {
            p.pattern_mut(pat).unwrap().notes.insert(id, notes.to_vec());
        }
        (p, ids)
    }

    fn midi_out_engine(p: &Project, ids: &[ChannelId]) -> (Engine, EngineHandle, Consumer<MidiOutMsg>) {
        let (tx, rx) = RingBuffer::new(256);
        let (mut e, h) = Engine::new(48_000, None, Some(tx));
        for &id in ids {
            e.handle_cmd(EngineCmd::SetSlot(id, SlotKind::Midi));
        }
        e.handle_cmd(EngineCmd::SwapGraph(Box::new(compile(p))));
        e.handle_cmd(EngineCmd::SetMode(PlayMode::Pattern(p.patterns[0].id)));
        e.handle_cmd(EngineCmd::Play);
        (e, h, rx)
    }

    fn callback(e: &mut Engine, frames: usize, latency: f64) {
        let mut buf = vec![0.0; frames * 2];
        e.process(&mut buf, 2, latency);
    }

    fn drain(rx: &mut Consumer<MidiOutMsg>) -> Vec<(u8, [u8; 3], f64)> {
        let mut v = Vec::new();
        while let Ok(m) = rx.pop() {
            v.push((m.port, m.bytes, m.delay_secs));
        }
        v
    }

    fn assert_msgs(got: &[(u8, [u8; 3], f64)], expected: &[(u8, [u8; 3], f64)]) {
        assert_eq!(got.len(), expected.len(), "{got:?}");
        for (g, x) in got.iter().zip(expected) {
            assert_eq!((g.0, g.1), (x.0, x.1), "{got:?}");
            assert!((g.2 - x.2).abs() < 1e-9, "delay {} != {}", g.2, x.2);
        }
    }

    #[test]
    fn midi_out_channel_port_and_delay() {
        // Tick 60 = frame 1500, ends at tick 80 = frame 2000.
        let note = Note { start: 60, len: 20, key: 62, velocity: 100 };
        let (p, ids) = midi_out_project(&[(3, 1)], &[note]);
        let sr = 48_000.0;
        // One large callback (split into 512-frame sub-blocks): delay = latency + offset in the callback.
        let (mut e, _h, mut rx) = midi_out_engine(&p, &ids);
        callback(&mut e, 2048, 0.01);
        assert_msgs(&drain(&mut rx), &[(1, [0x93, 62, 100], 0.01 + 1500.0 / sr), (1, [0x83, 62, 0], 0.01 + 2000.0 / sr)]);
        // Smaller callbacks: the offset is relative to the start of each callback.
        let (mut e, _h, mut rx) = midi_out_engine(&p, &ids);
        let mut got = Vec::new();
        for _ in 0..3 {
            callback(&mut e, 1000, 0.02);
            got.extend(drain(&mut rx));
        }
        assert_msgs(&got, &[(1, [0x93, 62, 100], 0.02 + 500.0 / sr), (1, [0x83, 62, 0], 0.02)]);
    }

    #[test]
    fn midi_out_note_off_before_retrigger() {
        // Two back-to-back steps: off then on at the same frame.
        let notes = [0, 1].map(|s| Note { start: s * STEP, len: STEP, key: 60, velocity: 80 });
        let (p, ids) = midi_out_project(&[(0, 0)], &notes);
        let (mut e, _h, mut rx) = midi_out_engine(&p, &ids);
        callback(&mut e, 12_288, 0.0);
        let step = (STEP * 25) as f64 / 48_000.0;
        assert_msgs(
            &drain(&mut rx),
            &[(0, [0x90, 60, 80], 0.0), (0, [0x80, 60, 0], step), (0, [0x90, 60, 80], step), (0, [0x80, 60, 0], 2.0 * step)],
        );
    }

    #[test]
    fn midi_out_stop_sends_note_off() {
        let note = Note { start: 0, len: 8 * STEP, key: 48, velocity: 127 };
        let (p, ids) = midi_out_project(&[(9, 2)], &[note]);
        let (mut e, mut h, mut rx) = midi_out_engine(&p, &ids);
        callback(&mut e, 1024, 0.005);
        assert_msgs(&drain(&mut rx), &[(2, [0x99, 48, 127], 0.005)]);
        h.send(EngineCmd::Stop);
        callback(&mut e, 512, 0.005);
        assert_msgs(&drain(&mut rx), &[(2, [0x89, 48, 0], 0.005)]);
        // Nothing more afterwards.
        callback(&mut e, 48_000, 0.005);
        assert!(drain(&mut rx).is_empty());
    }

    #[test]
    fn midi_out_muted_channel_sends_no_note_on() {
        let note = Note { start: 0, len: STEP, key: 60, velocity: 100 };
        let (mut p, ids) = midi_out_project(&[(0, 0)], &[note]);
        p.channel_mut(ids[0]).unwrap().mute = true;
        let (mut e, _h, mut rx) = midi_out_engine(&p, &ids);
        callback(&mut e, 12_000, 0.0);
        assert!(drain(&mut rx).iter().all(|m| m.1[0] & 0xF0 != 0x90));
    }

    #[test]
    fn midi_out_respects_solo() {
        let note = Note { start: 0, len: STEP, key: 60, velocity: 100 };
        let (mut p, ids) = midi_out_project(&[(0, 0), (1, 0)], &[note]);
        p.channel_mut(ids[0]).unwrap().solo = true;
        let (mut e, _h, mut rx) = midi_out_engine(&p, &ids);
        callback(&mut e, 12_000, 0.0);
        let on: Vec<u8> = drain(&mut rx).iter().filter(|m| m.1[0] & 0xF0 == 0x90).map(|m| m.1[0]).collect();
        // Channel 1 is not soloed: only channel 0 plays.
        assert_eq!(on, vec![0x90]);
    }

    #[test]
    fn midi_out_channel_change_releases_on_old_channel() {
        // As the app does it: SetSlot (kind changed), then SwapGraph.
        let note = Note { start: 0, len: 8 * STEP, key: 60, velocity: 100 };
        let (mut p, ids) = midi_out_project(&[(0, 0)], &[note]);
        let (mut e, mut h, mut rx) = midi_out_engine(&p, &ids);
        callback(&mut e, 1000, 0.0);
        p.channel_mut(ids[0]).unwrap().kind = ChannelKind::MidiOut { channel: 5, out: 1 };
        h.send(EngineCmd::SetSlot(ids[0], SlotKind::Midi));
        h.send(EngineCmd::SwapGraph(Box::new(compile(&p))));
        callback(&mut e, 512, 0.0);
        h.send(EngineCmd::Stop);
        callback(&mut e, 512, 0.0);
        let msgs: Vec<(u8, [u8; 3])> = drain(&mut rx).iter().map(|m| (m.0, m.1)).collect();
        assert_eq!(msgs, vec![(0, [0x90, 60, 100]), (0, [0x80, 60, 0])]);
    }

    // ------------------------------------------------------------------
    // Transport
    // ------------------------------------------------------------------

    #[test]
    fn pause_keeps_position_and_resumes() {
        let (mut p, ch, pat) = project_with_steps(&[0, 4], 16);
        let n = p.pattern_mut(pat).unwrap().notes.get_mut(&ch).unwrap();
        n[0].len = 8 * STEP; // still sounding at the pause
        n[1].key = 64;
        let (mut e, mut h, log) = engine_with_probe(&p, ch, 48_000, PlayMode::Pattern(pat));
        run(&mut e, 10_000, &[256]);
        h.send(EngineCmd::Pause);
        run(&mut e, 1, &[1]); // the Probe clock advances by 1 frame while paused
        assert!(!h.shared.playing());
        assert_eq!(e.position(), 400.0);
        assert_eq!(h.shared.playhead(), 400.0);
        run(&mut e, 5_000, &[256]);
        assert_eq!(e.position(), 400.0, "no movement while paused");
        e.handle_cmd(EngineCmd::Play);
        // Step 4 = tick 960: 560 ticks = 14000 frames after the resume (at t = 15001).
        run(&mut e, 20_000, &[256]);
        assert_eq!(*log.lock().unwrap(), vec![(0, true, 60), (10_000, false, 60), (29_001, true, 64)]);
        assert_eq!(e.position(), 400.0 + 20_000.0 / 25.0);
    }

    #[test]
    fn stop_returns_to_region_start() {
        let (p, ch, pat) = project_with_steps(&[0], 16);
        let (mut e, _h, log) = engine_with_probe(&p, ch, 48_000, PlayMode::Pattern(pat));
        run(&mut e, 30_000, &[256]);
        e.handle_cmd(EngineCmd::Stop);
        assert_eq!(e.position(), 0.0);
        e.handle_cmd(EngineCmd::Play);
        run(&mut e, 100, &[100]);
        assert_eq!(ons(&log), vec![0, 30_000]);
        // Song mode with a loop: Stop goes back to the loop start.
        let (mut p, ch, pat) = project_with_steps(&[0], 16);
        p.arrangement.clips.push(Clip { pattern: pat, track: 0, start: 0, len: 32 * STEP });
        let (mut e, _h, _log) = engine_with_probe(&p, ch, 48_000, PlayMode::Song);
        e.handle_cmd(EngineCmd::SetLoop(Some((4 * STEP, 8 * STEP))));
        run(&mut e, 10_000, &[256]);
        e.handle_cmd(EngineCmd::Stop);
        assert_eq!(e.position(), (4 * STEP) as f64);
    }

    #[test]
    fn seek_moves_and_releases_notes() {
        let (mut p, ch, pat) = project_with_steps(&[0, 8], 16);
        let n = p.pattern_mut(pat).unwrap().notes.get_mut(&ch).unwrap();
        n[0].len = 8 * STEP;
        n[1].key = 64;
        let (mut e, _h, log) = engine_with_probe(&p, ch, 48_000, PlayMode::Pattern(pat));
        run(&mut e, 1000, &[256]);
        e.handle_cmd(EngineCmd::Seek((8 * STEP) as f64));
        assert_eq!(e.position(), (8 * STEP) as f64);
        // The note at the seek target plays right away.
        run(&mut e, 1000, &[256]);
        assert_eq!(*log.lock().unwrap(), vec![(0, true, 60), (1000, false, 60), (1000, true, 64)]);
        // Seek past the end of the pattern: back to its start.
        e.handle_cmd(EngineCmd::Seek(1e9));
        assert_eq!(e.position(), 0.0);
        // Seek while stopped.
        e.handle_cmd(EngineCmd::Stop);
        e.handle_cmd(EngineCmd::Seek(123.0));
        assert_eq!(e.position(), 123.0);
    }

    #[test]
    fn song_loop_repeats_selection() {
        let (mut p, ch, pat) = project_with_steps(&[0, 2, 4, 6, 8, 10, 12, 14], 16);
        p.arrangement.clips.push(Clip { pattern: pat, track: 0, start: 0, len: 16 * STEP });
        let (mut e, _h, log) = engine_with_probe(&p, ch, 48_000, PlayMode::Song);
        e.handle_cmd(EngineCmd::SetLoop(Some((4 * STEP, 8 * STEP))));
        e.handle_cmd(EngineCmd::Stop);
        e.handle_cmd(EngineCmd::Play);
        let step = STEP * 25;
        run(&mut e, (12 * step) as usize, &[333]);
        // Steps 4, 6, then back to 4, 6...
        assert_eq!(ons(&log), (0..6).map(|i| i * 2 * step).collect::<Vec<_>>());
        assert_eq!(e.position(), (4 * STEP) as f64);
        // Loop removed: the song continues past step 8.
        e.handle_cmd(EngineCmd::SetLoop(None));
        run(&mut e, (6 * step) as usize, &[333]);
        assert_eq!(ons(&log)[6..], [12 * step, 14 * step, 16 * step]);
    }

    #[test]
    fn mode_change_restarts_at_region_start() {
        let (mut p, ch, pat_a) = project_with_steps(&[0], 16);
        let pat_b = p.add_pattern("B", 16 * STEP);
        p.pattern_mut(pat_b).unwrap().notes.insert(
            ch,
            vec![Note { start: 0, len: STEP, key: 62, velocity: 100 }, Note { start: 8 * STEP, len: STEP, key: 64, velocity: 100 }],
        );
        let (mut e, _h, log) = engine_with_probe(&p, ch, 48_000, PlayMode::Pattern(pat_a));
        run(&mut e, 5000, &[256]);
        e.handle_cmd(EngineCmd::SetMode(PlayMode::Pattern(pat_b)));
        assert_eq!(e.position(), 0.0);
        run(&mut e, 50_000, &[256]);
        assert_eq!(
            *log.lock().unwrap(),
            vec![(0, true, 60), (5000, false, 60), (5000, true, 62), (11_000, false, 62), (53_000, true, 64)]
        );
        // Same mode again: no restart.
        e.handle_cmd(EngineCmd::SetMode(PlayMode::Pattern(pat_b)));
        assert_eq!(e.position(), 50_000.0 / 25.0);
    }

    #[test]
    fn synth_params_change_keeps_note_sounding() {
        let mut p = Project::empty();
        let ch = p.add_channel("s", ChannelKind::Synth(SynthParams::default())).unwrap();
        let (mut e, mut h) = Engine::new(48_000, None, None);
        e.handle_cmd(EngineCmd::SetSlot(ch, SlotKind::Audio(Box::new(crate::synth::Synth::new(SynthParams::default(), 48_000)))));
        e.handle_cmd(EngineCmd::SwapGraph(Box::new(compile(&p))));
        e.handle_cmd(EngineCmd::Preview { channel: ch, key: 48, vel: 100, on: true });
        let peak = |e: &mut Engine| {
            let mut out = vec![0.0f32; 4800 * 2];
            e.process(&mut out, 2, 0.0);
            out[4800..].iter().fold(0.0f32, |m, x| m.max(x.abs()))
        };
        let before = peak(&mut e);
        assert!(before > 0.01);
        h.send(EngineCmd::SetSynthParams(ch, SynthParams { cutoff: 3000.0, osc2_detune: 7.0, ..SynthParams::default() }));
        for _ in 0..5 {
            assert!(peak(&mut e) > 0.5 * before, "note cut by the parameter change");
        }
        h.send(EngineCmd::Preview { channel: ch, key: 48, vel: 0, on: false });
        for _ in 0..10 {
            peak(&mut e);
        }
        assert!(peak(&mut e) < 1e-4);
    }
}
