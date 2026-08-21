//! The control for `a_safe_impl_of_strategy_is_refused`. Without it that test passes whether
//! the trait is unsafe or the impl simply does not compile for some unrelated reason, because
//! a build failure on its own says nothing about which error produced it.

use core::alloc::Layout;
use core::ptr::NonNull;

use dollop::Strategy;

static mut REGION: [u8; 64] = [0; 64];

struct Honest;

// SAFETY: hands out the one region it owns, which is 64 bytes at an address the compiler
// aligned for `u8`, and refuses anything that does not fit or wants more alignment than it
// has. It never hands the same block out twice because it never hands out a second one.
unsafe impl Strategy for Honest {
    fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        if layout.size() > 64 || layout.align() > 1 {
            return None;
        }
        NonNull::new(&raw mut REGION as *mut u8)
    }

    unsafe fn deallocate(&mut self, _ptr: NonNull<u8>, _layout: Layout) {}

    fn free_bytes(&self) -> usize {
        64
    }
}

fn main() {}
