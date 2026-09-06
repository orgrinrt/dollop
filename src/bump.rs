//! Bump allocation.
//!
//! The simplest thing that is still an allocator: a region and a mark. A block
//! is the next `size` bytes past the mark, aligned up, and the mark moves past
//! it. Nothing is tracked per block, so allocation is an add and a compare and
//! the region carries no headers at all.
//!
//! What it gives up is release. A block in the middle of the region cannot be
//! taken back, because the mark has moved past it and there is no record of
//! where it was; only the most recent block can, by moving the mark back. That
//! is the whole trade, and it is the right one for memory that is used in
//! phases and thrown away together: a frame, a request, a parse.

use core::alloc::Layout;
use core::marker::PhantomData;
use core::ptr::NonNull;

use crate::strategy::Strategy;

#[inline]
fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

/// A bump allocator over a region of memory.
///
/// The lifetime is the region's, for the reason [`Tlsf`](crate::Tlsf) gives:
/// every pointer this hands out points into the region, so the allocator may
/// not outlive it, and `'a` is what says so to the borrow checker.
///
/// ```
/// use core::alloc::Layout;
///
/// use dollop::{Bump, Strategy};
///
/// let mut region = [0u8; 256];
/// let mut arena = Bump::new(&mut region);
///
/// let layout = Layout::from_size_align(64, 8).unwrap();
/// let a = arena.allocate(layout).expect("a fresh region has room");
/// let b = arena.allocate(layout).expect("and room for a second");
/// assert!(arena.free_bytes() < 256);
///
/// // Only the most recent block comes back. `a` is behind the mark with
/// // nothing remembering where it was, so returning it moves nothing.
/// let before = arena.free_bytes();
/// unsafe { arena.deallocate(a, layout) };
/// assert_eq!(arena.free_bytes(), before, "a is not the last block");
/// unsafe { arena.deallocate(b, layout) };
/// assert!(
///     arena.free_bytes() > before,
///     "b was the last block, and came back"
/// );
///
/// // Everything at once is what a bump allocator is for.
/// unsafe { arena.reset() };
/// assert_eq!(arena.free_bytes(), 256);
/// ```
pub struct Bump<'a> {
    base:   *mut u8,
    len:    usize,
    /// How far into the region the next block starts.
    mark:   usize,
    region: PhantomData<&'a mut [u8]>,
}

// SAFETY: `base` points into the region, which the allocator holds by
// exclusive borrow for its whole life, so moving the allocator moves the only
// access to that memory with it, as moving the `&'a mut [u8]` would.
unsafe impl Send for Bump<'_> {}

impl<'a> Bump<'a> {
    /// Creates an allocator that hands out parts of `region`, front to back.
    ///
    /// Never refuses: an empty region is an allocator that refuses every
    /// request, which is a consistent thing to be.
    pub fn new(region: &'a mut [u8]) -> Self {
        Self {
            base:   region.as_mut_ptr(),
            len:    region.len(),
            mark:   0,
            region: PhantomData,
        }
    }

    /// Takes every block back at once, whether or not it was returned.
    ///
    /// # Safety
    ///
    /// Every pointer this allocator has handed out is invalid afterwards. The
    /// caller has to be finished with all of them, which is the usual state of
    /// affairs at the end of the phase the region was for.
    pub unsafe fn reset(&mut self) {
        self.mark = 0;
    }

    /// How many bytes have been handed out and not taken back.
    ///
    /// The distance from the start of the region to the mark, which includes
    /// whatever alignment padding sat between blocks.
    #[must_use]
    pub const fn used_bytes(&self) -> usize {
        self.mark
    }

    /// Whether `ptr` with `size` is the most recent block, which is the only
    /// one that can be given back or changed in place.
    #[inline]
    fn is_last(&self, ptr: NonNull<u8>, size: usize) -> bool {
        let start = ptr.as_ptr() as usize;
        start + size == self.base as usize + self.mark
    }
}

// SAFETY: a block is the bytes between the mark before and the mark after an
// allocation, the mark only ever moves past a block or back to exactly where a
// block being returned started, and the end is checked against the region's
// length before the mark moves. So a live block is inside the region, aligned
// as asked, and overlapped by nothing handed out after it.
unsafe impl Strategy for Bump<'_> {
    fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        if layout.size() == 0 {
            return None;
        }
        let start = align_up(self.base as usize + self.mark, layout.align());
        let end = start.checked_add(layout.size())?;
        if end > self.base as usize + self.len {
            return None;
        }
        self.mark = end - self.base as usize;
        NonNull::new(start as *mut u8)
    }

    unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) {
        // Only the most recent block can come back. Any other is behind the mark
        // with no record of where it was, and stays taken until `reset`.
        if self.is_last(ptr, layout.size()) {
            self.mark = ptr.as_ptr() as usize - self.base as usize;
        }
    }

    unsafe fn reallocate(
        &mut self,
        ptr: NonNull<u8>,
        layout: Layout,
        new_size: usize,
    ) -> Option<NonNull<u8>> {
        if new_size == 0 {
            return None;
        }
        // The most recent block grows or shrinks by moving the mark, and keeps
        // its address. Anything else takes the copying path, and leaves the old
        // block where it was: it could not have been returned anyway.
        if self.is_last(ptr, layout.size()) {
            let start = ptr.as_ptr() as usize;
            let end = start.checked_add(new_size)?;
            if end > self.base as usize + self.len {
                return None;
            }
            self.mark = end - self.base as usize;
            return Some(ptr);
        }
        let new_layout = Layout::from_size_align(new_size, layout.align()).ok()?;
        let new = self.allocate(new_layout)?;
        core::ptr::copy_nonoverlapping(ptr.as_ptr(), new.as_ptr(), layout.size().min(new_size));
        Some(new)
    }

    fn free_bytes(&self) -> usize {
        self.len - self.mark
    }
}
