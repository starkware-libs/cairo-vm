use cairo_vm::{
    cairo_run::{cairo_run_fuzzed_program, Cairo0RunConfig},
    hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor,
    types::program::Program,
};
use honggfuzz::fuzz;

const STEPS_LIMIT: usize = 1000000;
fn main() {
    loop {
        fuzz!(|data: (Cairo0RunConfig, Program)| {
            let (cairo_config, program) = data;
            let _ = cairo_run_fuzzed_program(
                program.clone(),
                &Cairo0RunConfig::default(),
                &mut BuiltinHintProcessor::new_empty(),
                STEPS_LIMIT,
            );
            let _ = cairo_run_fuzzed_program(
                program,
                &cairo_config,
                &mut BuiltinHintProcessor::new_empty(),
                STEPS_LIMIT,
            );
        });
    }
}
