use crate::types::layout_name::LayoutName;
use crate::vm::trace::trace_entry::RelocatedTraceEntry;

use crate::{
    cairo_run::{cairo_run, Cairo0RunConfig},
    hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor,
};

mod bitwise_test;
#[cfg(test)]
mod run_deprecated_contract_class_simplified;

mod cairo_run_test;
mod pedersen_test;
mod segment_arena_test;
mod struct_test;

mod cairo_pie_test;
#[cfg(feature = "test_utils")]
mod skip_instruction_test;

//For simple programs that should just succeed and have no special needs.
//Checks memory holes == 0
fn run_program_simple(data: &[u8]) {
    run_program(data, false, None, None, None)
}

//For simple programs that should just succeed but using small layout.
fn run_program_small(data: &[u8]) {
    run_program(data, false, Some(LayoutName::small), None, None)
}

fn run_program_with_trace(data: &[u8], trace: &[(usize, usize, usize)]) {
    run_program(data, false, None, Some(trace), None)
}

fn run_program_with_error(data: &[u8], error: &str) {
    run_program(data, false, None, None, Some(error))
}

fn run_program(
    data: &[u8],
    proof_mode: bool,
    layout: Option<LayoutName>,
    trace: Option<&[(usize, usize, usize)]>,
    error: Option<&str>,
) {
    let mut hint_executor = BuiltinHintProcessor::new_empty();
    let cairo_run_config = Cairo0RunConfig {
        layout: layout.unwrap_or(LayoutName::all_cairo),
        relocate_mem: true,
        trace_enabled: true,
        proof_mode,
        fill_holes: proof_mode,
        ..Default::default()
    };
    let res = cairo_run(data, &cairo_run_config, &mut hint_executor);
    if let Some(error) = error {
        assert!(res.is_err());
        assert!(res.err().unwrap().to_string().contains(error));
        return;
    }
    let runner = res.expect("Execution failed");
    if let Some(trace) = trace {
        let expected_trace: Vec<_> = trace
            .iter()
            .copied()
            .map(|(pc, ap, fp)| RelocatedTraceEntry { pc, ap, fp })
            .collect();
        let trace = runner.relocated_trace.as_ref().unwrap();
        assert_eq!(trace.len(), expected_trace.len());
        for (entry, expected) in trace.iter().zip(expected_trace.iter()) {
            assert_eq!(entry, expected);
        }
    }
}
