//! MIDI out scheduler: keeps the messages pushed by the engine until their deadline.
//!
//! The thread sleeps until the earliest deadline (`park_timeout`), then spins for the last
//! `SPIN` to beat the OS sleep overshoot. With nothing pending it parks with no timeout.
//! The engine unparks it after each callback that pushed messages, the app after each
//! control message.

use rtrb::Consumer;
use seq_engine::MidiOutMsg;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Busy-wait before a deadline: `park_timeout` often wakes up 0.1–2 ms late.
const SPIN: Duration = Duration::from_micros(500);

/// Messages are sent at most this long after the engine produced them.
const MAX_DELAY_SECS: f64 = 1.0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Pending {
    pub at: Instant,
    pub port: u8,
    pub bytes: [u8; 3],
}

/// Pending messages, sorted by deadline; equal deadlines stay in push order.
#[derive(Default)]
pub(crate) struct Scheduler {
    ring: Option<Consumer<MidiOutMsg>>,
    queue: VecDeque<Pending>,
}

/// Note on/off: (channel, key).
fn note_key(bytes: &[u8; 3]) -> Option<(u8, u8)> {
    matches!(bytes[0] & 0xF0, 0x80 | 0x90).then_some((bytes[0] & 0x0F, bytes[1]))
}

fn insert(queue: &mut VecDeque<Pending>, mut p: Pending) {
    // A note on/off never overtakes an earlier one for the same key and port (the
    // latency reported by the audio driver may jitter from one callback to the next).
    if let Some(k) = note_key(&p.bytes) {
        for q in queue.iter().rev() {
            if q.port == p.port && note_key(&q.bytes) == Some(k) {
                p.at = p.at.max(q.at);
                break;
            }
        }
    }
    // Usually at the end: deadlines mostly arrive in increasing order.
    let i = queue.partition_point(|q| q.at <= p.at);
    queue.insert(i, p);
}

impl Scheduler {
    pub fn set_ring(&mut self, ring: Consumer<MidiOutMsg>) {
        self.ring = Some(ring);
    }

    pub fn clear(&mut self) {
        self.queue.clear();
    }

    /// Moves the messages waiting in the ring into the queue.
    pub fn pull(&mut self) {
        let Some(ring) = self.ring.as_mut() else { return };
        while let Ok(m) = ring.pop() {
            let at = m.time + Duration::from_secs_f64(m.delay_secs.clamp(0.0, MAX_DELAY_SECS));
            insert(&mut self.queue, Pending { at, port: m.port, bytes: m.bytes });
        }
    }

    #[cfg(test)]
    fn insert(&mut self, p: Pending) {
        insert(&mut self.queue, p);
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.queue.front().map(|p| p.at)
    }

    /// Sends (in order) the messages due at `now`.
    pub fn send_due(&mut self, now: Instant, mut send: impl FnMut(u8, &[u8; 3])) {
        while let Some(p) = self.queue.front().copied() {
            if p.at > now {
                break;
            }
            self.queue.pop_front();
            send(p.port, &p.bytes);
        }
    }
}

/// Waits until `deadline` or until the thread is unparked (whichever comes first);
/// with no deadline, until unparked. May also return early (spurious wake-up).
pub(crate) fn wait_until(deadline: Option<Instant>) {
    let Some(at) = deadline else {
        std::thread::park();
        return;
    };
    let now = Instant::now();
    if at <= now {
        return;
    }
    if at - now > SPIN {
        std::thread::park_timeout(at - now - SPIN);
        return;
    }
    while Instant::now() < at {
        std::hint::spin_loop();
    }
}

/// Side of the thread that talks to the outside world: control messages and MIDI ports.
pub(crate) trait Output {
    /// Handles the pending control messages; `false` = quit.
    fn control(&mut self, sched: &mut Scheduler) -> bool;
    /// Sends a message on output `port` (index into the outputs).
    fn send(&mut self, port: u8, bytes: &[u8; 3]);
}

