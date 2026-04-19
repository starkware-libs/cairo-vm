use crate::types::relocatable::MaybeRelocatable;

/// Loads a compiled Cairo `.json` program from the same directory as the calling source file.
///
/// Pass only the filename (no directory prefix). The directory is inferred from the call site
/// via `file!()`, so the `.json` must live next to the `.cairo` and `.rs` files.
///
/// # Example
/// ```rust,ignore
/// static PROGRAM: LazyLock<Program> = LazyLock::new(|| load_cairo_program!("main_math_test.json"));
/// ```
///
/// # Panics
/// - If the `.json` file does not exist: run `make tests_cairo_programs` first.
/// - If the `.json` file cannot be parsed as a Cairo `Program`.
#[macro_export]
macro_rules! load_cairo_program {
    ($name:literal) => {{
        // CARGO_MANIFEST_DIR is the `vm/` crate dir; workspace root is one level up.
        // file!() expands at the call site — with_file_name replaces the filename portion
        // so the JSON is resolved relative to the calling source file's directory.
        let json_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("vm crate should have a parent directory")
            .join(file!())
            .with_file_name($name);

        let bytes = match std::fs::read(&json_path) {
            Ok(b) => b,
            Err(err) => panic!(
                "Cairo program not found at {json_path:?}: {err}\n\
                 Did you run `make cairo_test_suite_programs`?"
            ),
        };

        match $crate::types::program::Program::from_bytes(&bytes, None) {
            Ok(p) => p,
            Err(e) => panic!("Failed to parse Cairo program at {json_path:?}: {e}"),
        }
    }};
}

/// Coerces `value` into a [`MaybeRelocatable`], panicking on failure.
///
/// The return type pins the conversion target to [`MaybeRelocatable`], so
/// callers cannot accidentally infer a different `TryInto` target. `side`
/// is used purely to disambiguate the panic message (e.g. `"left"` / `"right"`).
#[track_caller]
fn coerce_to_mr<T>(value: T, side: &str) -> MaybeRelocatable
where
    T: TryInto<MaybeRelocatable>,
    T::Error: core::fmt::Debug,
{
    match value.try_into() {
        Ok(v) => v,
        Err(e) => panic!("{side} conversion to MaybeRelocatable failed: {e:?}"),
    }
}

/// Asserts that two values are equal after converting both to [`MaybeRelocatable`].
///
/// If the left conversion fails, the panic message says "left conversion … failed".
/// If the right conversion fails, it says "right conversion … failed".
#[track_caller]
pub fn assert_mr_eq<L, R>(left: L, right: R)
where
    L: TryInto<MaybeRelocatable>,
    L::Error: core::fmt::Debug,
    R: TryInto<MaybeRelocatable>,
    R::Error: core::fmt::Debug,
{
    let left_mr = coerce_to_mr(left, "left");
    let right_mr = coerce_to_mr(right, "right");
    assert_eq!(left_mr, right_mr);
}

#[cfg(test)]
mod tests {
    use super::assert_mr_eq;
    use crate::types::relocatable::{MaybeRelocatable, Relocatable};
    use rstest::rstest;

    /// A type whose `TryInto<MaybeRelocatable>` always fails, used to exercise
    /// the conversion-failure panic branch in `assert_mr_eq`.
    struct AlwaysFailConversion;

    impl TryFrom<AlwaysFailConversion> for MaybeRelocatable {
        type Error = &'static str;
        fn try_from(_: AlwaysFailConversion) -> Result<Self, Self::Error> {
            Err("intentional failure")
        }
    }

    /// `load_cairo_program!` successfully loads a compiled Cairo program from the same directory.
    ///
    /// The source `dummy.cairo` used to produce `dummy.json` is:
    /// ```cairo
    /// func main() {
    ///     return ();
    /// }
    /// ```
    #[test]
    fn load_cairo_program_loads_dummy() {
        let program = load_cairo_program!("dummy.json");
        assert!(!program.shared_program_data.data.is_empty());
    }

    /// `load_cairo_program!` panics when the file does not exist.
    #[test]
    #[should_panic(expected = "Cairo program not found")]
    fn load_cairo_program_panics_on_missing_file() {
        load_cairo_program!("nonexistent.json");
    }

    // --- assert_mr_eq: passing cases ---

    #[rstest]
    #[case::int(MaybeRelocatable::from(42), MaybeRelocatable::from(42))]
    #[case::relocatable(
        MaybeRelocatable::from(Relocatable::from((1, 5))),
        MaybeRelocatable::from(Relocatable::from((1, 5)))
    )]
    fn assert_mr_eq_passes(#[case] left: MaybeRelocatable, #[case] right: MaybeRelocatable) {
        assert_mr_eq(left, right);
    }

    // --- assert_mr_eq: mismatch panics ---

    #[rstest]
    #[case::int_mismatch(MaybeRelocatable::from(1), MaybeRelocatable::from(2))]
    #[case::felt_vs_relocatable(
        MaybeRelocatable::from(1),
        MaybeRelocatable::from(Relocatable::from((0, 1)))
    )]
    #[case::relocatable_diff_segment(
        MaybeRelocatable::from(Relocatable::from((0, 5))),
        MaybeRelocatable::from(Relocatable::from((1, 5)))
    )]
    #[case::relocatable_diff_offset(
        MaybeRelocatable::from(Relocatable::from((1, 0))),
        MaybeRelocatable::from(Relocatable::from((1, 1)))
    )]
    #[should_panic]
    fn assert_mr_eq_panics_on_mismatch(
        #[case] left: MaybeRelocatable,
        #[case] right: MaybeRelocatable,
    ) {
        assert_mr_eq(left, right);
    }

    // --- assert_mr_eq: conversion failure panics ---

    /// Panics with "right conversion" message when right `try_into` fails.
    #[test]
    #[should_panic(expected = "right conversion to MaybeRelocatable failed")]
    fn assert_mr_eq_panics_on_right_conversion_failure() {
        let val = MaybeRelocatable::from(42);
        assert_mr_eq(&val, AlwaysFailConversion);
    }

    /// Panics with "left conversion" message when left `try_into` fails.
    #[test]
    #[should_panic(expected = "left conversion to MaybeRelocatable failed")]
    fn assert_mr_eq_panics_on_left_conversion_failure() {
        let val = MaybeRelocatable::from(42);
        assert_mr_eq(AlwaysFailConversion, &val);
    }
}
