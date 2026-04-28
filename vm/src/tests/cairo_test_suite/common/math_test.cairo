%builtins range_check

from starkware.cairo.common.math import (
    assert_not_zero,
    assert_not_equal,
    assert_nn,
    assert_le,
    assert_lt,
    assert_nn_le,
    assert_in_range,
    assert_250_bit,
    split_felt,
    assert_le_felt,
    assert_lt_felt,
    abs_value,
    sign,
    unsigned_div_rem,
    signed_div_rem,
    safe_div,
    safe_mult,
    split_int,
    sqrt,
    horner_eval,
    is_quad_residue,
    assert_is_power_of_2,
)


func main{range_check_ptr}() {
    return ();
}
