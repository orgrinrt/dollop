//! Allocator strategies behind a shared API.
//!
//! Each strategy manages a region of memory it is handed and differs in how it picks a free block
//! and tracks the ones it is not using. They share the [`Strategy`] contract, so swapping one for
//! another changes the type named and nothing else.
//!
//! [`Tlsf`] is the first, a two-level segregated fit allocator: it finds a block big enough in a
//! fixed number of steps rather than by walking a list, and merges neighbouring free blocks as
//! they are returned.
//!
//! ```
//! use core::alloc::Layout;
//! use dollop::{Strategy, Tlsf};
//!
//! let mut region = [0u8; 4096];
//! let mut alloc = Tlsf::new(&mut region).expect("the region holds at least one block");
//!
//! let layout = Layout::from_size_align(64, 8).unwrap();
//! let ptr = alloc.allocate(layout).expect("a fresh region has room");
//! unsafe { alloc.deallocate(ptr, layout) };
//! ```

#![cfg_attr(feature = "no_std", no_std)]

mod strategy;
mod tlsf;

pub use crate::strategy::Strategy;
pub use crate::tlsf::Tlsf;
