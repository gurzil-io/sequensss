//! MIDI in/out. Backend: midir (WinMM on Windows); on Android, `MidiManager` through
//! the JNI bridge `com.sequensss.app.MidiBridge` (midi_android.rs).
//!
//! - MIDI in: the backend callback pushes messages into an `rtrb` read by the engine.
//! - MIDI out: sending is not RT-safe → a dedicated thread reads an `rtrb<MidiOutMsg>`
//!   and sends each message at its time (audio latency + offset within the buffer), see
//!   midi_sched.rs.

use rtrb::{Consumer, Producer};
use seq_engine::{MidiInMsg, MidiOutMsg, midi_msg_len};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{JoinHandle, Thread};

#[cfg(not(target_os = "android"))]
#[path = "midi_midir.rs"]
mod backend;
#[cfg(target_os = "android")]
#[path = "midi_android.rs"]
mod backend;

#[path = "midi_sched.rs"]
mod sched;

use backend::OutConn;
use sched::Scheduler;
pub use backend::{connect_input, devices_generation, list_in_ports, list_out_ports};

/// Shared producer to the engine. The lock is only taken in the MIDI callback
/// (MIDI thread, not the audio thread): never any contention with audio.
pub type MidiInTx = Arc<Mutex<Producer<MidiInMsg>>>;

pub struct MidiInConn {
    conn: backend::InConn,
    pub port: String,
}

impl MidiInConn {
    pub fn close(self) {
        self.conn.close();
    }
}

/// Pushes a received MIDI message to the engine (called by the backend).
fn push_in(tx: &MidiInTx, msg: &[u8]) {
    if msg.is_empty() || msg.len() > 3 {
        return;
    }
    let mut bytes = [0u8; 3];
    bytes[..msg.len()].copy_from_slice(msg);
    if let Ok(mut tx) = tx.lock() {
        let _ = tx.push(MidiInMsg { bytes, len: msg.len() as u8, time: Some(std::time::Instant::now()) });
    }
}

/// Splits a raw MIDI byte stream (Android delivers USB packets: several messages per
/// packet, running status, messages split across two packets) into complete channel
/// messages (0x80–0xEF). SysEx, system and real-time messages are ignored.
#[derive(Default)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // midir already delivers one message at a time
pub(crate) struct MidiParser {
    /// Current channel status (running status), 0 if none.
    running: u8,
    buf: [u8; 3],
    len: usize,
    /// Data bytes being skipped (SysEx or system common message in progress).
    skipping: bool,
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
impl MidiParser {
    fn data_len(status: u8) -> usize {
        match status & 0xF0 {
            0xC0 | 0xD0 => 1,
            _ => 2,
        }
    }

    pub(crate) fn feed(&mut self, data: &[u8], mut out: impl FnMut(&[u8])) {
        for &b in data {
            if b >= 0xF8 {
                continue; // real-time: may appear anywhere, does not affect state
            }
            if b >= 0x80 {
                self.len = 0;
                if b < 0xF0 {
                    self.running = b;
                    self.skipping = false;
                } else {
                    // SysEx / system common: cancels running status, data is skipped.
                    self.running = 0;
                    self.skipping = b != 0xF7 && b != 0xF6;
                }
                continue;
            }
            if self.skipping || self.running == 0 {
                continue;
            }
            if self.len == 0 {
                self.buf[0] = self.running;
                self.len = 1;
            }
            self.buf[self.len] = b;
            self.len += 1;
            if self.len == 1 + Self::data_len(self.running) {
                out(&self.buf[..self.len]);
                self.len = 0;
            }
        }
    }
}

enum Ctl {
    /// Port of each output (OUT 1, OUT 2…). Several outputs may share a port.
    SetPorts(Vec<Option<String>>),
    SetRing(Consumer<MidiOutMsg>),
    /// Closes all ports (device unplugged/replugged); the next `SetPorts` reopens them.
    Reset,
    Quit,
}

/// MIDI out sending thread.
pub struct MidiOutThread {
    ctl: Sender<Ctl>,
    join: Option<JoinHandle<()>>,
    /// Last error (opening a port), read by the UI.
    pub error: Arc<Mutex<Option<String>>>,
    /// Ports actually open (names), read by the UI for the indicators.
    pub open: Arc<Mutex<Vec<String>>>,
    ports: Vec<Option<String>>,
}

impl MidiOutThread {
    pub fn spawn() -> Self {
        let (ctl, rx) = mpsc::channel();
        let error = Arc::new(Mutex::new(None));
        let open = Arc::new(Mutex::new(Vec::new()));
        let mut ports = Ports { rx, conns: Vec::new(), outs: Vec::new(), error: error.clone(), open: open.clone() };
        let join = std::thread::Builder::new()
            .name("midi-out".into())
            .spawn(move || sched::run(&mut ports))
            .ok();
        MidiOutThread { ctl, join, error, open, ports: Vec::new() }
    }

    /// The thread to unpark when messages are pushed into the ring (`Engine::set_midi_out_waker`).
    pub fn waker(&self) -> Option<Thread> {
        self.join.as_ref().map(|j| j.thread().clone())
    }

