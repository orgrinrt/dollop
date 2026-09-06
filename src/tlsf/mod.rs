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

// SAFETY: the raw pointers here all point into the region, which the
// allocator holds by exclusive borrow for its whole life, so moving the
// allocator to another thread moves the only access to that memory along with
// it. That is exactly what the `&'a mut [u8]` it was built from would be
// allowed to do. It is what lets a `Tlsf<'static>` be a `#[global_allocator]`
// behind `Global`.
unsafe impl Send for Tlsf<'_> {}

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

    /// Splits `block` so it holds exactly `size`, returning the remainder as a
    /// block of its own, or null when the leftover could not hold one and the
    /// whole thing stays with `block`.
    ///
    /// The remainder is not filed. What to do with it depends on who is asking:
    /// a fresh allocation files it, since its neighbours cannot be free, and a
    /// shrink merges it first, since the block after it may well be.
    unsafe fn split_off(&mut self, block: *mut Header, size: usize) -> *mut Header {
        let total = (*block).size();
        let needed = size + size_of::<Header>();
        if total < needed + MIN_PAYLOAD {
            // the leftover could not hold a block, so the whole thing goes to the caller
            return null_mut();
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
        rest
    }

    /// Splits `block` so it holds exactly `size`, filing the remainder if one
    /// is worth keeping.
    ///
    /// For a block whose neighbours are not free, which a block just taken off
    /// a free list is guaranteed to be: two free blocks are never left side by
    /// side, so the remainder can be filed as it is.
    unsafe fn split(&mut self, block: *mut Header, size: usize) {
        let rest = self.split_off(block, size);
        if !rest.is_null() {
            self.insert_free(rest);
        }
    }

    /// Takes the free block after `block` into it, and returns whether there
    /// was one to take.
    unsafe fn absorb_next(&mut self, block: *mut Header) -> bool {
        let next = (*block).next_phys();
        if next.is_null() || !(*next).is_free() {
            return false;
        }
        self.remove_free(next);
        let grown = (*block).size() + size_of::<Header>() + (*next).size();
        let next_was_last = (*next).is_last();
        (*block).set_size(grown);
        (*block).set_flag(FLAG_LAST, next_was_last);
        let after = (*block).next_phys();
        if !after.is_null() {
            (*after).prev_phys = block;
        }
        true
    }

    /// The payload size a request of `size` bytes gets.
    #[inline]
    fn payload_for(size: usize) -> usize {
        align_up(size.max(MIN_PAYLOAD), ALIGN)
    }

    /// Merges `block` with the free blocks on either side of it, and files the
    /// result.
    unsafe fn coalesce_and_insert(&mut self, block: *mut Header) {
        let mut block = block;

        self.absorb_next(block);

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
        let size = Self::payload_for(layout.size());
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

    /// Resizes where the block stands when it can, and moves it when it cannot.
    ///
    /// Shrinking always stays in place: the tail comes off as a block of its
    /// own and merges with whatever free block follows. Growing stays in place
    /// when the block after this one is free and the two together are enough,
    /// and otherwise takes the copying path. Either way the address a caller
    /// gets back is the one to return later, with the new size.
    unsafe fn reallocate(
        &mut self,
        ptr: NonNull<u8>,
        layout: Layout,
        new_size: usize,
    ) -> Option<NonNull<u8>> {
        if new_size == 0 {
            return None;
        }
        let block = ptr.as_ptr().sub(size_of::<Header>()) as *mut Header;
        debug_assert!(!(*block).is_free(), "a block being resized is live");
        let wanted = Self::payload_for(new_size);
        let current = (*block).size();

        if wanted <= current {
            // The tail becomes a block of its own. Unlike a fresh split, the block
            // after it may be free, so the tail is merged rather than filed.
            let rest = self.split_off(block, wanted);
            if !rest.is_null() {
                self.coalesce_and_insert(rest);
            }
            return Some(ptr);
        }

        let next = (*block).next_phys();
        let room_after = if next.is_null() || !(*next).is_free() {
            0
        } else {
            size_of::<Header>() + (*next).size()
        };
        if current + room_after >= wanted {
            // The free neighbour is taken whole and the surplus split back off.
            // Its own neighbour after it cannot be free, so the surplus is filed
            // as it is.
            self.absorb_next(block);
            self.split(block, wanted);
            debug_assert!((*block).size() >= wanted);
            return Some(ptr);
        }

        // No room where it stands, so it moves. The alignment is the layout's,
        // which a fresh allocation honours, and the old block goes back once the
        // bytes are across.
        let new_layout = Layout::from_size_align(new_size, layout.align()).ok()?;
        let new = self.allocate(new_layout)?;
        core::ptr::copy_nonoverlapping(ptr.as_ptr(), new.as_ptr(), layout.size().min(new_size));
        self.coalesce_and_insert(block);
        Some(new)
    }

    fn free_bytes(&self) -> usize {
        self.free
    }
}

#[cfg(test)]
mod invariants;
