use crate::{
    felt_str,
    types::{builtin_name::BuiltinName, layout_name::LayoutName},
};

use std::collections::BTreeMap;

use crate::{
    cairo_run::{cairo_run, Cairo0RunConfig},
    hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor,
    types::relocatable::Relocatable,
    vm::runners::{
        cairo_pie::{
            BuiltinAdditionalData, CairoPieMemory, OutputBuiltinAdditionalData, SegmentInfo,
        },
        cairo_runner::ExecutionResources,
    },
};

#[test]
fn pedersen_test() {
    // Run the program
    let program_content = include_bytes!("../../../cairo_programs/pedersen_test.json");
    let mut hint_processor = BuiltinHintProcessor::new_empty();
    let result = cairo_run(
        program_content,
        &Cairo0RunConfig {
            layout: LayoutName::all_cairo,
            ..Default::default()
        },
        &mut hint_processor,
    );
    assert!(result.is_ok());
    let runner = result.unwrap();
    // Obtain the pie
    let result = runner.get_cairo_pie();
    assert!(result.is_ok());
    let cairo_pie = result.unwrap();
    // Check pie values
    // CairoPieMedatada
    let pie_metadata = cairo_pie.metadata;
    // ret_pc_segment
    assert_eq!(pie_metadata.ret_pc_segment, SegmentInfo::from((6, 0)));
    // builtin_segments
    let expected_builtin_segments = BTreeMap::from([
        (BuiltinName::output, SegmentInfo::from((2, 1))),
        (BuiltinName::pedersen, SegmentInfo::from((3, 3))),
        (BuiltinName::range_check, SegmentInfo::from((4, 0))),
    ]);
    assert_eq!(pie_metadata.builtin_segments, expected_builtin_segments);
    // program_segment
    assert_eq!(pie_metadata.program_segment, SegmentInfo::from((0, 19)));
    // ret_fp_segment
    assert_eq!(pie_metadata.ret_fp_segment, SegmentInfo::from((5, 0)));
    //program
    assert_eq!(pie_metadata.program.main, 6);
    assert_eq!(pie_metadata.program.builtins, runner.program.builtins);
    assert_eq!(
        pie_metadata.program.data,
        runner.program.shared_program_data.data
    );
    // execution_segment
    assert_eq!(pie_metadata.execution_segment, SegmentInfo::from((1, 15)));
    // extra_segments
    assert!(pie_metadata.extra_segments.is_empty());

    // execution_resources
    let expected_execution_resources = ExecutionResources {
        n_steps: 14,
        n_memory_holes: 0,
        builtin_instance_counter: BTreeMap::from([
            (BuiltinName::range_check, 0),
            (BuiltinName::output, 1),
            (BuiltinName::pedersen, 1),
        ]),
    };
    assert_eq!(cairo_pie.execution_resources, expected_execution_resources);
    // additional_data
    let expected_additional_data = BTreeMap::from([
        (
            BuiltinName::output,
            BuiltinAdditionalData::Output(OutputBuiltinAdditionalData {
                pages: BTreeMap::new(),
                attributes: BTreeMap::new(),
            }),
        ),
        (
            BuiltinName::pedersen,
            BuiltinAdditionalData::Hash(vec![Relocatable::from((3, 2))]),
        ),
        (BuiltinName::range_check, BuiltinAdditionalData::None),
    ]);
    assert_eq!(cairo_pie.additional_data.0, expected_additional_data);
    // memory
    assert_eq!(
        cairo_pie.memory,
        Into::<CairoPieMemory>::into(&runner.vm.segments.memory)
    );
}

#[test]
fn common_signature() {
    // Run the program
    let program_content = include_bytes!("../../../cairo_programs/common_signature.json");
    let mut hint_processor = BuiltinHintProcessor::new_empty();
    let result = cairo_run(
        program_content,
        &Cairo0RunConfig {
            layout: LayoutName::all_cairo,
            ..Default::default()
        },
        &mut hint_processor,
    );
    assert!(result.is_ok());
    let runner = result.unwrap();
    // Obtain the pie
    let result = runner.get_cairo_pie();
    assert!(result.is_ok());
    let cairo_pie = result.unwrap();
    // Check pie values
    // CairoPieMedatada
    let pie_metadata = cairo_pie.metadata;
    // ret_pc_segment
    assert_eq!(pie_metadata.ret_pc_segment, SegmentInfo::from((4, 0)));
    // builtin_segments
    let expected_builtin_segments =
        BTreeMap::from([(BuiltinName::ecdsa, SegmentInfo::from((2, 2)))]);
    assert_eq!(pie_metadata.builtin_segments, expected_builtin_segments);
    // program_segment
    assert_eq!(pie_metadata.program_segment, SegmentInfo::from((0, 21)));
    // ret_fp_segment
    assert_eq!(pie_metadata.ret_fp_segment, SegmentInfo::from((3, 0)));
    //program
    assert_eq!(pie_metadata.program.main, 9);
    assert_eq!(pie_metadata.program.builtins, runner.program.builtins);
    assert_eq!(
        pie_metadata.program.data,
        runner.program.shared_program_data.data
    );
    // execution_segment
    assert_eq!(pie_metadata.execution_segment, SegmentInfo::from((1, 11)));
    // extra_segments
    assert!(pie_metadata.extra_segments.is_empty());

    // execution_resources
    let expected_execution_resources = ExecutionResources {
        n_steps: 11,
        n_memory_holes: 0,
        builtin_instance_counter: BTreeMap::from([(BuiltinName::ecdsa, 1)]),
    };
    assert_eq!(cairo_pie.execution_resources, expected_execution_resources);
    // additional_data
    let expected_additional_data = BTreeMap::from([(
        BuiltinName::ecdsa,
        BuiltinAdditionalData::Signature(BTreeMap::from([(
            Relocatable::from((2, 0)),
            (
                felt_str!(
                    "3086480810278599376317923499561306189851900463386393948998357832163236918254"
                ),
                felt_str!(
                    "598673427589502599949712887611119751108407514580626464031881322743364689811"
                ),
            ),
        )])),
    )]);
    assert_eq!(cairo_pie.additional_data.0, expected_additional_data);
    // memory
    assert_eq!(
        cairo_pie.memory,
        Into::<CairoPieMemory>::into(&runner.vm.segments.memory)
    );
}

