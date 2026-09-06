//! `Global` is a strategy behind a lock, so the claims are that it builds the
//! strategy once, that its `GlobalAlloc` surface forwards to it, and that
//! threads hammering it at once neither overlap nor lose a block.
//!
//! It is exercised through the `GlobalAlloc` trait directly rather than as this
//! test binary's `#[global_allocator]`: the harness allocates on its own
//! account, and a region sized for these tests would not be sized for that.
//! `examples/global_allocator.rs` is where it is the real thing.

#![cfg(all(feature = "global", feature = "tlsf"))]

use core::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicUsize, Ordering};

use dollop::{Global, Tlsf};

static INITS: AtomicUsize = AtomicUsize::new(0);
static mut REGION: [u8; 1 << 16] = [0; 1 << 16];

static HEAP: Global<Tlsf<'static>> = Global::new(|| {
    INITS.fetch_add(1, Ordering::SeqCst);
    // SAFETY: this runs once, under the allocator's own lock, and nothing else
    // names REGION.
    Tlsf::new(unsafe { &mut *core::ptr::addr_of_mut!(REGION) }).expect("64 KiB holds a heap")
});

#[test]
fn the_strategy_is_built_once_on_first_use_and_shared_after() {
    let first = HEAP.free_bytes();
    let second = HEAP.with(|tlsf| dollop::Strategy::free_bytes(tlsf));
    assert_eq!(first, second);
    assert_eq!(
        INITS.load(Ordering::SeqCst),
        1,
        "built once, not once per call"
    );
}

#[test]
fn alloc_and_dealloc_forward_to_the_strategy() {
    let before = HEAP.free_bytes();
    let layout = Layout::from_size_align(256, 16).unwrap();

    let block = unsafe { HEAP.alloc(layout) };
    assert!(!block.is_null());
    assert_eq!(block as usize % 16, 0, "aligned as asked");
    assert!(HEAP.free_bytes() < before, "the strategy gave something up");

    unsafe { core::ptr::write_bytes(block, 0x7E, 256) };
    let bytes = unsafe { core::slice::from_raw_parts(block, 256) };
    assert!(bytes.iter().all(|b| *b == 0x7E));

    unsafe { HEAP.dealloc(block, layout) };
    assert_eq!(HEAP.free_bytes(), before, "and got it back");
}

#[test]
fn a_request_the_region_cannot_hold_is_null_rather_than_a_panic() {
    let before = HEAP.free_bytes();
    let too_big = Layout::from_size_align(1 << 20, 8).unwrap();
    assert!(unsafe { HEAP.alloc(too_big) }.is_null());
    assert_eq!(HEAP.free_bytes(), before, "and the refusal cost nothing");
}

#[test]
fn realloc_keeps_the_contents_and_returns_the_old_block() {
    let before = HEAP.free_bytes();
    let layout = Layout::from_size_align(64, 8).unwrap();
    let block = unsafe { HEAP.alloc(layout) };
    assert!(!block.is_null());
    unsafe { core::ptr::write_bytes(block, 0x42, 64) };

    let bigger = unsafe { HEAP.realloc(block, layout, 4096) };
    assert!(!bigger.is_null());
    let bytes = unsafe { core::slice::from_raw_parts(bigger, 64) };
    assert!(
        bytes.iter().all(|b| *b == 0x42),
        "the first 64 bytes are what they were"
    );

    unsafe { HEAP.dealloc(bigger, Layout::from_size_align(4096, 8).unwrap()) };
    assert_eq!(
        HEAP.free_bytes(),
        before,
        "one block out, whatever route it took"
    );
}

#[test]
fn threads_allocating_at_once_get_distinct_blocks_and_everything_comes_back() {
    let before = HEAP.free_bytes();
    let layout = Layout::from_size_align(48, 8).unwrap();

    // Every thread takes a handful of blocks, stamps each with its own byte,
    // checks nobody else's stamp landed in them, and returns them. A lock that
    // let two threads into the strategy at once would corrupt the free lists
    // and show up here as overlapping blocks or a free count that does not
    // come back.
    std::thread::scope(|s| {
        for t in 0u8 .. 8 {
            s.spawn(move || {
                let stamp = 0x10 + t;
                for _ in 0 .. 200 {
                    let mut held = Vec::new();
                    for _ in 0 .. 4 {
                        let block = unsafe { HEAP.alloc(layout) };
                        assert!(!block.is_null(), "room for eight threads' worth");
                        unsafe { core::ptr::write_bytes(block, stamp, 48) };
                        held.push(block);
                    }
                    for block in &held {
                        let bytes = unsafe { core::slice::from_raw_parts(*block, 48) };
                        assert!(
                            bytes.iter().all(|b| *b == stamp),
                            "another thread's block overlapped this one"
                        );
                    }
                    for block in held {
                        unsafe { HEAP.dealloc(block, layout) };
                    }
                }
            });
        }
    });

    assert_eq!(
        HEAP.free_bytes(),
        before,
        "every block came back and the free lists held"
    );
}

#[test]
fn the_lock_is_released_when_the_strategy_panics_inside_it() {
    // A panic inside `with` must not leave the lock held, or every later
    // allocation in the process spins forever.
    let outcome = std::panic::catch_unwind(|| {
        HEAP.with(|_| panic!("inside the critical section"));
    });
    assert!(outcome.is_err());
    // Provable only by taking the lock again, which is what this does.
    let _ = HEAP.free_bytes();
}
