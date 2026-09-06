//! Structural checks over the block list, and a randomised sequence that
//! runs them after every operation.
//!
//! The suite that existed checked what the allocator returns. These check
//! what it leaves behind: the physical chain, the free lists, the
//! bitmaps, and the free counter, none of which are observable through
//! the public surface and any of which going wrong is how an
//! allocator corrupts memory long after the operation that broke it.

// The crate is `no_std` under its default features, so the test module reaches
// for the collections it needs explicitly rather than through a prelude
// that is not there.
extern crate std;
use std::vec::Vec;

use super::*;
use crate::Strategy;

/// One block as the walk sees it.
#[derive(Debug, Clone, Copy)]
struct Seen {
    header: *mut Header,
    size:   usize,
    free:   bool,
    last:   bool,
}

/// Walks the physical chain from the first block and checks it holds
/// together.
///
/// # Safety
///
/// `base` is the region [`Tlsf::new`] was given, unmoved, and the allocator
/// built from it is still alive.
unsafe fn walk(base: *mut u8) -> Vec<Seen> {
    let mut blocks = Vec::new();
    let mut header = align_up(base as usize, ALIGN) as *mut Header;
    let mut expected_prev: *mut Header = null_mut();

    loop {
        let size = (*header).size();
        assert_eq!(size & (ALIGN - 1), 0, "a block size is a multiple of ALIGN");
        assert!(size >= MIN_PAYLOAD, "a block is at least MIN_PAYLOAD");
        assert_eq!(
            (*header).prev_phys,
            expected_prev,
            "prev_phys points at the block physically before"
        );

        blocks.push(Seen {
            header,
            size,
            free: (*header).is_free(),
            last: (*header).is_last(),
        });

        let next = (*header).next_phys();
        if next.is_null() {
            break;
        }
        assert!(
            !(*header).is_last(),
            "only the final block carries FLAG_LAST"
        );
        expected_prev = header;
        header = next;
        assert!(blocks.len() < 100_000, "the chain terminates");
    }

    assert!(
        blocks.last().map_or(false, |block| block.last),
        "the final block carries FLAG_LAST"
    );
    blocks
}

/// Every invariant that has to hold between operations.
///
/// # Safety
///
/// As [`walk`].
unsafe fn check(base: *mut u8, alloc: &Tlsf) {
    let blocks = walk(base);

    // Coalescing: a free block never sits next to another free block, or the
    // allocator would be holding two blocks where it could hand out one.
    for pair in blocks.windows(2) {
        assert!(
            !(pair[0].free && pair[1].free),
            "adjacent free blocks were not merged: {:?} then {:?}",
            pair[0],
            pair[1]
        );
    }

    // The free counter matches what the chain says is free.
    let counted: usize = blocks.iter().filter(|b| b.free).map(|b| b.size).sum();
    assert_eq!(
        counted,
        alloc.free_bytes(),
        "free_bytes agrees with the blocks marked free"
    );

    // Every block on a size-class list is free, is filed under the class its size
    // maps to, and appears exactly once.
    let mut listed = Vec::new();
    for fl in 0 .. FL_COUNT {
        for sl in 0 .. SL_COUNT {
            let mut node = alloc.heads[fl][sl];
            let mut guard = 0;
            while !node.is_null() {
                assert!((*node).is_free(), "a block on a free list is marked free");
                assert_eq!(
                    mapping((*node).size()),
                    (fl, sl),
                    "a block is filed under the class its size maps to"
                );
                assert!(
                    !listed.contains(&node),
                    "a block appears on at most one free list, once"
                );
                listed.push(node);
                node = (*(*node).links()).next;
                guard += 1;
                assert!(guard < 100_000, "a free list terminates");
            }

            // The bitmaps say exactly which classes have anything in them.
            let bit_set = alloc.sl_bitmaps[fl] & (1 << sl) != 0;
            assert_eq!(
                bit_set,
                !alloc.heads[fl][sl].is_null(),
                "the second-level bit for class ({}, {}) matches its list",
                fl,
                sl
            );
        }
        let fl_set = alloc.fl_bitmap & (1 << fl) != 0;
        assert_eq!(
            fl_set,
            alloc.sl_bitmaps[fl] != 0,
            "the first-level bit for class {} matches its second level",
            fl
        );
    }

    // And every block the chain says is free is on a list, so none is stranded.
    let free_in_chain: Vec<*mut Header> =
        blocks.iter().filter(|b| b.free).map(|b| b.header).collect();
    assert_eq!(
        free_in_chain.len(),
        listed.len(),
        "every free block is filed, and nothing else is"
    );
    for header in free_in_chain {
        assert!(
            listed.contains(&header),
            "a free block was left off its list"
        );
    }
}

