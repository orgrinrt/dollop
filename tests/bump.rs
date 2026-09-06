//! The bump allocator hands out the region front to back and takes back only
//! what sits at the end, so the checks are about where the mark goes.

#![cfg(feature = "bump")]

use core::alloc::Layout;

use dollop::{Bump, Strategy};

fn layout(size: usize, align: usize) -> Layout {
    Layout::from_size_align(size, align).expect("valid layout")
}

#[test]
fn an_empty_region_is_an_allocator_that_refuses_everything() {
    let mut nothing = [0u8; 0];
    let mut arena = Bump::new(&mut nothing);
    assert_eq!(arena.free_bytes(), 0);
    assert!(arena.allocate(layout(1, 1)).is_none());
}

#[test]
fn a_zero_sized_request_is_refused() {
    let mut region = [0u8; 64];
    let mut arena = Bump::new(&mut region);
    assert!(arena.allocate(layout(0, 1)).is_none());
    assert_eq!(arena.free_bytes(), 64, "and costs nothing");
}

#[test]
fn blocks_come_out_in_order_and_do_not_overlap() {
    let mut region = [0u8; 256];
    let mut arena = Bump::new(&mut region);

    let a = arena.allocate(layout(32, 1)).unwrap();
    let b = arena.allocate(layout(32, 1)).unwrap();
    let c = arena.allocate(layout(32, 1)).unwrap();
    assert!(a < b && b < c, "front to back");
    assert!(
        b.as_ptr() as usize - a.as_ptr() as usize >= 32,
        "a and b do not overlap"
    );
    assert!(
        c.as_ptr() as usize - b.as_ptr() as usize >= 32,
        "b and c do not overlap"
    );
    assert_eq!(arena.used_bytes(), 96);
    assert_eq!(arena.free_bytes(), 256 - 96);
}

#[test]
fn alignment_is_honoured_and_the_padding_is_counted_as_used() {
    let mut backing = [0u8; 1024];
    // Start one byte in, so the first request at a wide alignment has to pad.
    let region = &mut backing[1 ..];
    let mut arena = Bump::new(region);
    let whole = arena.free_bytes();

    for align in [2usize, 8, 64, 256] {
        let ptr = arena.allocate(layout(16, align)).expect("room");
        assert_eq!(ptr.as_ptr() as usize % align, 0, "alignment {align} is met");
    }
    assert!(
        arena.used_bytes() > 4 * 16,
        "the padding between blocks is part of what was used"
    );
    assert_eq!(arena.used_bytes() + arena.free_bytes(), whole);
}

#[test]
fn only_the_most_recent_block_comes_back() {
    let mut region = [0u8; 256];
    let mut arena = Bump::new(&mut region);
    let l = layout(32, 8);

    let a = arena.allocate(l).unwrap();
    let b = arena.allocate(l).unwrap();
    let after_two = arena.free_bytes();

    // Returning the older block moves nothing: the mark is past it and nothing
    // remembers where it was.
    unsafe { arena.deallocate(a, l) };
    assert_eq!(
        arena.free_bytes(),
        after_two,
        "a block in the middle stays taken"
    );

    // Returning the most recent one moves the mark back to where it started.
    unsafe { arena.deallocate(b, l) };
    assert_eq!(arena.free_bytes(), after_two + 32);

    // And now `a` is the most recent block, so a second attempt at it counts.
    unsafe { arena.deallocate(a, l) };
    assert_eq!(arena.free_bytes(), 256, "the region is whole again");
}

#[test]
fn reset_takes_everything_back_at_once() {
    let mut region = [0u8; 256];
    let mut arena = Bump::new(&mut region);
    let l = layout(32, 8);
    for _ in 0 .. 5 {
        arena.allocate(l).expect("room");
    }
    assert_eq!(arena.used_bytes(), 160);

    unsafe { arena.reset() };
    assert_eq!(arena.used_bytes(), 0);
    assert_eq!(arena.free_bytes(), 256);
    assert!(
        arena.allocate(layout(256, 1)).is_some(),
        "and the whole region is usable again"
    );
}

#[test]
fn runs_out_of_room_rather_than_overrunning() {
    let mut region = [0u8; 100];
    let mut arena = Bump::new(&mut region);
    let l = layout(32, 1);
    assert!(arena.allocate(l).is_some());
    assert!(arena.allocate(l).is_some());
    assert!(arena.allocate(l).is_some());
    assert_eq!(arena.free_bytes(), 4);
    assert!(arena.allocate(l).is_none(), "a fourth does not fit");
    assert_eq!(arena.free_bytes(), 4, "and the refusal cost nothing");
    assert!(
        arena.allocate(layout(4, 1)).is_some(),
        "what is left is still usable"
    );
}

#[test]
fn the_largest_request_a_layout_allows_is_refused_rather_than_wrapped() {
    let mut region = [0u8; 64];
    let mut arena = Bump::new(&mut region);
    // `isize::MAX` is the largest size a `Layout` accepts, and `start + size` for
    // it is where an unchecked add would wrap on a target whose addresses reach
    // that high. The end is computed with a checked add so the answer is a
    // refusal either way.
    assert!(arena.allocate(layout(isize::MAX as usize, 1)).is_none());
    assert_eq!(arena.free_bytes(), 64, "and the refusal cost nothing");
}

#[test]
fn the_last_block_resizes_where_it_stands() {
    let mut region = [0u8; 256];
    let mut arena = Bump::new(&mut region);
    let l = layout(32, 8);

    let a = arena.allocate(l).unwrap();
    unsafe { core::ptr::write_bytes(a.as_ptr(), 0xA5, 32) };

    let grown = unsafe { arena.reallocate(a, l, 96) }.expect("room to grow");
    assert_eq!(grown, a, "the last block grows in place");
    assert_eq!(arena.used_bytes(), 96);
    let bytes = unsafe { core::slice::from_raw_parts(grown.as_ptr(), 32) };
    assert!(bytes.iter().all(|b| *b == 0xA5), "the contents survive");

    let shrunk = unsafe { arena.reallocate(grown, layout(96, 8), 16) }.expect("shrinking fits");
    assert_eq!(shrunk, a, "and shrinks in place");
    assert_eq!(arena.used_bytes(), 16);

    assert!(
        unsafe { arena.reallocate(shrunk, layout(16, 8), 1024) }.is_none(),
        "growing past the region is refused"
    );
    assert_eq!(arena.used_bytes(), 16, "and the refusal changed nothing");
    assert!(unsafe { arena.reallocate(shrunk, layout(16, 8), 0) }.is_none());
}

#[test]
fn an_older_block_resizes_by_moving_and_keeps_its_contents() {
    let mut region = [0u8; 512];
    let mut arena = Bump::new(&mut region);
    let l = layout(32, 8);

    let a = arena.allocate(l).unwrap();
    let _b = arena.allocate(l).unwrap();
    unsafe { core::ptr::write_bytes(a.as_ptr(), 0x3C, 32) };

    let moved = unsafe { arena.reallocate(a, l, 64) }.expect("room");
    assert_ne!(moved, a, "a block that is not the last one has to move");
    let bytes = unsafe { core::slice::from_raw_parts(moved.as_ptr(), 32) };
    assert!(
        bytes.iter().all(|b| *b == 0x3C),
        "the contents came with it"
    );
    assert_eq!(
        arena.used_bytes(),
        64 + 64,
        "the old block stays taken, as it would have anyway"
    );
}
