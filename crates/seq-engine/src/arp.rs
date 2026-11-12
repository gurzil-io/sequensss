//! Real-time arpeggiator state (one per channel). Fixed-size arrays: no allocation.

use seq_model::arp::{ARP_MAX_HELD, ArpParams, ArpPool};

pub const ARP_MAX_SOUNDING: usize = 64;

pub(crate) struct ArpState {
    /// Held notes (key, velocity), in press order.
    held: [(u8, u8); ARP_MAX_HELD],
    pub held_n: usize,
    /// Keys physically pressed (for hold mode).
    phys: u32,
    /// Hold mode: everything was released, the next press replaces the chord.
    latched: bool,
    pub pool: ArpPool,
    dirty: bool,
    pub running: bool,
    pub step: u64,
    /// Next step, in engine clock frames.
    pub next: f64,
    /// Currently sounding notes: (key, end in clock frames).
    pub sounding: [(u8, f64); ARP_MAX_SOUNDING],
    pub sounding_n: usize,
    pub rng: u32,
    pub last_rand: u32,
}

impl Default for ArpState {
    fn default() -> Self {
        ArpState {
            held: [(0, 0); ARP_MAX_HELD],
            held_n: 0,
            phys: 0,
            latched: false,
            pool: ArpPool::default(),
            dirty: true,
            running: false,
            step: 0,
            next: 0.0,
            sounding: [(0, 0.0); ARP_MAX_SOUNDING],
            sounding_n: 0,
            rng: 0x2545_f491,
            last_rand: 0,
        }
    }
}

impl ArpState {
    /// Returns `true` if the arpeggio starts (no note was held).
    pub fn press(&mut self, key: u8, vel: u8, p: &ArpParams) -> bool {
        if p.hold && self.latched {
            self.held_n = 0;
            self.latched = false;
        }
        self.phys += 1;
        let was_empty = self.held_n == 0;
        if let Some(i) = self.held[..self.held_n].iter().position(|h| h.0 == key) {
            self.held[i].1 = vel;
        } else if self.held_n < ARP_MAX_HELD {
            self.held[self.held_n] = (key, vel);
            self.held_n += 1;
        }
        self.dirty = true;
        was_empty
    }

    pub fn release(&mut self, key: u8, p: &ArpParams) {
        self.phys = self.phys.saturating_sub(1);
        if p.hold {
            if self.phys == 0 {
                self.latched = true;
            }
            return;
        }
        if let Some(i) = self.held[..self.held_n].iter().position(|h| h.0 == key) {
            for j in i..self.held_n - 1 {
                self.held[j] = self.held[j + 1];
            }
            self.held_n -= 1;
            self.dirty = true;
        }
        if self.held_n == 0 {
            self.running = false;
            self.step = 0;
        }
    }

    pub fn rebuild_if_needed(&mut self, p: &ArpParams) {
        if self.dirty {
            self.pool.build(&self.held[..self.held_n], p);
            self.dirty = false;
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Forgets held notes (sounding notes are handled by the engine).
    pub fn reset_held(&mut self) {
        self.held_n = 0;
        self.phys = 0;
        self.latched = false;
        self.running = false;
        self.step = 0;
        self.dirty = true;
    }

    pub fn push_sounding(&mut self, key: u8, end: f64) {
        if self.sounding_n < ARP_MAX_SOUNDING {
            self.sounding[self.sounding_n] = (key, end);
            self.sounding_n += 1;
        }
    }

    pub fn remove_sounding(&mut self, i: usize) {
        self.sounding_n -= 1;
        self.sounding[i] = self.sounding[self.sounding_n];
    }
}