    fn send_ctl(&self, c: Ctl) {
        let _ = self.ctl.send(c);
        if let Some(j) = &self.join {
            j.thread().unpark();
        }
    }

    /// Sends nothing if the configuration has not changed.
    pub fn set_ports(&mut self, ports: &[Option<String>]) {
        if self.ports == ports {
            return;
        }
        self.ports = ports.to_vec();
        self.send_ctl(Ctl::SetPorts(self.ports.clone()));
    }

    /// After a device change: closes everything; the current config will be resent
    /// (and the ports reopened) on the next `set_ports`.
    pub fn reset(&mut self) {
        self.ports.clear();
        self.send_ctl(Ctl::Reset);
    }

    pub fn is_open(&self, port: &str) -> bool {
        self.open.lock().map(|o| o.iter().any(|p| p == port)).unwrap_or(false)
    }

    /// New ring (each time the engine is recreated).
    pub fn set_ring(&self, rx: Consumer<MidiOutMsg>) {
        self.send_ctl(Ctl::SetRing(rx));
    }

    pub fn take_error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|mut e| e.take())
    }
}

impl Drop for MidiOutThread {
    fn drop(&mut self) {
        self.send_ctl(Ctl::Quit);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// State of the MIDI out thread, apart from the scheduler.
struct Ports {
    rx: Receiver<Ctl>,
    /// One connection per physical port, shared by the outputs targeting it.
    conns: Vec<(String, OutConn)>,
    outs: Vec<Option<String>>,
    error: Arc<Mutex<Option<String>>>,
    open: Arc<Mutex<Vec<String>>>,
}

impl Ports {
    fn publish(&self) {
        if let Ok(mut o) = self.open.lock() {
            *o = self.conns.iter().map(|c| c.0.clone()).collect();
        }
    }
}

impl sched::Output for Ports {
    fn control(&mut self, sched: &mut Scheduler) -> bool {
        while let Ok(c) = self.rx.try_recv() {
            match c {
                Ctl::Quit => {
                    for (_, c) in self.conns.iter_mut() {
                        all_notes_off(c);
                    }
                    return false;
                }
                Ctl::SetRing(r) => sched.set_ring(r),
                Ctl::Reset => {
                    sched.clear();
                    self.conns.clear();
                    self.outs.clear();
                    self.publish();
                }
                Ctl::SetPorts(ports) => {
                    sched.clear();
                    // Close ports no longer in use (after an all-notes-off).
                    let mut i = 0;
                    while i < self.conns.len() {
                        if ports.iter().flatten().any(|p| *p == self.conns[i].0) {
                            i += 1;
                        } else {
                            let (_, mut c) = self.conns.remove(i);
                            all_notes_off(&mut c);
                        }
                    }
                    // Open the new ones.
                    for name in ports.iter().flatten() {
                        if self.conns.iter().any(|c| c.0 == *name) {
                            continue;
                        }
                        match backend::open_out(name) {
                            Ok(c) => self.conns.push((name.clone(), c)),
                            Err(e) => {
                                if let Ok(mut g) = self.error.lock() {
                                    *g = Some(e.to_string());
                                }
                            }
                        }
                    }
                    self.outs = ports;
                    self.publish();
                }
            }
        }
        true
    }

    fn send(&mut self, port: u8, bytes: &[u8; 3]) {
        let Some(Some(name)) = self.outs.get(port as usize) else { return };
        if let Some((_, c)) = self.conns.iter_mut().find(|c| c.0 == *name) {
            // Real-time messages are 1 byte, SPP 3, notes 3: the padding is not sent.
            let _ = c.send(&bytes[..midi_msg_len(bytes[0])]);
        }
    }
}

fn all_notes_off(c: &mut OutConn) {
    for ch in 0..16u8 {
        let _ = c.send(&[0xB0 | ch, 123, 0]);
    }
}

#[cfg(test)]
mod tests {
    use super::MidiParser;

    fn parse(chunks: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut p = MidiParser::default();
        let mut out = Vec::new();
        for c in chunks {
            p.feed(c, |m| out.push(m.to_vec()));
        }
        out
    }

    #[test]
    fn several_messages_and_running_status() {
        let out = parse(&[&[0x90, 60, 100, 62, 90, 0x80, 60, 0, 0xC1, 5]]);
        assert_eq!(out, vec![vec![0x90, 60, 100], vec![0x90, 62, 90], vec![0x80, 60, 0], vec![0xC1, 5]]);
    }

    #[test]
    fn message_split_across_packets_and_realtime_inside() {
        let out = parse(&[&[0x90, 60], &[0xF8, 100]]);
        assert_eq!(out, vec![vec![0x90, 60, 100]]);
    }

    #[test]
    fn sysex_and_system_common_are_skipped() {
        let out = parse(&[&[0xF0, 1, 2, 3], &[4, 0xF7, 0xF2, 1, 2, 5, 0x90, 64, 1]]);
        assert_eq!(out, vec![vec![0x90, 64, 1]]);
    }
}