/// A deterministic sequence, so a failure is reproducible from the seed
/// alone.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*, which is enough for choosing sizes and victims.
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

#[test]
fn a_fresh_region_starts_consistent() {
    let mut region = [0u8; 4096];
    let base = region.as_mut_ptr();
    let alloc = Tlsf::new(&mut region).expect("4096 bytes holds a block");
    unsafe { check(base, &alloc) };
}

#[test]
fn one_allocation_and_its_release_return_the_region_to_where_it_started() {
    let mut region = [0u8; 4096];
    let base = region.as_mut_ptr();
    let mut alloc = Tlsf::new(&mut region).expect("4096 bytes holds a block");
    let before = alloc.free_bytes();

    let layout = Layout::from_size_align(64, 8).unwrap();
    let ptr = alloc.allocate(layout).expect("a fresh region has room");
    unsafe { check(base, &alloc) };
    assert!(alloc.free_bytes() < before, "an allocation costs something");

    unsafe { alloc.deallocate(ptr, layout) };
    unsafe { check(base, &alloc) };
    assert_eq!(
        alloc.free_bytes(),
        before,
        "releasing the only allocation gives every byte back"
    );
}

#[test]
fn a_randomised_sequence_holds_every_invariant_at_every_step() {
    let mut region = [0u8; 1 << 16];
    let base = region.as_mut_ptr();
    let mut alloc = Tlsf::new(&mut region).expect("the region holds a block");
    let initial = alloc.free_bytes();

    let mut rng = Rng(0x5EED_1234_ABCD_0001);
    let mut live: Vec<(NonNull<u8>, Layout, u8)> = Vec::new();

    for step in 0 .. 4_000 {
        // Allocate more often than free while there is little outstanding, so the
        // region fills and the interesting paths (splitting, exhaustion,
        // coalescing) are reached. Three moves rather than two: allocate, free,
        // or resize a live block, since the in-place paths of `reallocate`
        // rewrite headers and file remainders exactly where a mistake corrupts
        // a neighbour.
        let roll = rng.below(100);
        let allocating = live.is_empty() || roll < 50;
        let resizing = !live.is_empty() && !allocating && roll < 75;

        if resizing {
            let victim = rng.below(live.len());
            let (ptr, layout, tag) = live[victim];
            let new_size = 1 + rng.below(600);
            if let Some(moved) = unsafe { alloc.reallocate(ptr, layout, new_size) } {
                let kept = layout.size().min(new_size);
                let bytes = unsafe { core::slice::from_raw_parts(moved.as_ptr(), kept) };
                assert!(
                    bytes.iter().all(|byte| *byte == tag),
                    "step {}: a resized block kept its bytes",
                    step
                );
                // The whole new length is the caller's, so stamp all of it: a
                // resize that lied about its size overlaps whatever follows.
                unsafe { core::ptr::write_bytes(moved.as_ptr(), tag, new_size) };
                live[victim] = (
                    moved,
                    Layout::from_size_align(new_size, layout.align()).unwrap(),
                    tag,
                );
            }
        } else if allocating {
            let size = 1 + rng.below(600);
            let align = 1usize << rng.below(7);
            let layout = Layout::from_size_align(size, align).unwrap();

            if let Some(ptr) = alloc.allocate(layout) {
                assert_eq!(
                    ptr.as_ptr() as usize % align,
                    0,
                    "step {}: the alignment asked for is honoured",
                    step
                );
                // A byte pattern per allocation, checked on release: if two live blocks
                // ever overlap, one of them will read back the other's.
                let tag = (step % 251) as u8 + 1;
                unsafe { core::ptr::write_bytes(ptr.as_ptr(), tag, size) };
                live.push((ptr, layout, tag));
            }
        } else {
            let victim = rng.below(live.len());
            let (ptr, layout, tag) = live.swap_remove(victim);
            unsafe {
                let bytes = core::slice::from_raw_parts(ptr.as_ptr(), layout.size());
                assert!(
                    bytes.iter().all(|byte| *byte == tag),
                    "step {}: a live allocation still holds its own bytes",
                    step
                );
                alloc.deallocate(ptr, layout);
            }
        }

        unsafe { check(base, &alloc) };
    }

    // Everything still outstanding goes back, and the region ends as it began. A
    // permanent loss here would mean a block went missing rather than
    // merely being fragmented.
    for (ptr, layout, _) in live {
        unsafe { alloc.deallocate(ptr, layout) };
        unsafe { check(base, &alloc) };
    }
    assert_eq!(
        alloc.free_bytes(),
        initial,
        "with nothing outstanding, every byte is back and merged into one block"
    );
    assert_eq!(
        unsafe { walk(base) }.len(),
        1,
        "and the region is one block again, not a pile of unmerged neighbours"
    );
}
