//! A lease is a real allocation, and giving it up really returns the block.
//!
//! The interesting claims are the ones a passing fill cannot show. A `Lease` that never
//! deallocated would fill just as happily, and so would one that never ran a destructor:
//! both look identical from the filled slice, and both are leaks. What separates them is
//! what the allocator has left afterwards, and what a type with a destructor observed.

#![cfg(all(feature = "tlsf", feature = "no_alloc"))]

use dollop::{Fill, Leasing, Outcome, Strategy, Tlsf};

/// A region big enough for the leases here and small enough that exhausting it is quick.
const REGION: usize = 4096;

#[test]
fn a_lease_is_storage_a_fill_can_take() {
    let mut region = [0u8; REGION];
    let mut allocator = Tlsf::new(&mut region).expect("the region holds a heap");
    let mut lease = allocator.lease::<u32>(4).expect("and four u32");

    let mut fill = Fill::new(&mut lease);
    assert_eq!(fill.capacity(), 4);
    assert!(fill.extend([10, 20, 30]).is_ok());
    assert_eq!(fill.finish(), &[10, 20, 30]);
}

#[test]
fn a_lease_returns_its_block_when_it_is_dropped() {
    let mut region = [0u8; REGION];
    let mut allocator = Tlsf::new(&mut region).expect("the region holds a heap");

    let before = allocator.free_bytes();

    {
        let mut lease = allocator.lease::<u64>(16).expect("sixteen u64");
        assert!(allocator_is_borrowed(&mut lease));
    }

    assert_eq!(
        allocator.free_bytes(),
        before,
        "the block was not returned, so a lease leaks its allocation",
    );
}

/// Reads the lease, which is what borrows the allocator for the block's lifetime.
///
/// Exists so the scope above holds the lease rather than dropping it immediately, and so
/// the compiler cannot decide the lease was unused.
fn allocator_is_borrowed<S: Strategy, T>(lease: &mut dollop::Lease<'_, S, T>) -> bool {
    !lease.is_empty()
}

#[test]
fn taking_and_returning_repeatedly_does_not_lose_the_region() {
    let mut region = [0u8; REGION];
    let mut allocator = Tlsf::new(&mut region).expect("the region holds a heap");
    let before = allocator.free_bytes();

    // A leak of even one block per round shows here within a few rounds, where a single
    // take-and-drop could hide it in the allocator's own rounding.
    for round in 0 .. 64 {
        let mut lease = allocator.lease::<u32>(8).expect("eight u32 on round {round}");
        let mut fill = Fill::new(&mut lease);
        assert!(fill.extend([round; 8]).is_ok());
    }

    assert_eq!(allocator.free_bytes(), before, "the region shrank across 64 rounds");
}

#[test]
fn a_lease_larger_than_the_region_is_refused() {
    let mut region = [0u8; 256];
    let mut allocator = Tlsf::new(&mut region).expect("the region holds a heap");

    assert!(
        allocator.lease::<u64>(1024).is_none(),
        "a lease that does not fit must be refused rather than truncated",
    );

    // And the refusal costs nothing: the allocator is untouched and still works.
    let before = allocator.free_bytes();
    assert!(allocator.lease::<u8>(1).is_some());
    assert_eq!(allocator.free_bytes(), before, "a refused lease consumed something");
}

#[test]
fn a_lease_of_nothing_is_refused() {
    let mut region = [0u8; REGION];
    let mut allocator = Tlsf::new(&mut region).expect("the region holds a heap");

    // A zero-sized layout is not something an allocator should be asked for, and a lend of
    // nothing is not useful.
    assert!(allocator.lease::<u32>(0).is_none());
}

#[test]
fn a_count_that_would_overflow_is_refused_rather_than_wrapping() {
    let mut region = [0u8; REGION];
    let mut allocator = Tlsf::new(&mut region).expect("the region holds a heap");

    // `count * size_of::<T>()` wraps for a large enough count, and a wrapped product is a
    // small allocation that the initialising writes then run straight past. `Layout::array`
    // refuses instead, which is why the size is computed through it.
    assert!(allocator.lease::<u64>(usize::MAX).is_none());
    assert!(allocator.lease::<u64>(usize::MAX / 4).is_none());
}

