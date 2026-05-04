//! INTERNAL_RUNTIME_HELPER_TEST: this file intentionally exercises unavailable runtime internals.
//! Compatibility shim for legacy internal tests.
//!
//! Public-flow tests must import `public_featureforge_cli.rs` directly. New
//! internal-runtime tests should import `internal_runtime_direct.rs` directly.

#![allow(unused_imports)]

#[path = "internal_runtime_direct.rs"]
mod internal_runtime_direct;

pub use internal_runtime_direct::*;
