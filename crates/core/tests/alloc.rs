//! Gate 8 (partial): the per-frame hot loop performs zero heap allocations.
#![allow(unsafe_code)]

use dvr_core::engine::{Engine, FrameInput, HandInput};
use dvr_core::math::{Pose, Vec3};
use dvr_core::panel::{Panel, Source};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator unchanged; only counts calls.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
fn hot_loop_is_allocation_free() {
    let head = Pose::new(Vec3::new(0.0, 1.6, 0.0), Default::default());
    let mut engine =
        Engine::new(vec![Panel::in_front_of(1, Source::Monitor(0), head, 9.0 / 16.0), Panel::keyboard_below(2, head)]);
    let mut out = Vec::with_capacity(64);
    let mut run = |frames: u64, engine: &mut Engine| {
        for f in 0..frames {
            let sweep = (f % 200) as f32 / 200.0 - 0.5;
            let hand = |x: f32, dy: f32| HandInput {
                pose: Pose::aim(Vec3::new(x, 1.3, -0.2), Vec3::new(sweep, 1.45 + dy, -1.0)),
                tracked: true,
                trigger: f % 7 < 3,
                grip: f % 97 < 40,
                menu: f % 13 == 0,
                stick: [0.0, if f % 50 < 25 { 1.0 } else { 0.0 }],
            };
            out.clear();
            let input =
                FrameInput { now_ms: f * 11, hands: [hand(-0.2, -0.6), hand(0.2, 0.0)], ..FrameInput::default() };
            engine.step(&input, &mut out);
        }
    };
    run(200, &mut engine);
    ALLOCS.store(0, Ordering::Relaxed);
    run(10_000, &mut engine);
    assert_eq!(ALLOCS.load(Ordering::Relaxed), 0);
}
