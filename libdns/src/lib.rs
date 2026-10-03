//! Shared DNS wire format and resolver core for NeoDOS.
//!
//! This crate contains the protocol logic (RFC 1035 A-record resolution) with
//! no kernel or syscall dependency, so it can be unit tested on the host. The
//! `libnet` userland library provides the transport (UDP sockets via
//! `net.nxl`) and the Registry-backed configuration on top of it.
//!
//! Only A / IPv4 resolution is implemented (plus CNAME following).
//!
//! When built for NeoDOS it is `no_std`; under `cargo test` it uses the host
//! `std` test harness.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod types;
mod wire;
mod host;
mod resolve;
#[cfg(test)]
mod tests;

pub use types::*;
pub use wire::*;
pub use host::*;
pub use resolve::*;
