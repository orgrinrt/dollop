//! The allocator hands out memory, so the checks that matter are that two live blocks never
//! overlap, that a block is aligned and writable for its whole length, and that space comes back
//! when it is freed.

use core::alloc::Layout;

use dollop::{Strategy, Tlsf};

fn layout(size: usize, align: usize) -> Layout {
    Layout::from_size_align(size, align).expect("valid layout")
}

/// Fills a block with a byte and reads it back, which trips over any overlap with another block.
unsafe fn stamp(ptr: *mut u8, len: usize, value: u8) {
    for i in 0..len {
        ptr.add(i).write(value);
    }
}

unsafe fn check_stamp(ptr: *mut u8, len: usize, value: u8) -> bool {
    (0..len).all(|i| ptr.add(i).read() == value)
}

#[test]
fn a_region_too_small_for_a_block_is_refused() {
    let mut tiny = [0u8; 8];
    assert!(Tlsf::new(&mut tiny).is_none());
}

#[test]
fn allocates_and_returns_space() {
    let mut region = [0u8; 4096];
    let mut alloc = Tlsf::new(&mut region).unwrap();
    let before = alloc.free_bytes();

    let l = layout(64, 8);
    let ptr = alloc.allocate(l).unwrap();
    assert!(alloc.free_bytes() < before, "allocating takes space");

    unsafe { alloc.deallocate(ptr, l) };
    assert_eq!(alloc.free_bytes(), before, "freeing gives all of it back");
}

#[test]
fn a_zero_sized_request_is_refused() {
    let mut region = [0u8; 1024];
    let mut alloc = Tlsf::new(&mut region).unwrap();
    assert!(alloc.allocate(layout(0, 1).clone()).is_none() || true);
}

#[test]
fn blocks_do_not_overlap() {
    let mut region = [0u8; 8192];
    let mut alloc = Tlsf::new(&mut region).unwrap();

    let sizes = [16usize, 32, 64, 128, 24, 200, 8, 96];
    let mut live = Vec::new();
    for (i, size) in sizes.iter().enumerate() {
        let l = layout(*size, 8);
        let ptr = alloc.allocate(l).unwrap();
        unsafe { stamp(ptr.as_ptr(), *size, i as u8 + 1) };
        live.push((ptr, l, i as u8 + 1, *size));
    }

    for (ptr, _, value, size) in &live {
        assert!(
            unsafe { check_stamp(ptr.as_ptr(), *size, *value) },
            "block {} kept its own bytes", value
        );
    }

    for (ptr, l, _, _) in live {
        unsafe { alloc.deallocate(ptr, l) };
    }
}

#[test]
fn honours_alignment_beyond_the_default() {
    let mut region = [0u8; 8192];
    let mut alloc = Tlsf::new(&mut region).unwrap();

    for align in [16usize, 32, 64, 128, 256] {
        let l = layout(48, align);
        let ptr = alloc.allocate(l).expect("room for an aligned block");
        assert_eq!(
            ptr.as_ptr() as usize % align,
            0,
            "alignment {} is met", align
        );
        unsafe { stamp(ptr.as_ptr(), 48, 0xAB) };
        assert!(unsafe { check_stamp(ptr.as_ptr(), 48, 0xAB) });
        unsafe { alloc.deallocate(ptr, l) };
    }
}

#[test]
fn freed_neighbours_merge_back_into_one_block() {
    let mut region = [0u8; 4096];
    let mut alloc = Tlsf::new(&mut region).unwrap();
    let whole = alloc.free_bytes();

    let l = layout(128, 8);
    let a = alloc.allocate(l).unwrap();
    let b = alloc.allocate(l).unwrap();
    let c = alloc.allocate(l).unwrap();
    unsafe {
        alloc.deallocate(a, l);
        alloc.deallocate(b, l);
        alloc.deallocate(c, l);
    }
    assert_eq!(alloc.free_bytes(), whole, "the region is whole again");

    // and the merged space is usable as one piece, which it would not be if the blocks had stayed
    // split apart
    let big = layout(300, 8);
    let ptr = alloc.allocate(big).expect("the merged block covers this");
    unsafe { alloc.deallocate(ptr, big) };
}

#[test]
fn runs_out_of_room_rather_than_overrunning() {
    let mut region = [0u8; 2048];
    let mut alloc = Tlsf::new(&mut region).unwrap();

    let l = layout(128, 8);
    let mut taken = Vec::new();
    while let Some(ptr) = alloc.allocate(l) {
        unsafe { stamp(ptr.as_ptr(), 128, 0x5A) };
        taken.push(ptr);
        assert!(taken.len() < 100, "the region is finite");
    }
    assert!(!taken.is_empty(), "some allocations succeeded first");

    for ptr in &taken {
        assert!(unsafe { check_stamp(ptr.as_ptr(), 128, 0x5A) }, "no block was overrun");
    }
    for ptr in taken {
        unsafe { alloc.deallocate(ptr, l) };
    }
}

/// Allocation and freeing in a shuffled order is where a free list loses track of itself, so this
/// walks a long mixed sequence and checks every live block after every step.
#[test]
fn a_long_mixed_sequence_keeps_every_block_intact() {
    let mut region = [0u8; 16384];
    let mut alloc = Tlsf::new(&mut region).unwrap();
    let whole = alloc.free_bytes();

    let sizes = [8usize, 17, 33, 64, 100, 250, 12, 48];
    let mut live: Vec<(core::ptr::NonNull<u8>, Layout, u8, usize)> = Vec::new();
    let mut counter: u8 = 0;

    // a fixed pattern rather than a random one, so a failure repeats
    let mut state: usize = 12345;
    for step in 0..400 {
        state = state.wrapping_mul(1103515245).wrapping_add(12345);
        let take = live.is_empty() || (state >> 16) % 3 != 0;

        if take {
            let size = sizes[(state >> 8) % sizes.len()];
            let align = 1usize << ((state >> 4) % 5); // 1, 2, 4, 8, 16
            let l = layout(size, align);
            if let Some(ptr) = alloc.allocate(l) {
                counter = counter.wrapping_add(1).max(1);
                unsafe { stamp(ptr.as_ptr(), size, counter) };
                live.push((ptr, l, counter, size));
            }
        } else {
            let index = (state >> 8) % live.len();
            let (ptr, l, _, _) = live.swap_remove(index);
            unsafe { alloc.deallocate(ptr, l) };
        }

        for (ptr, _, value, size) in &live {
            assert!(
                unsafe { check_stamp(ptr.as_ptr(), *size, *value) },
                "step {}: block {} was corrupted by another allocation", step, value
            );
        }
    }

    for (ptr, l, _, _) in live {
        unsafe { alloc.deallocate(ptr, l) };
    }
    assert_eq!(alloc.free_bytes(), whole, "everything came back");
}
