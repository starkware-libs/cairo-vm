//! # An implementation of the Cairo virtual machine
//!
//! ## Feature Flags
//! - `test_utils`: Enables the following to help with tests (not enabled by default):
//!    - [`Hooks`](crate::vm::hooks::Hooks) support for the [VirtualMachine](vm::vm_core::VirtualMachine);
//!    - the `print_*` family of hints;
//!    - the `skip_next_instruction()` hints;
//!    - implementations of [`arbitrary::Arbitrary`](https://docs.rs/arbitrary/latest/arbitrary/) for some structs.
//! - `cairo-1-hints`: Enable hints that were introduced in Cairo 1. Not enabled by default.
//! - `cairo-0-secp-hints`: Enable secp hints that were introduced in Cairo 0. Not enabled by default.
//! - `cairo-0-data-availability-hints`: Enable data availability hints that were introduced in Cairo 0. Not enabled by default.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(warnings)]
#![forbid(unsafe_code)]

pub mod air_private_input;
pub mod air_public_input;
pub mod cairo_run;
pub mod hint_processor;
pub mod math_utils;
pub mod program_hash;
pub mod serde;
pub mod typed_operations;
pub mod types;
pub mod utils;
pub mod vm;

#[cfg(feature = "test_utils")]
pub mod test_helpers;

// TODO: use `Felt` directly
pub use starknet_types_core::felt::Felt as Felt252;

#[cfg(test)]
mod tests;