#[test]
fn relocate_segments() {
    // Run the program
    let program_content = include_bytes!("../../../cairo_programs/relocate_segments.json");
    let mut hint_processor = BuiltinHintProcessor::new_empty();
    let result = cairo_run(
        program_content,
        &Cairo0RunConfig {
            layout: LayoutName::all_cairo,
            ..Default::default()
        },
        &mut hint_processor,
    );
    assert!(result.is_ok());
    let runner = result.unwrap();
    // Obtain the pie
    let result = runner.get_cairo_pie();
    assert!(result.is_ok());
    let cairo_pie = result.unwrap();
    // Check pie values
    // CairoPieMedatada
    let pie_metadata = cairo_pie.metadata;
    // ret_pc_segment
    assert_eq!(pie_metadata.ret_pc_segment, SegmentInfo::from((3, 0)));
    // builtin_segments
    assert!(pie_metadata.builtin_segments.is_empty());
    // program_segment
    assert_eq!(pie_metadata.program_segment, SegmentInfo::from((0, 32)));
    // ret_fp_segment
    assert_eq!(pie_metadata.ret_fp_segment, SegmentInfo::from((2, 0)));
    //program
    assert_eq!(pie_metadata.program.main, 5);
    assert!(pie_metadata.program.builtins.is_empty());
    assert_eq!(
        pie_metadata.program.data,
        runner.program.shared_program_data.data
    );
    // execution_segment
    assert_eq!(pie_metadata.execution_segment, SegmentInfo::from((1, 16)));
    // extra_segments
    assert_eq!(pie_metadata.extra_segments, vec![SegmentInfo::from((4, 3))]);

    // execution_resources
    let expected_execution_resources = ExecutionResources {
        n_steps: 22,
        n_memory_holes: 0,
        builtin_instance_counter: BTreeMap::default(),
    };
    assert_eq!(cairo_pie.execution_resources, expected_execution_resources);
    // additional_data
    assert!(cairo_pie.additional_data.0.is_empty());
    // memory
    assert_eq!(
        cairo_pie.memory,
        Into::<CairoPieMemory>::into(&runner.vm.segments.memory)
    );
}

#[test]
fn serialize_cairo_pie() {
    // Run the program
    let program_content = include_bytes!("../../../cairo_programs/print.json");
    let mut hint_processor = BuiltinHintProcessor::new_empty();
    let result = cairo_run(
        program_content,
        &Cairo0RunConfig {
            layout: LayoutName::small,
            ..Default::default()
        },
        &mut hint_processor,
    );
    assert!(result.is_ok());
    let runner = result.unwrap();
    // Obtain the pie
    let result = runner.get_cairo_pie();
    assert!(result.is_ok());
    let cairo_pie = result.unwrap();

    assert_eq!(
        serde_json::to_value(cairo_pie).unwrap(),
        serde_json::from_str::<serde_json::Value>(include_str!("cairo_pie_test_output.json"))
            .unwrap(),
    );
}

#[test]
fn run_pie_validity_checks_integration() {
    // Run the program
    let program_content = include_bytes!("../../../cairo_programs/integration.json");
    let mut hint_processor = BuiltinHintProcessor::new_empty();
    let runner = cairo_run(
        program_content,
        &Cairo0RunConfig {
            layout: LayoutName::all_cairo,
            ..Default::default()
        },
        &mut hint_processor,
    )
    .expect("cairo_run failure");
    // Obtain the pie
    let cairo_pie = runner.get_cairo_pie().expect("Failed to get pie");
    assert!(cairo_pie.run_validity_checks().is_ok())
}
