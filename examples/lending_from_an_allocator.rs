//! A whole program with one region of memory and no allocator underneath it.
//!
//! Composes the three pieces: a fixed region, this crate's allocator over it, and notko's
//! lending contract on top, so anything written against that contract takes storage from
//! here without knowing where it came from.
//!
//! The scenario is a fixed-budget event buffer: readings arrive, a batch of them is
//! collected into a block taken from the allocator, the batch is summarised, and the block
//! goes back. Every byte in the program comes from the array declared in `main`.
//!
//! ```text
//! cargo run --example lending_from_an_allocator --features no_alloc
//! ```

use dollop::{Exhausted, Fill, Leasing, Outcome, Strategy, Tlsf};

/// One reading from somewhere.
#[derive(Clone, Copy, Default, Debug)]
struct Reading {
    sensor: u8,
    value:  i16,
}

fn main() {
    // The whole program's memory, and the only place any of it comes from.
    let mut region = [0u8; 2048];
    let mut allocator = Tlsf::new(&mut region).expect("2 KiB is enough for a heap");

    println!("A 2048 byte region. Everything below comes out of it.\n");

    let readings = [
        Reading { sensor: 1, value: 20 },
        Reading { sensor: 1, value: 22 },
        Reading { sensor: 2, value: -5 },
        Reading { sensor: 2, value: -3 },
        Reading { sensor: 1, value: 25 },
        Reading { sensor: 3, value: 100 },
        Reading { sensor: 2, value: 0 },
        Reading { sensor: 3, value: 98 },
    ];

    // Four at a time, so the batching is visible and the block is taken and returned more
    // than once.
    for (batch, chunk) in readings.chunks(4).enumerate() {
        // The block. Borrowing the allocator for as long as it lives, and returning itself
        // at the end of this iteration, which is why the next one can take another.
        let mut lease = allocator
            .lease::<Reading>(4)
            .expect("four readings fit in what is left");

        // `Fill` is notko's, and knows nothing about dollop. It sees storage.
        let mut fill = Fill::new(&mut lease);
        match fill.extend(chunk.iter().copied()) {
            Outcome::Ok(()) => {},
            Outcome::Err(Exhausted { wanted, had }) => {
                println!("batch {batch}: wanted {wanted}, had {had}");
                continue;
            },
        }

        let filled = fill.finish();
        let sum: i32 = filled.iter().map(|r| i32::from(r.value)).sum();
        let sensors = distinct_sensors(filled);

        println!(
            "batch {batch}: {} readings, {sensors} sensors, sum {sum}",
            filled.len(),
        );
    }

    // Nothing above could ask the allocator how much was left, and that is the design
    // rather than an oversight: a lease borrows the allocator exclusively for as long as it
    // lives, so a leak is not expressible. Forgetting to return the block would mean
    // forgetting to drop the lease, and the allocator is unusable until that happens.

    println!("\nEvery block came back, so the region is whole again.\n");
    println!("{} bytes free, the same as at the start", allocator.free_bytes());

    println!("\nA lease larger than what is left is refused, and costs nothing.\n");

    // Scoped, so the refused lease is dropped before the allocator is read. The same
    // exclusive borrow is why: the `Option` holds a potential lease until the end of the
    // match, and until then nothing else may look at the allocator.
    let refused = { allocator.lease::<Reading>(4096).is_none() };
    if refused {
        println!("refused, and {} bytes are still free", allocator.free_bytes());
    } else {
        println!("unexpectedly took 4096 readings");
    }
}

/// How many distinct sensors a batch mentions, counted without allocating.
///
/// A bitmask over sensor ids, which is what "without allocating" looks like when the set is
/// small and bounded. Sensors above 63 are not counted, which the caller here has none of.
fn distinct_sensors(readings: &[Reading]) -> u32 {
    let mut seen: u64 = 0;
    for reading in readings {
        if reading.sensor < 64 {
            seen |= 1 << reading.sensor;
        }
    }
    seen.count_ones()
}
