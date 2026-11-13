//! Cost of the per-frame project sync (undo history + graph) on a large project.
//! `cargo test --release -p seq-engine --test perf -- --ignored --nocapture`

use seq_edit::History;
use seq_engine::{Compiler, compile};
use seq_model::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

/// Counts live heap bytes.
struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        LIVE.fetch_add(size as isize - l.size() as isize, Ordering::Relaxed);
        unsafe { System.realloc(p, l, size) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn live() -> isize {
    LIVE.load(Ordering::Relaxed)
}

/// 64 channels, 32 patterns of 4 bars, 10 notes per channel and pattern (20 480 notes).
fn large_project() -> Project {
    let mut p = Project::empty();
    let ids: Vec<ChannelId> =
        (0..64).map(|i| p.add_channel(&format!("ch {i}"), ChannelKind::Synth(SynthParams::default())).unwrap()).collect();
    let len = 4 * p.bar_len();
    p.patterns.clear();
    for i in 0..32 {
        let pat = p.add_pattern(&format!("Pattern {i}"), len);
        for (c, &id) in ids.iter().enumerate() {
            let notes = (0..10).map(|n| Note { start: (n * 6 + c as u64 % 4) * STEP, len: STEP, key: 40 + (n as u8 * 3 + c as u8) % 48, velocity: 100 }).collect();
            p.pattern_mut(pat).unwrap().notes.insert(id, notes);
        }
    }
    p
}

/// Moves one note of one pattern: one edit = one frame of a drag.
fn edit(p: &mut Project, i: usize) {
    let id = p.patterns[i % p.patterns.len()].id;
    let ch = p.channels[i % p.channels.len()].id;
    let pat = p.pattern_mut(id).unwrap();
    let n = &mut pat.notes.get_mut(&ch).unwrap()[0];
    n.start = (n.start + STEP) % pat.length;
}

#[test]
#[ignore]
fn frame_sync_cost() {
    let mut p = large_project();
    let notes: usize = p.patterns.iter().flat_map(|x| x.notes.values()).map(|v| v.len()).sum();
    println!("project: {} channels, {} patterns, {notes} notes", p.channels.len(), p.patterns.len());

    let before = live();
    let mut h = History::new(&p, 200);
    let mut sent: Option<Project> = None;

    // What the app does every frame: commit, then resend the graph if the project changed.
    let mut compiler = Compiler::default();
    let mut frame = |p: &Project, h: &mut History, sent: &mut Option<Project>| {
        h.commit_if_changed(p, false);
        if sent.as_ref() != Some(p) {
            std::hint::black_box(compiler.compile(p));
            *sent = Some(p.clone());
        }
    };
    frame(&p, &mut h, &mut sent);

    const IDLE: u32 = 1000;
    let t = Instant::now();
    for _ in 0..IDLE {
        frame(&p, &mut h, &mut sent);
    }
    let idle = t.elapsed() / IDLE;

    const EDITS: usize = 300;
    let t = Instant::now();
    for i in 0..EDITS {
        edit(&mut p, i);
        frame(&p, &mut h, &mut sent);
    }
    let edit_frame = t.elapsed() / EDITS as u32;

    let t = Instant::now();
    for _ in 0..IDLE {
        std::hint::black_box(p.clone());
    }
    let clone = t.elapsed() / IDLE;

    let t = Instant::now();
    for _ in 0..20 {
        std::hint::black_box(compile(&p));
    }
    let full_compile = t.elapsed() / 20;

    drop(sent);
    drop(compiler);
    let history = live() - before;
    println!("idle frame (2 compares):         {idle:?}");
    println!("edit frame (commit + graph):     {edit_frame:?}");
    println!("project clone:                   {clone:?}");
    println!("full compile:                    {full_compile:?}");
    println!("history after {EDITS} edits:        {:.1} MiB ({} entries, estimate {:.1} MiB)", history as f64 / (1 << 20) as f64, h.undo_len(), h.undo_bytes() as f64 / (1 << 20) as f64);
    drop(h);
}
