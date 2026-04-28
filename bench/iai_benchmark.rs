use cairo_vm::cairo_run::Cairo0RunConfig;
use core::hint::black_box;
use iai_callgrind::library_benchmark;
use iai_callgrind::library_benchmark_group;
use iai_callgrind::main;
use iai_callgrind::LibraryBenchmarkConfig;

use cairo_vm::{
    types::{layout_name::LayoutName, program::Program},
    vm::runners::cairo_runner::CairoRunner,
};

use mimalloc::MiMalloc;

#[global_allocator]
static ALLOC: MiMalloc = MiMalloc;

#[library_benchmark]
fn parse_program() {
    //Picked the biggest one at the time of writing
    let program = include_bytes!("../cairo_programs/benchmarks/keccak_integration_benchmark.json");
    let program =
        Program::from_bytes(black_box(program.as_slice()), black_box(Some("main"))).unwrap();
    core::mem::drop(black_box(program));
}

#[export_name = "helper::parse_program"]
#[inline(never)]
fn parse_program_helper() -> Program {
    //Picked the biggest one at the time of writing
    let program = include_bytes!("../cairo_programs/benchmarks/keccak_integration_benchmark.json");
    Program::from_bytes(program.as_slice(), Some("main")).unwrap()
}

#[library_benchmark]
fn build_runner() {
    let program = parse_program_helper();
    let runner = CairoRunner::new(
        black_box(&program),
        &Cairo0RunConfig {
            layout: LayoutName::starknet_with_keccak,
            proof_mode: false,
            trace_enabled: false,
            disable_trace_padding: false,
            ..Default::default()
        }
        .run_config()
        .unwrap(),
    );
    core::mem::drop(black_box(runner));
}

#[export_name = "helper::build_runner"]
#[inline(never)]
fn build_runner_helper() -> CairoRunner {
    //Picked the biggest one at the time of writing
    let program = include_bytes!("../cairo_programs/benchmarks/keccak_integration_benchmark.json");
    let program = Program::from_bytes(program.as_slice(), Some("main")).unwrap();
    CairoRunner::new(
        &program,
        &Cairo0RunConfig {
            layout: LayoutName::starknet_with_keccak,
            proof_mode: false,
            trace_enabled: false,
            disable_trace_padding: false,
            ..Default::default()
        }
        .run_config()
        .unwrap(),
    )
}

#[library_benchmark]
fn load_program_data() {
    let mut runner = build_runner_helper();
    _ = black_box(runner.initialize(false).unwrap());
}

library_benchmark_group!(
    name = cairo_vm_group;
    benchmarks =
        parse_program,
        build_runner,
        load_program_data
);

main!(
    config = LibraryBenchmarkConfig::default()
        .callgrind_args(["toggle-collect=helper::*,core::mem::drop"]);
    library_benchmark_groups = cairo_vm_group
);
