# dollop

<div align="center" style="text-align: center;">

[![GitHub Stars](https://img.shields.io/github/stars/orgrinrt/dollop.svg)](https://github.com/orgrinrt/dollop/stargazers)
[![GitHub Issues](https://img.shields.io/github/issues/orgrinrt/dollop.svg)](https://github.com/orgrinrt/dollop/issues)
[![Latest Version](https://img.shields.io/badge/version-0.0.1-red.svg?label=latest)](https://github.com/orgrinrt/dollop)
![GitHub last commit](https://img.shields.io/github/last-commit/orgrinrt/dollop?color=%23009689&link=https%3A%2F%2Fgithub.com%2Forgrinrt%2Fdollop)

> An experimental allocator implementing several strategies with common api patterns for more convenient reuse.

</div>

dollop is an early work in progress. One allocator has landed, `Tlsf`, behind the `Strategy`
contract described below. It is not a `GlobalAlloc` and allocation is single-threaded. The
workspace also contains `impligen`, a proc-macro crate for masquerading implicit generics for
struct implementations, developed alongside the allocator.

## Features

| Feature | Default | What it does |
|---|---|---|
| `tlsf` | yes | The two-level segregated fit allocator. Gates the module, so turning it off removes the code rather than leaving a switch that forwards nothing. |
| `no_std` | yes | Sets `#![no_std]`. There is no paired `std` feature, because nothing here needs one: with this off the crate compiles against std. |
| `no_alloc` | no | Adds `Lease`, which presents a block from any `Strategy` as storage satisfying notko's lending contract. Implies `no_std`. |

Nothing here allocates in the `alloc` sense under any selection. The memory is always the
region a strategy was handed at construction, and `no_alloc` is not about removing an
allocation but about joining this crate to everything written against a contract for storage
somebody else obtained.

`tests/feature_matrix.rs` builds every selection, and the table above is checked against the
manifest by `tests/readme.rs`, because a feature table is a claim about the manifest and this
one had drifted: it documented a `std` feature the manifest does not have, and said `tlsf`
gated nothing, which stopped being true.

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

`Strategy` is the shared contract: `allocate`, `deallocate`, and `free_bytes`. Strategies differ
in how they choose a free block and how they track the ones they are not using, so swapping one
for another changes the type named and nothing else.

`Tlsf` is the first. Free blocks are filed by size into classes, indexed by a pair: the first
level is the power of two the size falls in, the second splits that range into four. A bitmap per
level records which classes hold anything, so finding a block big enough is a matter of masking
off the classes that are too small and taking the lowest bit still set, rather than walking a
list. Blocks carry a header pointing at the block physically before them, so a block being freed
merges with the free neighbours on either side.

It is not a `GlobalAlloc` yet: that needs the allocator to be shared, which is a synchronisation
question this does not answer. Allocation is single-threaded for now.

## Examples

```text
cargo run --example one_region
cargo run --example lending_from_an_allocator --features no_alloc
```

The first takes three blocks of different sizes and alignments out of a 4 KiB array on the
stack and gives them all back, printing the free count at each step. The second is a whole
program with one region and nothing underneath it: a fixed-budget event buffer that batches
readings into a block taken from the allocator, summarises them, and returns it, with notko's
`Fill` doing the filling and knowing nothing about where the storage came from.

Both are run by `cargo test`, in `tests/examples_run.rs`, which reads the free counts back
and checks they match. An example that leaked one block per round would print a plausible
falling number and look fine.

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

> You can check out the full license [here](https://github.com/orgrinrt/dollop/blob/main/LICENSE)
