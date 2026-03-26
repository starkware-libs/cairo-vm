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
