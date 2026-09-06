//! Allocator strategies behind a shared API.
//!
//! Each strategy manages a region of memory it is handed and differs in how it
//! picks a free block and tracks the ones it is not using. They share the
//! [`Strategy`] contract, so swapping one for another changes the type named
//! and nothing else.
//!
//! [`Tlsf`] is a two-level segregated fit allocator: it finds a block big
//! enough in a fixed number of steps rather than by walking a list, and merges
//! neighbouring free blocks as they are returned. [`Bump`] is the other end of
//! the trade: an add and a compare per block, no headers, and nothing but the
//! most recent block ever comes back before the whole region does. Each
//! carries its own worked example, because each needs its own feature and
//! this page does not.
//!
//! [`Global`] puts any of them behind a lock and a [`core::alloc::GlobalAlloc`]
//! impl, which is what a `#[global_allocator]` has to be.

#![cfg_attr(feature = "no_std", no_std)]

#[cfg(feature = "bump")]
mod bump;
#[cfg(feature = "global")]
mod global;
#[cfg(feature = "no_alloc")]
mod lending;
mod strategy;
#[cfg(feature = "tlsf")]
mod tlsf;

// The lending contract answers in notko's types, so they are re-exported here.
// Without this a consumer has to take a direct dependency on notko in order to
// fill what this crate handed it, which is a dependency it did not choose and
// would have to keep in step.
#[cfg(feature = "no_alloc")]
pub use notko::lend::{Exhausted, Fill, Lend};
#[cfg(feature = "no_alloc")]
pub use notko::outcome::Outcome;

#[cfg(feature = "bump")]
pub use crate::bump::Bump;
#[cfg(feature = "global")]
pub use crate::global::Global;
#[cfg(feature = "no_alloc")]
pub use crate::lending::{Lease, Leasing};
pub use crate::strategy::Strategy;
#[cfg(feature = "tlsf")]
pub use crate::tlsf::Tlsf;