/// Body of the MIDI out thread.
pub(crate) fn run(out: &mut impl Output) {
    let mut sched = Scheduler::default();
    loop {
        if !out.control(&mut sched) {
            return;
        }
        sched.pull();
        sched.send_due(Instant::now(), |port, bytes| out.send(port, bytes));
        wait_until(sched.next_deadline());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::{Producer, RingBuffer};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::{JoinHandle, Thread};

    /// Records when each message is sent and how many times the loop ran.
    struct FakeOutput {
        ring: Option<Consumer<MidiOutMsg>>,
        quit: Arc<AtomicBool>,
        sent: Vec<(Instant, u8, [u8; 3])>,
        wakes: usize,
    }

    impl Output for FakeOutput {
        fn control(&mut self, sched: &mut Scheduler) -> bool {
            self.wakes += 1;
            if let Some(r) = self.ring.take() {
                sched.set_ring(r);
            }
            !self.quit.load(Ordering::Relaxed)
        }

        fn send(&mut self, port: u8, bytes: &[u8; 3]) {
            self.sent.push((Instant::now(), port, *bytes));
        }
    }

    /// Scheduler thread fed by a ring, as in the app (the test thread plays the engine).
    struct Harness {
        tx: Producer<MidiOutMsg>,
        thread: Thread,
        quit: Arc<AtomicBool>,
        join: JoinHandle<FakeOutput>,
    }

    impl Harness {
        fn new() -> Self {
            let (tx, rx) = RingBuffer::new(4096);
            let quit = Arc::new(AtomicBool::new(false));
            let mut out = FakeOutput { ring: Some(rx), quit: quit.clone(), sent: Vec::new(), wakes: 0 };
            let join = std::thread::spawn(move || {
                run(&mut out);
                out
            });
            Harness { tx, thread: join.thread().clone(), quit, join }
        }

        fn push(&mut self, time: Instant, delay_secs: f64, port: u8, bytes: [u8; 3]) {
            self.tx.push(MidiOutMsg { port, bytes, time, delay_secs }).unwrap();
        }

        fn finish(self) -> FakeOutput {
            self.quit.store(true, Ordering::Relaxed);
            self.thread.unpark();
            self.join.join().unwrap()
        }
    }

    fn secs(d: Duration) -> f64 {
        d.as_secs_f64()
    }

    /// Lateness of each message (seconds, negative = early). `bytes[1..]` holds the index
    /// of its deadline in `deadlines`.
    fn lateness(sent: &[(Instant, u8, [u8; 3])], deadlines: &[Instant]) -> Vec<f64> {
        sent.iter()
            .map(|(t, _, b)| {
                let d = deadlines[b[1] as usize * 128 + b[2] as usize];
                if *t >= d { secs(*t - d) } else { -secs(d - *t) }
            })
            .collect()
    }

    fn percentile(sorted: &[f64], p: f64) -> f64 {
        sorted[((sorted.len() - 1) as f64 * p).round() as usize]
    }

    /// Simulated audio callbacks (256 frames at 48 kHz, 10 ms output latency), a few
    /// messages per callback at varied offsets. Returns the deadline of each message.
    fn simulate(n_msgs: usize, mut wake: impl FnMut(), mut push: impl FnMut(MidiOutMsg)) -> Vec<Instant> {
        let period = Duration::from_micros(5333);
        let mut deadlines = Vec::with_capacity(n_msgs);
        let mut seed = 12345u32;
        let mut next = Instant::now();
        while deadlines.len() < n_msgs {
            next += period;
            while Instant::now() < next {
                std::thread::sleep(Duration::from_micros(200));
            }
            let time = Instant::now();
            for _ in 0..2 {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let offset = (seed >> 8) % 256;
                if deadlines.len() >= n_msgs || (seed >> 20) & 3 == 0 {
                    continue;
                }
                let delay_secs = 0.010 + offset as f64 / 48_000.0;
                let i = deadlines.len();
                deadlines.push(time + Duration::from_secs_f64(delay_secs));
                // Index of the message in the data bytes (recovered by `lateness`).
                push(MidiOutMsg { port: 0, bytes: [0xB0, (i / 128) as u8, (i % 128) as u8], time, delay_secs });
            }
            wake();
        }
        // Let the last ones go out.
        std::thread::sleep(Duration::from_millis(30));
        deadlines
    }

    struct Stats {
        min: f64,
        p50: f64,
        p90: f64,
        p99: f64,
        max: f64,
    }

    impl std::fmt::Display for Stats {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let us = |x: f64| x * 1e6;
            write!(
                f,
                "lateness µs: min {:.0}, p50 {:.0}, p90 {:.0}, p99 {:.0}, max {:.0}",
                us(self.min),
                us(self.p50),
                us(self.p90),
                us(self.p99),
                us(self.max)
            )
        }
    }

    fn stats(mut late: Vec<f64>) -> Stats {
        late.sort_by(f64::total_cmp);
        Stats {
            min: late[0],
            p50: percentile(&late, 0.5),
            p90: percentile(&late, 0.9),
            p99: percentile(&late, 0.99),
            max: late[late.len() - 1],
        }
    }

    fn measure_scheduler(n_msgs: usize) -> Stats {
        let mut h = Harness::new();
        let thread = h.thread.clone();
        let tx = &mut h.tx;
        let deadlines = simulate(n_msgs, || thread.unpark(), |m| tx.push(m).unwrap());
        let out = h.finish();
        assert_eq!(out.sent.len(), n_msgs);
        stats(lateness(&out.sent, &deadlines))
    }

    /// The previous loop, for comparison: polls every 1 ms, deadline counted from when the
    /// message is popped.
    fn measure_poll_1ms(n_msgs: usize) -> Stats {
        let (mut tx, mut rx) = RingBuffer::<MidiOutMsg>::new(4096);
        let quit = Arc::new(AtomicBool::new(false));
        let q2 = quit.clone();
        let join = std::thread::spawn(move || {
            let mut queue: Vec<(Instant, [u8; 3])> = Vec::new();
            let mut sent = Vec::new();
            while !q2.load(Ordering::Relaxed) {
                let now = Instant::now();
                while let Ok(m) = rx.pop() {
                    queue.push((now + Duration::from_secs_f64(m.delay_secs.clamp(0.0, 1.0)), m.bytes));
                }
                queue.sort_by_key(|q| q.0);
                let due = queue.partition_point(|q| q.0 <= now);
                for (_, b) in queue.drain(..due) {
                    sent.push((Instant::now(), 0u8, b));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            sent
        });
        let deadlines = simulate(n_msgs, || {}, |m| tx.push(m).unwrap());
        quit.store(true, Ordering::Relaxed);
        let sent = join.join().unwrap();
        assert_eq!(sent.len(), n_msgs);
        stats(lateness(&sent, &deadlines))
    }

    #[test]
    fn same_deadline_keeps_push_order() {
        let mut s = Scheduler::default();
        let t = Instant::now();
        let ms = Duration::from_millis(1);
        s.insert(Pending { at: t + 2 * ms, port: 0, bytes: [0xB0, 1, 0] });
        for v in 0..5 {
            s.insert(Pending { at: t + ms, port: 0, bytes: [0xB0, 0, v] });
        }
        s.insert(Pending { at: t, port: 0, bytes: [0xB0, 2, 0] });
        let mut got = Vec::new();
        s.send_due(t + 3 * ms, |_, b| got.push(*b));
        let expected: Vec<[u8; 3]> =
            [[0xB0, 2, 0]].into_iter().chain((0..5).map(|v| [0xB0, 0, v])).chain([[0xB0, 1, 0]]).collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn note_on_never_overtakes_note_off_of_same_key() {
        let mut s = Scheduler::default();
        let t = Instant::now();
        let us = Duration::from_micros(1);
        // Off at the end of one callback, retrigger at the start of the next, with a
        // latency estimate 30 µs lower: the on is stamped before the off.
        s.insert(Pending { at: t + 50 * us, port: 1, bytes: [0x80, 60, 0] });
        s.insert(Pending { at: t + 20 * us, port: 1, bytes: [0x90, 60, 100] });
        // Other key, other port, other channel: not held back.
        s.insert(Pending { at: t + 20 * us, port: 1, bytes: [0x90, 61, 100] });
        s.insert(Pending { at: t + 20 * us, port: 0, bytes: [0x90, 60, 100] });
        s.insert(Pending { at: t + 20 * us, port: 1, bytes: [0x91, 60, 100] });
        let mut got = Vec::new();
        s.send_due(t + 40 * us, |p, b| got.push((p, *b)));
        assert_eq!(got, vec![(1, [0x90, 61, 100]), (0, [0x90, 60, 100]), (1, [0x91, 60, 100])]);
        got.clear();
        s.send_due(t + 50 * us, |p, b| got.push((p, *b)));
        assert_eq!(got, vec![(1, [0x80, 60, 0]), (1, [0x90, 60, 100])]);
    }

    #[test]
    fn message_pushed_while_sleeping_toward_later_deadline_is_on_time() {
        let mut h = Harness::new();
        let t = Instant::now();
        h.push(t, 0.200, 0, [0xB0, 1, 0]);
        h.thread.unpark();
        std::thread::sleep(Duration::from_millis(20));
        // The thread now sleeps toward t + 200 ms.
        let t2 = Instant::now();
        h.push(t2, 0.010, 0, [0xB0, 2, 0]);
        h.thread.unpark();
        std::thread::sleep(Duration::from_millis(250));
        let out = h.finish();
        let got: Vec<[u8; 3]> = out.sent.iter().map(|s| s.2).collect();
        assert_eq!(got, vec![[0xB0, 2, 0], [0xB0, 1, 0]]);
        let late2 = out.sent[0].0 - (t2 + Duration::from_millis(10));
        let late1 = out.sent[1].0 - (t + Duration::from_millis(200));
        // Loose bound: robust on a loaded machine (an early send would underflow above).
        assert!(late2 < Duration::from_millis(5), "{late2:?}");
        assert!(late1 < Duration::from_millis(5), "{late1:?}");
    }

    #[test]
    fn thread_parks_when_idle_or_waiting() {
        let mut h = Harness::new();
        std::thread::sleep(Duration::from_millis(100));
        // A message far ahead: sleeps toward it, does not poll.
        h.push(Instant::now(), 0.150, 0, [0xB0, 1, 0]);
        h.thread.unpark();
        std::thread::sleep(Duration::from_millis(200));
        let out = h.finish();
        assert_eq!(out.sent.len(), 1);
        // A 1 ms poll would run ~300 times. Startup, unpark, deadline, spin, quit: a handful
        // (spurious wake-ups allowed).
        assert!(out.wakes < 20, "{} wake-ups", out.wakes);
    }

    #[test]
    fn messages_are_sent_on_time() {
        // Loose bounds so that a busy CI machine passes; see the ignored strict variant.
        let s = measure_scheduler(300);
        println!("{s}");
        assert!(s.min > -50e-6, "{s}");
        assert!(s.p50 < 1e-3, "{s}");
        assert!(s.p90 < 2e-3, "{s}");
    }

    /// Run alone on an idle machine: `cargo test -p seq-io -- --ignored strict_timing`.
    #[test]
    #[ignore = "timing-sensitive: needs an idle machine"]
    fn strict_timing_messages_are_sent_on_time() {
        let s = measure_scheduler(1000);
        println!("{s}");
        assert!(s.min > -50e-6, "{s}");
        assert!(s.p99 < 300e-6, "{s}");
        assert!(s.max < 2e-3, "{s}");
    }

    /// Jitter of the previous 1 ms polling loop vs the scheduler:
    /// `cargo test --release -p seq-io -- --ignored --nocapture bench_jitter`.
    #[test]
    #[ignore = "benchmark"]
    fn bench_jitter() {
        println!("1 ms poll: {}", measure_poll_1ms(1500));
        println!("scheduler: {}", measure_scheduler(1500));
    }
}
