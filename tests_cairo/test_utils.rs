/// Asserts that a `MaybeRelocatable` reference equals a value convertible into `MaybeRelocatable`.
#[macro_export]
macro_rules! assert_mr_eq {
    ($left:expr, $right:expr) => {{
        let right_mr = ($right)
            .try_into()
            .unwrap_or_else(|e| panic!("conversion to MaybeRelocatable failed: {e:?}"));
        assert_eq!($left, &right_mr);
    }};
    ($left:expr, $right:expr, $($arg:tt)+) => {{
        let right_mr = ($right)
            .try_into()
            .unwrap_or_else(|e| panic!("conversion to MaybeRelocatable failed: {e:?}"));
        assert_eq!($left, &right_mr, $($arg)+);
    }};
}
