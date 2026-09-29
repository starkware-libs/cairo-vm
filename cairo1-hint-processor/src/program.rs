use std::collections::HashMap;

use cairo_lang_starknet_classes::casm_contract_class::CasmContractClass;
use cairo_vm::serde::deserialize_program::{
    ApTracking, FlowTrackingData, HintParams, ReferenceManager,
};
use cairo_vm::types::errors::program_errors::ProgramError;
use cairo_vm::types::program::Program;
use cairo_vm::types::relocatable::MaybeRelocatable;
use cairo_vm::Felt252;

/// Builds a [`Program`] out of a compiled contract class.
///
/// Note: the resulting `Program` will only work when using `run_from_entrypoint` together with
/// [`Cairo1HintProcessor`](crate::hint_processor::Cairo1HintProcessor).
pub fn program_from_casm_contract_class(
    value: &CasmContractClass,
) -> Result<Program, ProgramError> {
    let data = value
        .bytecode
        .iter()
        .map(|x| MaybeRelocatable::from(Felt252::from(&x.value)))
        .collect();
    //Hint data is going to be hosted processor-side, hints field will only store the pc where hints are located.
    // Only one pc will be stored, so the hint processor will be responsible for executing all hints for a given pc
    let hints = value
        .hints
        .iter()
        .map(|(x, _)| {
            (
                *x,
                vec![HintParams {
                    code: x.to_string(),
                    accessible_scopes: Vec::new(),
                    flow_tracking_data: FlowTrackingData {
                        ap_tracking: ApTracking::default(),
                        reference_ids: HashMap::new(),
                    },
                }],
            )
        })
        .collect();
    let error_message_attributes = Vec::new();
    let reference_manager = ReferenceManager {
        references: Vec::new(),
    };
    Program::new(
        vec![],
        data,
        None,
        hints,
        reference_manager,
        HashMap::new(),
        error_message_attributes,
        None,
    )
}
