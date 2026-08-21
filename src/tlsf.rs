//! Two-level segregated fit.
//!
//! Free blocks are filed by size into classes, and the class index is a pair:
//! the first level is the power of two the size falls in, the second splits
//! that range into [`SL_COUNT`] even parts. A bitmap per level records which
//! classes hold anything, so finding a block big enough is a matter of masking
//! off the classes that are too small and taking the lowest bit that is still
//! set. That is a fixed amount of work rather than a walk, which is the point
//! of the arrangement.
//!
//! Blocks carry a header with their size and a pointer to the block physically
//! before them, so a block being freed can be merged with the neighbours on
//! either side. Free blocks additionally keep their list links inside the
//! payload, which costs nothing: the space is not in use.

use core::alloc::Layout;
use core::marker::PhantomData;
use core::mem::{align_of, size_of};
use core::ptr::{null_mut, NonNull};

use crate::strategy::Strategy;

/// Second-level classes per first-level class.
const SL_COUNT: usize = 4;
const SL_BITS: u32 = 2;
/// Smallest payload a block can hold, which is what the free list links need.
const MIN_PAYLOAD: usize = size_of::<FreeLinks>();
/// Every block payload starts at this alignment.
const ALIGN: usize = align_of::<usize>() * 2;
/// The first level starts at [`MIN_PAYLOAD`], so smaller sizes all land in
/// class zero.
const FL_SHIFT: u32 = MIN_PAYLOAD.trailing_zeros();
/// Enough first-level classes to reach any size a `usize` can express.
const FL_COUNT: usize = (usize::BITS - FL_SHIFT) as usize;

const FLAG_FREE: usize = 0b01;
const FLAG_LAST: usize = 0b10;
const FLAG_MASK: usize = 0b11;

/// Sits immediately before every block's payload, allocated or free.
#[repr(C)]
struct Header {
    /// Payload size, with [`FLAG_FREE`] and [`FLAG_LAST`] in the low bits.
    /// Sizes are a multiple of [`ALIGN`], so those bits are free to use.
    size_and_flags: usize,
    /// The block physically before this one, or null for the first block in the
    /// region.
    prev_phys:      *mut Header,
}

/// Written into a free block's payload to thread it onto its size class's list.
#[repr(C)]
struct FreeLinks {
    next: *mut Header,
    prev: *mut Header,
}

impl Header {
    #[inline]
    fn size(&self) -> usize {
        self.size_and_flags & !FLAG_MASK
    }

    #[inline]
    fn is_free(&self) -> bool {
        self.size_and_flags & FLAG_FREE != 0
    }

    #[inline]
    fn is_last(&self) -> bool {
        self.size_and_flags & FLAG_LAST != 0
    }

    #[inline]
    fn set_size(&mut self, size: usize) {
        debug_assert_eq!(size & FLAG_MASK, 0, "sizes keep the flag bits clear");
        self.size_and_flags = size | (self.size_and_flags & FLAG_MASK);
    }

    #[inline]
    fn set_flag(&mut self, flag: usize, on: bool) {
        if on {
            self.size_and_flags |= flag;
        } else {
            self.size_and_flags &= !flag;
        }
    }

    /// The payload, which is where the caller's bytes go, and where the free
    /// links live while the block is on a list.
    #[inline]
    fn payload(&mut self) -> *mut u8 {
        // safety: a header is always followed by its payload, by construction
        unsafe { (self as *mut Header as *mut u8).add(size_of::<Header>()) }
    }

    #[inline]
    unsafe fn links(&mut self) -> *mut FreeLinks {
        self.payload() as *mut FreeLinks
    }

    /// The block physically after this one, or null when this is the last.
    #[inline]
    unsafe fn next_phys(&mut self) -> *mut Header {
        if self.is_last() {
            null_mut()
        } else {
            self.payload().add(self.size()) as *mut Header
        }
    }
}

#[inline]
fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

