//! # A `HintProcessor` for the hints introduced by the Cairo 1 compiler
//!
//! This lives outside the `cairo-vm` crate on purpose: executing Cairo 1 hints means speaking the
//! compiler's `Hint` AST, so this crate carries the `cairo-lang-*` dependency that `cairo-vm` does
//! not. Running Cairo 0 programs pulls in none of it.

#![deny(warnings)]
#![forbid(unsafe_code)]

pub mod circuit;
pub mod dict_manager;
pub mod hint_processor;
pub mod hint_processor_utils;
mod program;

pub use program::program_from_casm_contract_class;

#[cfg(test)]
mod tests;
