//! Allocator strategies behind a shared API.
//!
//! Each strategy manages a region of memory it is handed and differs in how it
//! picks a free block and tracks the ones it is not using. They share the
//! [`Strategy`] contract, so swapping one for another changes the type named
//! and nothing else.
//!
//! [`Tlsf`] is the first, a two-level segregated fit allocator: it finds a
//! block big enough in a fixed number of steps rather than by walking a list,
//! and merges neighbouring free blocks as they are returned. Its own
//! documentation carries the worked example, because the example needs
//! the `tlsf` feature and this page does not.

#![cfg_attr(feature = "no_std", no_std)]

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

#[cfg(feature = "no_alloc")]
pub use crate::lending::{Lease, Leasing};
pub use crate::strategy::Strategy;
#[cfg(feature = "tlsf")]
pub use crate::tlsf::Tlsf;