/// Which class a size belongs to.
#[inline]
fn mapping(size: usize) -> (usize, usize) {
    if size < MIN_PAYLOAD {
        return (0, 0);
    }
    let fl_raw = usize::BITS - 1 - size.leading_zeros();
    let fl = (fl_raw - FL_SHIFT) as usize;
    let sl = (size >> (fl_raw - SL_BITS)) & (SL_COUNT - 1);
    (fl.min(FL_COUNT - 1), sl)
}

/// The class to start searching from for a request of `size`.
///
/// Rounding up first means every block filed in the class that is found is big
/// enough, so the search never has to look at a block to reject it.
#[inline]
fn mapping_for_request(size: usize) -> (usize, usize) {
    if size >= MIN_PAYLOAD {
        let fl_raw = usize::BITS - 1 - size.leading_zeros();
        if fl_raw > SL_BITS {
            let round = (1usize << (fl_raw - SL_BITS)) - 1;
            return mapping(size + round);
        }
    }
    mapping(size)
}

/// A two-level segregated fit allocator over a region of memory.
///
/// The lifetime is the region's. Every pointer this hands out points into that
/// region, and the bookkeeping lives there too, so the allocator may not
/// outlive it. `'a` is what enforces that: without it, `new` would be a safe
/// function that turns a borrow into an owned value and every later `allocate`
/// would be a use-after-free reachable from entirely safe code.
///
/// The marker is what ties `'a` to the struct, since the pointers below are raw
/// and carry no lifetime of their own. It is invariant in `'a`, which is the
/// conservative choice and the right one here: the region is written through,
/// not merely read.
///
/// Outliving the region is refused, and the refusal is pinned here so that
/// loosening the bound breaks the suite rather than silently restoring a
/// use-after-free reachable from safe code:
///
/// ```compile_fail,E0597
/// # // The error code is documentation, not enforcement: rustdoc on the pinned 1.64 toolchain
/// # // accepts any code here. What binds is the block below failing to compile at all, which it
/// # // stops doing the moment the lifetime is removed.
/// use core::alloc::Layout;
/// use dollop::{Strategy, Tlsf};
///
/// let mut alloc = {
///     let mut region = [0u8; 4096];
///     Tlsf::new(&mut region).unwrap()
/// };
/// let _ = alloc.allocate(Layout::from_size_align(64, 8).unwrap());
/// ```
///
/// The same program with the region outliving the allocator is accepted, which
/// is also the worked example for the crate:
///
/// ```
/// use core::alloc::Layout;
///
/// use dollop::{Strategy, Tlsf};
///
/// let mut region = [0u8; 4096];
/// let mut alloc =
///     Tlsf::new(&mut region).expect("the region holds at least one block");
///
/// let layout = Layout::from_size_align(64, 8).unwrap();
/// let ptr = alloc.allocate(layout).expect("a fresh region has room");
/// unsafe { alloc.deallocate(ptr, layout) };
/// ```
pub struct Tlsf<'a> {
    fl_bitmap:  usize,
    sl_bitmaps: [usize; FL_COUNT],
    heads:      [[*mut Header; SL_COUNT]; FL_COUNT],
    free:       usize,
    region:     PhantomData<&'a mut [u8]>,
}

impl<'a> Tlsf<'a> {
    /// Creates an allocator that hands out parts of `region`.
    ///
    /// Returns `None` when the region is too small to hold a single block. The
    /// allocator borrows the region for its whole life and writes its
    /// bookkeeping into it.
    pub fn new(region: &'a mut [u8]) -> Option<Self> {
        let mut this = Tlsf {
            fl_bitmap:  0,
            sl_bitmaps: [0; FL_COUNT],
            heads:      [[null_mut(); SL_COUNT]; FL_COUNT],
            free:       0,
            region:     PhantomData,
        };

        let start = region.as_mut_ptr();
        let base = align_up(start as usize, ALIGN);
        let head_room = base - start as usize;
        if region.len() <= head_room + size_of::<Header>() + MIN_PAYLOAD {
            return None;
        }
        let usable = region.len() - head_room - size_of::<Header>();
        let payload = usable & !(ALIGN - 1);
        if payload < MIN_PAYLOAD {
            return None;
        }

        // safety: the region is at least this long, checked above
        let header = base as *mut Header;
        unsafe {
            header.write(Header {
                size_and_flags: payload | FLAG_FREE | FLAG_LAST,
                prev_phys:      null_mut(),
            });
            this.insert_free(header);
        }
        Some(this)
    }

