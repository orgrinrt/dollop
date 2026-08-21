//! One region, one allocator, blocks in and out of it.
//!
//! The smallest thing this crate does. The region is a fixed array on the stack, and the
//! allocator hands out parts of it and takes them back; nothing here asks the operating
//! system for anything.
//!
//! ```text
//! cargo run --example one_region
//! ```

use core::alloc::Layout;

use dollop::{Strategy, Tlsf};

fn main() {
    // The allocator manages a region it is handed. This one is on the stack; a page from
    // the operating system or a slice of a larger arena would work identically, which is
    // the whole point of it not acquiring memory of its own.
    let mut region = [0u8; 4096];
    let mut allocator = Tlsf::new(&mut region).expect("4 KiB is enough for a heap");

    println!("a 4096 byte region, with {} bytes free\n", allocator.free_bytes());

    // Three blocks of different sizes and alignments.
    let mut blocks = Vec::new();
    for (size, align) in [(64, 8), (128, 16), (32, 4)] {
        let layout = Layout::from_size_align(size, align).expect("a valid layout");
        let ptr = allocator.allocate(layout).expect("the region has room");

        println!(
            "took {size:>4} bytes aligned to {align:>2}, {} free, address aligned: {}",
            allocator.free_bytes(),
            ptr.as_ptr() as usize % align == 0,
        );
        blocks.push((ptr, layout));
    }

    println!("\ngiving them back, newest first\n");

    for (ptr, layout) in blocks.into_iter().rev() {
        // SAFETY: each came from `allocate` on this allocator with this layout, and none
        // has been returned yet.
        unsafe { allocator.deallocate(ptr, layout) };
        println!("returned {:>4} bytes, {} free", layout.size(), allocator.free_bytes());
    }

    println!("\nA request the region cannot hold is refused rather than half-served.\n");

    let too_big = Layout::from_size_align(1 << 20, 8).expect("a valid layout");
    println!("one megabyte: {:?}", allocator.allocate(too_big).map(|_| "took it"));

    // And the refusal costs nothing: the allocator is exactly as it was.
    println!("still {} bytes free afterwards", allocator.free_bytes());
}
