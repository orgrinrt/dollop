//! Handing an allocation to something written against notko's lending contract.
//!
//! [`Lend`] is the protocol for storage a caller obtained and is passing on: fill part of
//! it, give back the part you filled, and say how much was wanted if it did not fit. It is
//! deliberately silent about where the memory came from, because that question belongs to
//! whoever obtained it.
//!
//! This is the other side of that silence. A [`Strategy`] is exactly a thing that obtains
//! memory, so a block from one is exactly what a `Lend` wants to be, and [`Lease`] is the
//! join: it takes a block, presents it as a slice of `T`, and returns it when it is
//! dropped.
//!
//! Nothing here allocates in the `alloc` sense. The memory is the region the strategy was
//! handed at construction, which may be a static array, a page from the operating system,
//! or a slice of a larger arena.

use core::alloc::Layout;
use core::marker::PhantomData;
use core::ptr::NonNull;

use notko::lend::Lend;

use crate::strategy::Strategy;

/// A block taken from an allocator, presented as storage that can be lent.
///
/// Holds the allocator borrowed for as long as it lives, and returns the block on drop, so
/// a leak is not expressible: forgetting to deallocate would mean forgetting to drop, which
/// the borrow checker does not allow while the allocator is still wanted.
///
/// The slots are initialised on creation, because [`Lend`] hands out `&mut [T]` and a
/// caller writing into a slot assigns over what was there, which for a type with a
/// destructor means running one. Uninitialised memory presented as `&mut [T]` would make
/// that undefined, so `T: Default` is the price and it is paid once, at creation.
pub struct Lease<'a, S: Strategy, T> {
    strategy: &'a mut S,
    ptr:      NonNull<u8>,
    layout:   Layout,
    count:    usize,
    _held:    PhantomData<T>,
}

impl<'a, S: Strategy, T: Default> Lease<'a, S, T> {
    /// Takes `count` slots from `strategy`, or `None` when it has no room for them.
    ///
    /// `None` for a zero count, because a lend of nothing is not useful.
    ///
    /// A zero-sized `T` is not the same thing and does succeed: it needs no memory, so the
    /// block is a dangling aligned pointer and the allocator is never asked.
    pub fn take(strategy: &'a mut S, count: usize) -> Option<Self> {
        if count == 0 {
            return None;
        }

        // `array` rather than multiplying by hand, so an overflowing count is refused here
        // rather than wrapping into a small allocation that the writes then run past.
        let layout = Layout::array::<T>(count).ok()?;

        // A zero-sized `T` needs no memory, and asking an allocator for zero bytes is a
        // question it is right to refuse: this crate's own allocator does, deliberately and
        // with a test saying so. So the block is a dangling pointer, aligned for `T`, which
        // is what `&mut [T]` requires of a zero-sized element and what the standard
        // library's own collections use.
        //
        // Found by a test whose counter type happened to be a unit struct, which is exactly
        // the shape somebody reaches for when counting destructor runs.
        let ptr = if layout.size() == 0 {
            NonNull::<T>::dangling().cast::<u8>()
        } else {
            strategy.allocate(layout)?
        };

        // Every slot initialised before anything can observe them. `write` rather than
        // assignment, because assignment would drop whatever the uninitialised bytes
        // happened to look like.
        for index in 0 .. count {
            // SAFETY: `ptr` is valid for `layout.size()` bytes, which is `count` slots of
            // `T` laid out end to end, and `index` is below `count`. The alignment is
            // `layout.align()`, which is `T`'s, so each slot address is aligned for `T`.
            unsafe {
                ptr.cast::<T>().as_ptr().add(index).write(T::default());
            }
        }

        Some(Self { strategy, ptr, layout, count, _held: PhantomData })
    }

}

impl<S: Strategy, T> Lease<'_, S, T> {
    /// How many slots this holds.
    ///
    /// No `T: Default` bound: only taking a lease needs one, and a caller holding a lease
    /// of a type it obtained some other way can still ask.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether it holds none, which [`take`](Lease::take) never produces.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl<S: Strategy, T> Lend<T> for Lease<'_, S, T> {
    fn lend(&mut self) -> &mut [T] {
        // SAFETY: the block is `count` slots of `T`, aligned for `T`, and every slot was
        // initialised in `take`. The borrow is tied to `&mut self`, so nothing else can
        // reach the block while this slice exists, and the block outlives the slice because
        // it is only returned in `drop`.
        unsafe { core::slice::from_raw_parts_mut(self.ptr.cast::<T>().as_ptr(), self.count) }
    }
}

impl<S: Strategy, T> Drop for Lease<'_, S, T> {
    fn drop(&mut self) {
        // Every slot holds an initialised `T`, so each one's destructor runs. For a `T`
        // with no destructor this compiles to nothing.
        for index in 0 .. self.count {
            // SAFETY: as in `take`, and each slot is dropped exactly once because this runs
            // exactly once.
            unsafe {
                self.ptr.cast::<T>().as_ptr().add(index).drop_in_place();
            }
        }

        // A zero-sized block never came from the allocator, so it does not go back to one.
        if self.layout.size() == 0 {
            return;
        }

        // SAFETY: `ptr` came from `allocate` on this same allocator with this same layout,
        // and has not been deallocated, because this is the only place that deallocates it
        // and it runs once.
        unsafe {
            self.strategy.deallocate(self.ptr, self.layout);
        }
    }
}

/// Taking a lease from any allocator.
///
/// A blanket implementation over [`Strategy`], so every allocator in this crate and every
/// one a consumer writes gets it without doing anything.
pub trait Leasing: Strategy + Sized {
    /// Takes `count` slots of `T`, or `None` when there is no room.
    ///
    /// ```
    /// # #[cfg(all(feature = "tlsf", feature = "no_alloc"))] {
    /// use dollop::{Leasing, Tlsf};
    /// use notko::lend::Fill;
    ///
    /// let mut region = [0u8; 1024];
    /// let mut allocator = Tlsf::new(&mut region).expect("1 KiB is enough for a heap");
    ///
    /// let mut lease = allocator.lease::<u32>(4).expect("and enough for four u32");
    ///
    /// // Anything written against notko's contract takes it from here without knowing
    /// // where the memory came from.
    /// let mut fill = Fill::new(&mut lease);
    /// assert!(fill.extend([10, 20, 30]).is_ok());
    /// assert_eq!(fill.finish(), &[10, 20, 30]);
    /// # }
    /// ```
    fn lease<T: Default>(&mut self, count: usize) -> Option<Lease<'_, Self, T>> {
        Lease::take(self, count)
    }
}

impl<S: Strategy + Sized> Leasing for S {}
