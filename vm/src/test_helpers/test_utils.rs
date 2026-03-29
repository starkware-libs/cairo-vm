/// Loads a compiled Cairo `.json` program from the same directory as the calling source file.
///
/// Pass only the filename (no directory prefix). The directory is inferred from the call site
/// via `file!()`, so the `.json` must live next to the `.cairo` and `.rs` files.
///
/// # Example
/// ```rust
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

        let bytes = std::fs::read(&json_path).unwrap_or_else(|err| {
            panic!(
                "Cairo program not found at {json_path:?}: {err}\n\
                 Did you run `make cairo_test_suite_programs`?"
            )
        });

        $crate::types::program::Program::from_bytes(&bytes, None)
            .unwrap_or_else(|e| panic!("Failed to parse Cairo program at {json_path:?}: {e}"))
    }};
}

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

#[cfg(test)]
mod tests {
    use crate::types::relocatable::{MaybeRelocatable, Relocatable};

    /// A type whose `TryInto<MaybeRelocatable>` always fails, used to exercise
    /// the `unwrap_or_else` panic branch in `assert_mr_eq!`.
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

    /// `assert_mr_eq!` passes when an integer `MaybeRelocatable` equals the given felt value.
    #[test]
    fn assert_mr_eq_int_passes() {
        let val = MaybeRelocatable::from(42);
        assert_mr_eq!(&val, 42);
    }

    /// `assert_mr_eq!` passes when a relocatable `MaybeRelocatable` equals the given pair.
    #[test]
    fn assert_mr_eq_relocatable_passes() {
        let val = MaybeRelocatable::from(Relocatable::from((1, 5)));
        assert_mr_eq!(&val, Relocatable::from((1, 5)));
    }

    /// `assert_mr_eq!` passes with a custom message format.
    #[test]
    fn assert_mr_eq_with_message_passes() {
        let val = MaybeRelocatable::from(7);
        assert_mr_eq!(&val, 7, "value at index {} should be 7", 0);
    }

    /// `assert_mr_eq!` panics when values differ.
    #[test]
    #[should_panic]
    fn assert_mr_eq_panics_on_mismatch() {
        let val = MaybeRelocatable::from(1);
        assert_mr_eq!(&val, 2);
    }

    /// `assert_mr_eq!` panics with a custom message when values differ.
    #[test]
    #[should_panic(expected = "wrong value")]
    fn assert_mr_eq_with_message_panics_on_mismatch() {
        let val = MaybeRelocatable::from(1);
        assert_mr_eq!(&val, 2, "wrong value");
    }

    /// `assert_mr_eq!` panics when comparing a felt against a relocatable.
    #[test]
    #[should_panic]
    fn assert_mr_eq_panics_felt_vs_relocatable() {
        let val = MaybeRelocatable::from(1);
        assert_mr_eq!(&val, Relocatable::from((0, 1)));
    }

    /// `assert_mr_eq!` panics when relocatables have the same offset but different segments.
    #[test]
    #[should_panic]
    fn assert_mr_eq_panics_relocatable_diff_segment() {
        let val = MaybeRelocatable::from(Relocatable::from((0, 5)));
        assert_mr_eq!(&val, Relocatable::from((1, 5)));
    }

    /// `assert_mr_eq!` panics when relocatables have the same segment but different offsets.
    #[test]
    #[should_panic]
    fn assert_mr_eq_panics_relocatable_diff_offset() {
        let val = MaybeRelocatable::from(Relocatable::from((1, 0)));
        assert_mr_eq!(&val, Relocatable::from((1, 1)));
    }

    /// `assert_mr_eq!` (no-message variant) panics when `try_into` conversion fails.
    #[test]
    #[should_panic(expected = "conversion to MaybeRelocatable failed")]
    fn assert_mr_eq_panics_on_conversion_failure() {
        let val = MaybeRelocatable::from(42);
        assert_mr_eq!(&val, AlwaysFailConversion);
    }

    /// `assert_mr_eq!` (message variant) panics when `try_into` conversion fails.
    #[test]
    #[should_panic(expected = "conversion to MaybeRelocatable failed")]
    fn assert_mr_eq_with_message_panics_on_conversion_failure() {
        let val = MaybeRelocatable::from(42);
        assert_mr_eq!(&val, AlwaysFailConversion, "should not reach assert_eq");
    }
}