    /// Files a free block under its size class.
    unsafe fn insert_free(&mut self, block: *mut Header) {
        let size = (*block).size();
        let (fl, sl) = mapping(size);
        let head = self.heads[fl][sl];

        (*block).links().write(FreeLinks {
            next: head,
            prev: null_mut(),
        });
        if !head.is_null() {
            (*(*head).links()).prev = block;
        }
        self.heads[fl][sl] = block;
        self.fl_bitmap |= 1 << fl;
        self.sl_bitmaps[fl] |= 1 << sl;
        (*block).set_flag(FLAG_FREE, true);
        self.free += size;
    }

    /// Takes a free block off its size class list.
    unsafe fn remove_free(&mut self, block: *mut Header) {
        let size = (*block).size();
        let (fl, sl) = mapping(size);
        let links = &*(*block).links();
        let (next, prev) = (links.next, links.prev);

        if !next.is_null() {
            (*(*next).links()).prev = prev;
        }
        if prev.is_null() {
            self.heads[fl][sl] = next;
            if next.is_null() {
                self.sl_bitmaps[fl] &= !(1 << sl);
                if self.sl_bitmaps[fl] == 0 {
                    self.fl_bitmap &= !(1 << fl);
                }
            }
        } else {
            (*(*prev).links()).next = next;
        }
        (*block).set_flag(FLAG_FREE, false);
        self.free -= size;
    }

    /// The smallest filed block that is at least `size`, if there is one.
    fn find_free(&self, size: usize) -> *mut Header {
        let (fl, sl) = mapping_for_request(size);
        if fl >= FL_COUNT {
            return null_mut();
        }

        // the classes in this first level that are big enough
        let mut sl_map = self.sl_bitmaps[fl] & (!0usize << sl);
        let mut fl_index = fl;
        if sl_map == 0 {
            // nothing here, so take the next first level that has anything at all
            let fl_map = self.fl_bitmap & (!0usize << (fl + 1));
            if fl_map == 0 {
                return null_mut();
            }
            fl_index = fl_map.trailing_zeros() as usize;
            sl_map = self.sl_bitmaps[fl_index];
            if sl_map == 0 {
                return null_mut();
            }
        }
        let sl_index = sl_map.trailing_zeros() as usize;
        self.heads[fl_index][sl_index]
    }

    /// Splits `block` so it holds exactly `size`, filing the remainder if one
    /// is worth keeping.
    unsafe fn split(&mut self, block: *mut Header, size: usize) {
        let total = (*block).size();
        let needed = size + size_of::<Header>();
        if total < needed + MIN_PAYLOAD {
            // the leftover could not hold a block, so the whole thing goes to the caller
            return;
        }

        let rest_size = total - needed;
        // the remainder's header sits immediately after this block's payload, which is
        // where next_phys() looks for it
        let rest = (*block).payload().add(size) as *mut Header;
        let was_last = (*block).is_last();

        (*block).set_size(size);
        (*block).set_flag(FLAG_LAST, false);

        rest.write(Header {
            size_and_flags: rest_size,
            prev_phys:      block,
        });
        (*rest).set_flag(FLAG_LAST, was_last);

        let after = (*rest).next_phys();
        if !after.is_null() {
            (*after).prev_phys = rest;
        }
        self.insert_free(rest);
    }

