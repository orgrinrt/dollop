# dollop

<div align="center" style="text-align: center;">

[![GitHub Stars](https://img.shields.io/github/stars/orgrinrt/dollop.svg)](https://github.com/orgrinrt/dollop/stargazers)
[![GitHub Issues](https://img.shields.io/github/issues/orgrinrt/dollop.svg)](https://github.com/orgrinrt/dollop/issues)
[![Latest Version](https://img.shields.io/badge/version-0.0.1-red.svg?label=latest)](https://github.com/orgrinrt/dollop)
![GitHub last commit](https://img.shields.io/github/last-commit/orgrinrt/dollop?color=%23009689&link=https%3A%2F%2Fgithub.com%2Forgrinrt%2Fdollop)

> An experimental allocator implementing several strategies with common api patterns for more convenient reuse.

</div>

dollop is an early work in progress. No allocator is implemented yet: the workspace scaffolding and
cargo feature flags are in place, but the public api has not landed. The workspace also contains
`impligen`, a proc-macro crate for masquerading implicit generics for struct implementations,
developed alongside the allocator.

## Features

| Feature  | Status       | Description                                           |
|----------|--------------|-------------------------------------------------------|
| `tlsf`   | ❎ Implemented | tlsf (two-level segregated fit) allocator             |
| `no_std` | ❎ Implemented | support for environments without the standard library |
| `std`    | ❎ Implemented | standard library support                              |

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

## Compatibility

This crate requires rust `1.64.0` or later.

The msrv is pinned there to use cargo's `workspace-inheritance` feature, stabilized in `1.64.0`,
while staying compatible with older toolchains.

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
