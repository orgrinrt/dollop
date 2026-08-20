//! Allocator strategies behind a shared API.
//!
//! Each strategy manages a region of memory it is handed and differs in how it picks a free block
//! and tracks the ones it is not using. They share the [`Strategy`] contract, so swapping one for
//! another changes the type named and nothing else.
//!
//! [`Tlsf`] is the first, a two-level segregated fit allocator: it finds a block big enough in a
//! fixed number of steps rather than by walking a list, and merges neighbouring free blocks as
//! they are returned. Its own documentation carries the worked example, because the example needs
//! the `tlsf` feature and this page does not.

#![cfg_attr(feature = "no_std", no_std)]

mod strategy;
#[cfg(feature = "tlsf")]
mod tlsf;

pub use crate::strategy::Strategy;
#[cfg(feature = "tlsf")]
pub use crate::tlsf::Tlsf;
