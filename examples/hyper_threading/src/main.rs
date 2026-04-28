use cairo_vm::{
    cairo_run::{cairo_run_program, Cairo0RunConfig},
    hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor,
    types::{layout_name::LayoutName, program::Program},
};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::path::Path;

// Define build_filename macro to prepend a relative path to the file names
macro_rules! build_filename {
    ($fname:expr) => {
        format!("cairo_programs/benchmarks/{}", $fname)
    };
}

fn main() {
    let mut programs = Vec::new();

    let program_filenames: [String; 18] = [
        build_filename!("big_factorial.json"),
        build_filename!("big_fibonacci.json"),
        build_filename!("blake2s_integration_benchmark.json"),
        build_filename!("compare_arrays_200000.json"),
        build_filename!("dict_integration_benchmark.json"),
        build_filename!("field_arithmetic_get_square_benchmark.json"),
        build_filename!("integration_builtins.json"),
        build_filename!("keccak_integration_benchmark.json"),
        build_filename!("linear_search.json"),
        build_filename!("math_cmp_and_pow_integration_benchmark.json"),
        build_filename!("math_integration_benchmark.json"),
        build_filename!("memory_integration_benchmark.json"),
        build_filename!("operations_with_data_structures_benchmarks.json"),
        build_filename!("pedersen.json"),
        build_filename!("poseidon_integration_benchmark.json"),
        build_filename!("secp_integration_benchmark.json"),
        build_filename!("set_integration_benchmark.json"),
        build_filename!("uint256_integration_benchmark.json"),
    ];

    let n_programs = &program_filenames.len();

    for filename in program_filenames {
        programs.push(Program::from_file(Path::new(&filename), Some("main")).unwrap())
    }

    let start_time = std::time::Instant::now();

    // Parallel execution of the program processing
    programs.into_par_iter().for_each(|program| {
        let cairo_run_config = Cairo0RunConfig {
            entrypoint: "main",
            trace_enabled: false,
            relocate_mem: false,
            layout: LayoutName::all_cairo,
            proof_mode: true,
            fill_holes: true,
            secure_run: Some(false),
            ..Default::default()
        };
        let mut hint_executor = BuiltinHintProcessor::new_empty();

        // Execute each program in parallel
        let _result = cairo_run_program(&program, &cairo_run_config, &mut hint_executor)
            .expect("Couldn't run program");
    });
    let elapsed = start_time.elapsed();

    tracing::info!(%n_programs, ?elapsed, "Finished");
}
