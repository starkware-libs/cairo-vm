use std::sync::LazyLock;

use crate::{math_utils::is_quad_residue, utils::CAIRO_PRIME};
use num_bigint::BigUint;
use num_integer::Integer;

/// RC_BOUND = 2^128
pub static RC_BOUND: LazyLock<BigUint> = LazyLock::new(|| BigUint::from(2u64).pow(128));

/// MAX_DIV = CAIRO_PRIME // RC_BOUND
pub static MAX_DIV: LazyLock<BigUint> = LazyLock::new(|| CAIRO_PRIME.div_floor(&RC_BOUND));

/// Returns 1 if `a` is a quadratic residue modulo CAIRO_PRIME, 0 if not, and -1 on error.
pub fn is_quad_residue_mod_prime(a: &BigUint) -> i64 {
    match is_quad_residue(a, &CAIRO_PRIME) {
        Ok(true) => 1,
        Ok(false) => 0,
        Err(_) => -1,
    }
}