    /// Merges `block` with the free blocks on either side of it, and files the
    /// result.
    unsafe fn coalesce_and_insert(&mut self, block: *mut Header) {
        let mut block = block;

        let next = (*block).next_phys();
        if !next.is_null() && (*next).is_free() {
            self.remove_free(next);
            let grown = (*block).size() + size_of::<Header>() + (*next).size();
            let next_was_last = (*next).is_last();
            (*block).set_size(grown);
            (*block).set_flag(FLAG_LAST, next_was_last);
            let after = (*block).next_phys();
            if !after.is_null() {
                (*after).prev_phys = block;
            }
        }

        let prev = (*block).prev_phys;
        if !prev.is_null() && (*prev).is_free() {
            self.remove_free(prev);
            let grown = (*prev).size() + size_of::<Header>() + (*block).size();
            let block_was_last = (*block).is_last();
            (*prev).set_size(grown);
            (*prev).set_flag(FLAG_LAST, block_was_last);
            let after = (*prev).next_phys();
            if !after.is_null() {
                (*after).prev_phys = prev;
            }
            block = prev;
        }

        self.insert_free(block);
    }
}

// SAFETY: every block handed out comes from `split_block`, which carves it from
// a free block whose header records a size the block genuinely has, rounds the
// payload address up to `layout.align()`, and unlinks the block from the free
// lists before returning it. So a live block is aligned, is as large as it was
// asked to be, and is reachable from no other allocation until `deallocate`
// puts it back.
unsafe impl Strategy for Tlsf<'_> {
    fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        if layout.size() == 0 {
            return None;
        }
        let align = layout.align().max(ALIGN);
        let size = align_up(layout.size().max(MIN_PAYLOAD), ALIGN);
        // An alignment wider than a block's own needs room to shift the payload up to
        // it. The front that is shifted past becomes a block of its own, so
        // when one boundary does not leave enough room for that the shift goes
        // up a further `align`, and the search covers the wider case.
        let search = if align > ALIGN { size + align + size_of::<Header>() } else { size };

        let block = self.find_free(search);
        if block.is_null() {
            return None;
        }

        unsafe {
            self.remove_free(block);

            let payload = (*block).payload() as usize;
            let mut aligned = align_up(payload, align);
            if aligned != payload && aligned - payload < size_of::<Header>() + MIN_PAYLOAD {
                // The front would be too narrow to be a block, and filing a block with no
                // payload would write its free links over the header that
                // follows it. The next boundary up always leaves room, and the
                // search reserved for it.
                aligned += align;
            }
            let mut block = block;
            if aligned != payload {
                // the front of this block cannot be used, so it becomes a block of its own. The
                // gap is wide enough to hold one, because the search asked for the alignment on
                // top of the size.
                let gap = aligned - payload - size_of::<Header>();
                let front_size = (*block).size();
                let was_last = (*block).is_last();

                let shifted = (aligned - size_of::<Header>()) as *mut Header;
                let rest = front_size - gap - size_of::<Header>();

                (*block).set_size(gap);
                (*block).set_flag(FLAG_LAST, false);

                shifted.write(Header {
                    size_and_flags: rest,
                    prev_phys:      block,
                });
                (*shifted).set_flag(FLAG_LAST, was_last);

                let after = (*shifted).next_phys();
                if !after.is_null() {
                    (*after).prev_phys = shifted;
                }
                self.insert_free(block);
                block = shifted;
            }

            self.split(block, size);
            debug_assert!(
                !(*block).is_free(),
                "a handed out block is not on a free list"
            );
            debug_assert_eq!(
                (*block).payload() as usize % align,
                0,
                "the payload meets the requested alignment"
            );
            NonNull::new((*block).payload())
        }
    }

    unsafe fn deallocate(&mut self, ptr: NonNull<u8>, _layout: Layout) {
        let block = ptr.as_ptr().sub(size_of::<Header>()) as *mut Header;
        debug_assert!(!(*block).is_free(), "a block is not freed twice");
        self.coalesce_and_insert(block);
    }

    fn free_bytes(&self) -> usize {
        self.free
    }
}

#[cfg(test)]
mod invariants {
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
            // coalescing) are reached.
            let allocating = live.is_empty() || rng.below(100) < 60;

            if allocating {
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
}
