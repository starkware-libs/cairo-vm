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
        // file!() expands at the call site, giving the path of the calling source file
        // relative to the workspace root (e.g. "tests_cairo/math/math_test.rs").
        // We derive the directory from it and join with the requested filename.
        let source_dir = std::path::Path::new(file!())
            .parent()
            .expect("source file should have a parent directory");
        // CARGO_MANIFEST_DIR is the `vm/` crate dir; workspace root is one level up.
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("vm crate should have a parent directory");
        let json_path = workspace_root.join(source_dir).join($name);

        let bytes = std::fs::read(&json_path).unwrap_or_else(|err| {
            panic!(
                "Cairo program not found at {json_path:?}: {err}\n\
                 Did you run `make tests_cairo_programs`?"
            )
        });

        cairo_vm::types::program::Program::from_bytes(&bytes, None)
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
