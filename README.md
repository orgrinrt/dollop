# dollop

<div align="center" style="text-align: center;">

[![GitHub Stars](https://img.shields.io/github/stars/orgrinrt/dollop.svg)](https://github.com/orgrinrt/dollop/stargazers)
[![Crates.io Total Downloads](https://img.shields.io/crates/d/dollop)](https://crates.io/crates/dollop)
[![GitHub Issues](https://img.shields.io/github/issues/orgrinrt/dollop.svg)](https://github.com/orgrinrt/dollop/issues)
[![Latest Version](https://img.shields.io/badge/version-0.0.1-red.svg?label=latest)](https://github.com/orgrinrt/dollop)
![Crates.io Version](https://img.shields.io/crates/v/dollop?logoSize=auto&color=%23FDC700&link=https%3A%2F%2Fcrates.io%2Fcrates%2Fdollop)
![Crates.io Size](https://img.shields.io/crates/size/dollop?color=%23C27AFF&link=https%3A%2F%2Fcrates.io%2Fcrates%2Fdollop)
![GitHub last commit](https://img.shields.io/github/last-commit/orgrinrt/dollop?color=%23009689&link=https%3A%2F%2Fgithub.com%2Forgrinrt%2Fdollop)

> An experimental allocator implementing several strategies with common api patterns for more convenient reuse.


</div>

dollop is an early work in progress. No allocator is implemented yet: the workspace scaffolding and
cargo feature flags are in place, but the public api has not landed. The workspace also contains
`impligen`, a proc-macro crate for masquerading implicit generics for struct implementations,
developed alongside the allocator.

## Features

| Feature  | Status    | Description                                           |
|----------|-----------|-------------------------------------------------------|
| `tlsf`   | ❓Planned  | tlsf (two-level segregated fit) allocator             |
| `no_std` | ❓Planned  | support for environments without the standard library |
| `std`    | ❓Planned  | standard library support                              |

The `tlsf`, `no_std` and `std` cargo features are declared in the manifest, but none of them gates
working code yet.

## Usage

There is no usable allocator api yet. Usage examples will be added once the first allocator
(`tlsf`) lands.

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
