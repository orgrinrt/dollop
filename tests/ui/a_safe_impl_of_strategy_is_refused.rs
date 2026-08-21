//! A safe `impl Strategy` is what made arbitrary writes reachable from entirely safe code:
//! `allocate` hands back a pointer that `Lease` writes through, so the pointer's validity is
//! the implementor's obligation and the trait has to say so. This file is the proof that it
//! does, and it is the case that must fail.

use core::alloc::Layout;
use core::ptr::NonNull;

use dollop::Strategy;

struct Liar;

impl Strategy for Liar {
    fn allocate(&mut self, _layout: Layout) -> Option<NonNull<u8>> {
        NonNull::new(0x1000 as *mut u8)
    }

    unsafe fn deallocate(&mut self, _ptr: NonNull<u8>, _layout: Layout) {}
}

fn main() {}
