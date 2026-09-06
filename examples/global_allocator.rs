//! The crate's allocator underneath a whole program.
//!
//! `Global` puts a strategy behind a lock and a `GlobalAlloc` impl, so every
//! `Vec`, `String` and `Box` in this program comes out of the array declared
//! below. Nothing here asks the operating system for memory.
//!
//! ```text
//! cargo run --example global_allocator --features global
//! ```

use dollop::{Global, Tlsf};

/// The whole program's heap, and the only place any of it comes from.
static mut REGION: [u8; 1 << 20] = [0; 1 << 20];

#[global_allocator]
static HEAP: Global<Tlsf<'static>> = Global::new(|| {
    // SAFETY: this runs once, under the allocator's own lock, and nothing else
    // names REGION.
    Tlsf::new(unsafe { &mut *core::ptr::addr_of_mut!(REGION) }).expect("1 MiB holds a heap")
});

fn main() {
    // The first print is what allocates stdout's line buffer, and that buffer
    // lives for the rest of the process, so the baseline is taken after it.
    // Measured before it, the closing count is short by exactly that buffer.
    println!(
        "a 1 MiB region, with {} bytes free before stdout took its buffer",
        HEAP.free_bytes()
    );
    let at_start = HEAP.free_bytes();
    println!("and {at_start} bytes free after\n");

    // Ordinary std collections, allocating through the region.
    let mut words: Vec<String> = Vec::new();
    for i in 0 .. 1_000 {
        words.push(format!("word number {i}"));
    }
    let joined = words.join(", ");
    println!(
        "{} strings joined into one of {} bytes, {} free",
        words.len(),
        joined.len(),
        HEAP.free_bytes()
    );

    // Growing a vector in place is what `Strategy::reallocate` is for: the
    // block after it is free, so the vector's block stretches rather than
    // moving and copying.
    let mut numbers: Vec<u64> = Vec::with_capacity(16);
    let first_address = numbers.as_ptr() as usize;
    numbers.extend(0 .. 4_096);
    println!(
        "a vector grew from 16 to {} slots, {}",
        numbers.capacity(),
        if numbers.as_ptr() as usize == first_address {
            "where it stood"
        } else {
            "by moving"
        }
    );

    drop(joined);
    drop(words);
    drop(numbers);
    println!(
        "\neverything dropped, {} bytes free, the same as at the start",
        HEAP.free_bytes()
    );
    assert_eq!(HEAP.free_bytes(), at_start);
}
