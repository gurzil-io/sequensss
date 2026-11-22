//! App sounds (`EngineCmd::PlaySound`) never allocate or free on the audio thread: the
//! sample is built on the UI side and goes back to it as garbage.

use seq_engine::{Engine, EngineCmd, Garbage, SampleData};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// Counts the heap operations of the current thread while counting is on.
struct Counting;

thread_local! {
    static COUNT: Cell<Option<usize>> = const { Cell::new(None) };
}

fn note() {
    let _ = COUNT.try_with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        note();
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        note();
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        note();
        unsafe { System.realloc(p, l, size) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// Heap operations made by `f` on this thread.
fn counted(f: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(Some(0)));
    f();
    COUNT.with(|c| c.replace(None)).unwrap()
}

#[test]
fn sound_does_not_allocate_on_the_audio_thread() {
    let sr = 48_000;
    let (mut e, mut h) = Engine::new(sr, None, None);
    let mut buf = vec![0.0f32; 512 * 2];
    let sample = || Box::new(SampleData::from_mono(sr, vec![0.5; 4800]));
    let mut callbacks = |e: &mut Engine, n: usize| {
        counted(|| {
            for _ in 0..n {
                e.process(&mut buf, 2, 0.0);
            }
        })
    };

    // Played to its end.
    assert!(h.send(EngineCmd::PlaySound { sample: sample(), gain: 0.5 }));
    assert_eq!(callbacks(&mut e, 20), 0);
    // Replaced while playing, then stopped.
    assert!(h.send(EngineCmd::PlaySound { sample: sample(), gain: 0.5 }));
    assert_eq!(callbacks(&mut e, 2), 0);
    assert!(h.send(EngineCmd::PlaySound { sample: sample(), gain: 0.5 }));
    assert_eq!(callbacks(&mut e, 2), 0);
    assert!(h.send(EngineCmd::StopSound));
    assert_eq!(callbacks(&mut e, 20), 0);

    let mut freed = 0;
    while let Ok(g) = h.garbage.pop() {
        assert!(matches!(g, Garbage::Sound(_)));
        freed += 1;
    }
    assert_eq!(freed, 3);
}
