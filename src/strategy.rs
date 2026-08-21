//! The contract every allocation strategy in this crate satisfies.
//!
//! Strategies differ in how they choose a free block and how they keep track of
//! the ones they are not using. They agree on this surface, so a consumer can
//! swap one for another without changing anything but the type it names.

use core::alloc::Layout;
use core::ptr::NonNull;

/// An allocator over a region of memory it was handed.
///
/// A strategy owns the region for as long as it lives and hands out parts of
/// it. It does not acquire memory of its own, so it works the same with a
/// static array, a page from the operating system, or a slice of a larger
/// arena. # Safety
///
/// `allocate` returns a pointer that callers read and write through without an
/// `unsafe` block of their own, so the obligation is the implementor's and not
/// the caller's. That is what makes this trait unsafe, and it is the same
/// reason `core::alloc::GlobalAlloc` is: a safe trait here would let entirely
/// safe consumer code produce arbitrary writes by returning a pointer of its
/// choosing.
///
/// An implementation guarantees that every `Some(ptr)` it returns from
/// [`Strategy::allocate`] is aligned to `layout.align()`, is valid for reads
/// and writes over `layout.size()` bytes, does not overlap any other block it
/// has returned and not yet taken back, and stays valid until that exact
/// pointer is passed to [`Strategy::deallocate`] with the same layout.
pub unsafe trait Strategy {
    /// Allocates a block matching `layout`, or returns `None` when the region
    /// has no room for it.
    ///
    /// The returned pointer is aligned to `layout.align()` and valid for
    /// `layout.size()` bytes until it is passed to
    /// [`Strategy::deallocate`]. See the trait's safety contract.
    fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>>;

    /// Returns a block to the allocator.
    ///
    /// # Safety
    ///
    /// `ptr` must have come from [`Strategy::allocate`] on this same allocator,
    /// with this same `layout`, and must not have been deallocated since.
    unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout);

    /// How many bytes of the region are not currently handed out.
    ///
    /// This counts every free block, so an allocation of that size can still
    /// fail when the free space is split across blocks that are not next to
    /// each other.
    fn free_bytes(&self) -> usize;
}
