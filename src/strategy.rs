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
/// arena.
///
/// # Safety
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
/// pointer is passed to [`Strategy::deallocate`] with the same layout, or to
/// [`Strategy::reallocate`], whose answer then carries the same guarantee at
/// the new size.
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

    /// Changes the size of a block, keeping its contents.
    ///
    /// Returns the block's new address, which may be the old one when the
    /// strategy could resize it where it stood. The first `min(old, new)`
    /// bytes are what they were; anything past the old size is uninitialised.
    /// A zero `new_size` is refused, as a zero-sized allocation is.
    ///
    /// On `None` nothing has happened: the old block is untouched and still
    /// the caller's to use and to return.
    ///
    /// The provided implementation allocates a new block, copies, and returns
    /// the old one, which is correct for any strategy. A strategy overrides
    /// it where it can do better, and both of this crate's do.
    ///
    /// # Safety
    ///
    /// As [`Strategy::deallocate`]: `ptr` must have come from
    /// [`Strategy::allocate`] on this same allocator with this same `layout`,
    /// and must not have been deallocated since.
    unsafe fn reallocate(
        &mut self,
        ptr: NonNull<u8>,
        layout: Layout,
        new_size: usize,
    ) -> Option<NonNull<u8>> {
        let new_layout = Layout::from_size_align(new_size, layout.align()).ok()?;
        let new = self.allocate(new_layout)?;
        // SAFETY: both blocks are valid for the length copied, since the old one
        // is `layout.size()` long and the new one `new_size`, and they do not
        // overlap because the old one has not been returned yet.
        core::ptr::copy_nonoverlapping(ptr.as_ptr(), new.as_ptr(), layout.size().min(new_size));
        self.deallocate(ptr, layout);
        Some(new)
    }

    /// How many bytes of the region are not currently handed out.
    ///
    /// This counts every free block, so an allocation of that size can still
    /// fail when the free space is split across blocks that are not next to
    /// each other.
    fn free_bytes(&self) -> usize;
}