#[test]
fn every_slot_starts_at_the_default() {
    let mut region = [0u8; REGION];
    let mut allocator = Tlsf::new(&mut region).expect("the region holds a heap");

    // Written into the block by `take`, because `Lend` hands out `&mut [T]` and a caller
    // assigning into a slot drops what was there. Reading them back is what says the write
    // happened rather than the memory happening to be zero.
    {
        let mut lease = allocator.lease::<u32>(8).expect("eight u32");
        assert_eq!(lease.len(), 8);
        assert_eq!(dollop::Lend::lend(&mut lease), &[0u32; 8]);
    }

    // A default that is not the zero pattern, so this cannot pass on a fresh region that
    // happened to be zeroed.
    #[derive(PartialEq, Debug)]
    struct Marked(u32);
    impl Default for Marked {
        fn default() -> Self {
            Self(0xDEAD_BEEF)
        }
    }

    let mut marked = allocator.lease::<Marked>(4).expect("four marked");
    assert_eq!(
        dollop::Lend::lend(&mut marked),
        &[Marked(0xDEAD_BEEF), Marked(0xDEAD_BEEF), Marked(0xDEAD_BEEF), Marked(0xDEAD_BEEF)],
    );
}

#[test]
fn dropping_a_lease_runs_the_destructor_of_every_slot() {
    use core::cell::Cell;

    // A counter the destructors can reach without allocating, since this crate is `no_std`
    // and the point of the exercise is that nothing here needs an allocator.
    thread_local! {
        static DROPPED: Cell<usize> = const { Cell::new(0) };
    }

    #[derive(Default)]
    struct Counted;
    impl Drop for Counted {
        fn drop(&mut self) {
            DROPPED.with(|d| d.set(d.get() + 1));
        }
    }

    let mut region = [0u8; REGION];
    let mut allocator = Tlsf::new(&mut region).expect("the region holds a heap");

    {
        let lease = allocator.lease::<Counted>(5).expect("five counted");
        assert_eq!(lease.len(), 5);
        assert_eq!(DROPPED.with(Cell::get), 0, "nothing has been dropped yet");
    }

    // Five, not one: the block holds five initialised values and each one's destructor has
    // to run. A `Lease` that only freed the bytes would leave this at zero.
    assert_eq!(
        DROPPED.with(Cell::get),
        5,
        "the slots' destructors did not run, so a lease leaks whatever its slots own",
    );
}

#[test]
fn what_a_lend_refuses_says_how_much_it_wanted() {
    let mut region = [0u8; REGION];
    let mut allocator = Tlsf::new(&mut region).expect("the region holds a heap");
    let mut lease = allocator.lease::<u16>(2).expect("two u16");

    let mut fill = Fill::new(&mut lease);
    match fill.extend([1, 2, 3, 4]) {
        Outcome::Ok(()) => panic!("four items fitted into two slots"),
        Outcome::Err(exhausted) => {
            assert_eq!(exhausted.had, 2, "it reports what the lease held");
            assert_eq!(exhausted.wanted, 4, "and how much the batch needed");
        },
    }

    // All or nothing, so the refused batch left the lease untouched and it is still usable.
    assert!(fill.is_empty());
    assert!(fill.extend([7, 8]).is_ok());
    assert_eq!(fill.finish(), &[7, 8]);
}

#[test]
fn a_zero_sized_type_leases_without_asking_the_allocator() {
    // A zero-sized `T` needs no memory, and this crate's allocator refuses a zero-byte
    // request deliberately, with a test saying so. Taken together that would make
    // `lease::<SomeUnitStruct>(n)` fail, which is what the destructor test above ran into:
    // a unit struct is exactly the shape somebody reaches for when counting drops.
    //
    // The block is a dangling aligned pointer instead, which is what `&mut [T]` wants of a
    // zero-sized element and what the standard library's own collections do.
    let mut region = [0u8; REGION];
    let mut allocator = Tlsf::new(&mut region).expect("the region holds a heap");
    let before = allocator.free_bytes();

    #[derive(Default, PartialEq, Debug)]
    struct Nothing;

    {
        let mut lease = allocator.lease::<Nothing>(5).expect("five of nothing");
        assert_eq!(lease.len(), 5);
        assert_eq!(dollop::Lend::lend(&mut lease).len(), 5);
    }

    assert_eq!(
        allocator.free_bytes(),
        before,
        "a zero-sized lease took something from the allocator, or gave back more",
    );
}

#[test]
fn the_allocator_still_refuses_a_zero_byte_request_directly() {
    // The control for the case above. `Lease` sidesteps the refusal for zero-sized types
    // rather than removing it, and the refusal is the allocator's own documented behaviour.
    use core::alloc::Layout;

    let mut region = [0u8; REGION];
    let mut allocator = Tlsf::new(&mut region).expect("the region holds a heap");

    let zero = Layout::from_size_align(0, 1).expect("a zero-sized layout");
    assert!(
        allocator.allocate(zero).is_none(),
        "the allocator no longer refuses zero bytes, so the sidestep above is unnecessary",
    );
}
