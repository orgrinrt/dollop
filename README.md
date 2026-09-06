# `dollop`

<div align="center" style="text-align: center;">

[![GitHub Stars](https://img.shields.io/github/stars/orgrinrt/dollop.svg)](https://github.com/orgrinrt/dollop/stargazers)
[![Crates.io](https://img.shields.io/crates/v/dollop)](https://crates.io/crates/dollop)
[![docs.rs](https://img.shields.io/docsrs/dollop)](https://docs.rs/dollop)
[![GitHub Issues](https://img.shields.io/github/issues/orgrinrt/dollop.svg)](https://github.com/orgrinrt/dollop/issues)
![License](https://img.shields.io/github/license/orgrinrt/dollop?color=%23009689)

> An experimental allocator. Two strategies behind one api, and a way to be the global one.

</div>

## Status

Experimental. Two allocators have landed, `Tlsf` and `Bump`, behind the one
`Strategy` contract, and `Global` puts either behind a lock and a `GlobalAlloc`
impl so it can be a program's `#[global_allocator]`. A strategy on its own is a
value and is used from one thread at a time, which is the shape most arenas
want; the lock is a cost only the shared shape pays.

## Features

| Feature | Default | What it does |
|---|---|---|
| `tlsf` | yes | The two-level segregated fit allocator. Gates the module, so turning it off removes the code rather than leaving a switch that forwards nothing. |
| `bump` | yes | The bump allocator: an add and a compare per block, no headers, and only the most recent block comes back before the whole region does. |
| `global` | no | `Global`, a strategy behind a spin lock and a `GlobalAlloc` impl, which is what a `#[global_allocator]` has to be. Off by default because most consumers want an arena as a value and the lock is a cost only the shared shape pays. |
| `no_std` | no | Sets `#![no_std]`. There is no paired `std` feature, because nothing here needs one: with this off the crate compiles against std. It is off by default so that turning it on is the consumer's decision: cargo unifies features across a dependency graph, so a default `no_std` would put every consumer of every sibling crate into `no_std` without any of them asking. |
| `no_alloc` | no | Adds `Lease`, which presents a block from any `Strategy` as storage satisfying notko's lending contract. Implies `no_std`. |

Nothing here allocates in the `alloc` sense under any selection. The memory is always the
region a strategy was handed at construction, and `no_alloc` is not about removing an
allocation but about joining this crate to everything written against a contract for storage
somebody else obtained.

`tests/feature_matrix.rs` builds every selection, and the table above is checked against the
manifest by `tests/readme.rs`, because a feature table is a claim about the manifest and this
one had drifted: it documented a `std` feature the manifest does not have, and said `tlsf`
gated nothing, which stopped being true.

## Installation

Not published yet, so this does not resolve. It is the command once a release
lands.

```bash
cargo add dollop
```

Or in `Cargo.toml`:

```toml
[dependencies]
dollop = "0.0.1"
```

## Usage

An allocator manages a region of memory it is handed, so the caller decides where that region
comes from: a static array, a page from the operating system, or a slice of a larger arena.

```rust
use core::alloc::Layout;
use dollop::{Strategy, Tlsf};

let mut region = [0u8; 4096];
let mut alloc = Tlsf::new(&mut region).expect("the region holds at least one block");

let layout = Layout::from_size_align(64, 8).unwrap();
let ptr = alloc.allocate(layout).expect("a fresh region has room");
unsafe { alloc.deallocate(ptr, layout) };
```

### Strategies

`Strategy` is the shared contract: `allocate`, `deallocate`, `reallocate` and `free_bytes`.
Strategies differ in how they choose a free block and how they track the ones they are not
using, so swapping one for another changes the type named and nothing else. `reallocate`
has a provided implementation that allocates, copies and returns, which is correct for any
strategy, and both strategies here override it to resize where the block stands when they
can.

`Tlsf` is the first. Free blocks are filed by size into classes, indexed by a pair: the first
level is the power of two the size falls in, the second splits that range into four. A bitmap per
level records which classes hold anything, so finding a block big enough is a matter of masking
off the classes that are too small and taking the lowest bit still set, rather than walking a
list. Blocks carry a header pointing at the block physically before them, so a block being freed
merges with the free neighbours on either side.

`Bump` is the other end of the trade. A region and a mark: a block is the next `size` bytes
past the mark, aligned up, and the mark moves past it. Nothing is written per block, so
allocation is an add and a compare. What it gives up is release: a block in the middle of
the region cannot be taken back, because nothing remembers where it was, so only the most
recent block comes back and `reset` takes back everything at once. That is the right trade
for memory used in phases and thrown away together, a frame, a request, a parse.

### The global allocator

A strategy is written against `&mut self`, and a `GlobalAlloc` is reached through a shared
reference from every thread at once, so `Global` sits between them: a spin lock around a
strategy, built on first use because a `static` has to be constructed in a `const` context
and a region cannot be borrowed in one.

```rust
use dollop::{Global, Tlsf};

static mut REGION: [u8; 1 << 16] = [0; 1 << 16];

// #[global_allocator], in a program that wants this underneath everything
static HEAP: Global<Tlsf<'static>> = Global::new(|| {
    // SAFETY: runs once, under the allocator's own lock, and nothing else names REGION
    Tlsf::new(unsafe { &mut *core::ptr::addr_of_mut!(REGION) }).expect("64 KiB holds a heap")
});

# #[cfg(feature = "global")]
assert!(HEAP.free_bytes() > 0);
```

The lock spins rather than parks, because parking needs an operating system and this is the
allocator underneath whatever the program has. The critical section is one allocation.

## Examples

```text
cargo run --example one_region
cargo run --example phases
cargo run --example global_allocator --features global
cargo run --example lending_from_an_allocator --features no_alloc
```

The first takes three blocks of different sizes and alignments out of a 4 KiB array on the
stack and gives them all back, printing the free count at each step. The second runs three
frames of a small simulation out of a bump arena, resetting it between frames. The third
puts `Tlsf` underneath a whole program as its `#[global_allocator]`, so every `Vec` and
`String` in it comes out of a static array. The last is a whole program with one region and
nothing underneath it: a fixed-budget event buffer that batches readings into a block taken
from the allocator, summarises them, and returns it, with notko's `Fill` doing the filling
and knowing nothing about where the storage came from.

All four are run by `cargo test`, in `tests/examples_run.rs`, which reads the free counts
back and checks they match. An example that leaked one block per round would print a
plausible falling number and look fine.

## Compatibility

The default feature set requires rust `1.64.0` or later, for cargo's `workspace-inheritance`,
stabilized there.

`no_alloc` requires more, because notko is edition 2024 and cargo has no way to declare a
floor per feature. `tests/feature_matrix.rs` builds the default set under 1.64.0 and says so;
it is `#[ignore]`d, since it needs a toolchain most machines do not have.

The repository's `rust-toolchain.toml` says `stable`, not the minimum. It used to say
`1.64.0`, which meant every build here was an MSRV build and none was ever a current one.

### Versioning policy

Minor versions may have breaking changes, which can include bumping msrv.

Patch versions are backwards compatible, so using version specifiers such as `~x.y` or `^x.y.0` is safe.

## Support

Whether you use this project, have learned something from it, or just like it, please consider supporting it by buying me a coffee, so I can dedicate more time on open-source projects like this :)

<a href="https://buymeacoffee.com/orgrinrt" target="_blank"><img src="https://www.buymeacoffee.com/assets/img/custom_images/orange_img.png" alt="Buy Me A Coffee" style="height: auto !important;width: auto !important;" ></a>

## License

> The project is licensed under the **Mozilla Public License 2.0**.

`SPDX-License-Identifier: MPL-2.0`

> You can check out the full license [here](https://github.com/orgrinrt/dollop/blob/dev/LICENSE)
