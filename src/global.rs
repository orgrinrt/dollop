//! A strategy as the program's global allocator.
//!
//! [`GlobalAlloc`] is reached through a shared reference from every thread at
//! once, and a [`Strategy`] is written against `&mut self`, so something has
//! to sit between them and hand out that exclusive access one caller at a
//! time. [`Global`] is that something: a spin lock around a strategy, with the
//! strategy built on first use because a `static` has to be constructed in a
//! `const` context and a region cannot be borrowed in one.
//!
//! The lock spins rather than parks, because parking needs an operating system
//! and this is the allocator underneath whatever the program has. The critical
//! section is one allocation, which for the strategies here is a few dozen
//! instructions, so a spinner waits a very short time.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::{null_mut, NonNull};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::strategy::Strategy;

/// A strategy behind a lock, shaped to be a `#[global_allocator]`.
///
/// The strategy is built by `init` the first time the allocator is used, so
/// the `static` this lives in needs nothing but a function pointer at
/// construction. The region the strategy manages is whatever `init` hands it:
/// a `static mut` array is the usual shape, and the example below is the
/// whole of what a program has to write.
///
/// ```
/// use dollop::{Global, Tlsf};
///
/// static mut REGION: [u8; 1 << 16] = [0; 1 << 16];
///
/// // What a program declares to run on this. Not declared here, since the
/// // doctest binary has an allocator already; `examples/global_allocator.rs`
/// // does declare it.
/// // #[global_allocator]
/// static HEAP: Global<Tlsf<'static>> = Global::new(|| {
///     // SAFETY: this runs once, under the allocator's own lock, and nothing
///     // else names REGION.
///     Tlsf::new(unsafe { &mut *core::ptr::addr_of_mut!(REGION) })
///         .expect("64 KiB holds a heap")
/// });
///
/// let before = HEAP.free_bytes();
/// let layout = core::alloc::Layout::new::<[u64; 8]>();
/// let block = unsafe { core::alloc::GlobalAlloc::alloc(&HEAP, layout) };
/// assert!(!block.is_null());
/// assert!(HEAP.free_bytes() < before);
/// unsafe { core::alloc::GlobalAlloc::dealloc(&HEAP, block, layout) };
/// assert_eq!(HEAP.free_bytes(), before);
/// ```
///
/// `init` must not allocate through this allocator, since it runs while the
/// lock is held and a second attempt to take it would spin forever.
pub struct Global<S> {
    busy: AtomicBool,
    slot: UnsafeCell<Option<S>>,
    init: fn() -> S,
}

// SAFETY: `slot` is only ever reached through `with`, which holds `busy` for
// the duration, so no two threads hold a reference into it at once. `S: Send`
// is what makes moving that access between threads sound.
unsafe impl<S: Send> Sync for Global<S> {}

/// Releases the lock when it goes out of scope, panic or not, so a strategy
/// that panics inside its critical section does not leave every later caller
/// spinning on a lock nobody holds.
struct Release<'a>(&'a AtomicBool);

impl Drop for Release<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl<S: Strategy> Global<S> {
    /// An allocator that builds its strategy with `init` on first use.
    pub const fn new(init: fn() -> S) -> Self {
        Self {
            busy: AtomicBool::new(false),
            slot: UnsafeCell::new(None),
            init,
        }
    }

    /// Runs `f` with exclusive access to the strategy, building it first if
    /// this is the first use.
    ///
    /// This is how anything the [`GlobalAlloc`] surface does not expose is
    /// reached: [`Strategy::free_bytes`], or a strategy's own methods. `f`
    /// must not allocate through this allocator, for the same reason `init`
    /// must not.
    pub fn with<R>(&self, f: impl FnOnce(&mut S) -> R) -> R {
        while self
            .busy
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        let _release = Release(&self.busy);

        // SAFETY: `busy` was taken above and is held until `_release` drops, so
        // this is the only reference into the slot for as long as it lives.
        let slot = unsafe { &mut *self.slot.get() };
        f(slot.get_or_insert_with(self.init))
    }

    /// How many bytes the strategy has not handed out, taking the lock to ask.
    pub fn free_bytes(&self) -> usize {
        self.with(|strategy| strategy.free_bytes())
    }
}

// SAFETY: every call takes the lock, so the strategy sees one caller at a
// time, which is what its `&mut self` contract needs. Beyond that the
// guarantees are the strategy's own: `Strategy` is an unsafe trait carrying
// exactly the obligations `GlobalAlloc` places on `alloc`, and a null pointer
// is what `GlobalAlloc` wants back where the strategy answered `None`.
unsafe impl<S: Strategy + Send> GlobalAlloc for Global<S> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.with(|strategy| {
            strategy
                .allocate(layout)
                .map_or(null_mut(), NonNull::as_ptr)
        })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `GlobalAlloc::dealloc` is only ever handed a pointer `alloc`
        // returned, and `alloc` never returns null.
        let ptr = NonNull::new_unchecked(ptr);
        self.with(|strategy| strategy.deallocate(ptr, layout));
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as for `dealloc`.
        let ptr = NonNull::new_unchecked(ptr);
        self.with(|strategy| {
            strategy
                .reallocate(ptr, layout, new_size)
                .map_or(null_mut(), NonNull::as_ptr)
        })
    }
}
