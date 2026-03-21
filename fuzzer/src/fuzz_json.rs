use arbitrary::{self, Arbitrary, Unstructured};
use cairo_vm::{
    cairo_run::{cairo_run, Cairo0RunConfig},
    hint_processor::builtin_hint_processor::{
        builtin_hint_processor_definition::BuiltinHintProcessor, hint_code::*,
    },
    serde::deserialize_program::{
        Attribute, DebugInfo, FlowTrackingData, Member, ReferenceManager,
    },
    Felt252,
};
use honggfuzz::fuzz;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;

const BUILTIN_NAMES: [&str; 9] = [
    "output",
    "range_check",
    "pedersen",
    "ecdsa",
    "keccak",
    "bitwise",
    "ec_op",
    "poseidon",
    "segment_arena",
];

const HEX_SYMBOLS: [&str; 16] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f",
];

const HINTS_CODE: [&str; 199] = [
    ADD_SEGMENT,
    VM_ENTER_SCOPE,
    VM_EXIT_SCOPE,
    MEMCPY_ENTER_SCOPE,
    MEMCPY_CONTINUE_COPYING,
    MEMSET_ENTER_SCOPE,
    MEMSET_CONTINUE_LOOP,
    POW,
    IS_NN,
    IS_NN_OUT_OF_RANGE,
    IS_LE_FELT,
    IS_POSITIVE,
    ASSERT_NN,
    ASSERT_NOT_ZERO,
    ASSERT_NOT_ZERO,
    ASSERT_NOT_EQUAL,
    ASSERT_LE_FELT,
    ASSERT_LE_FELT_V_0_6,
    ASSERT_LE_FELT_V_0_8,
    ASSERT_LE_FELT_EXCLUDED_0,
    ASSERT_LE_FELT_EXCLUDED_1,
    ASSERT_LE_FELT_EXCLUDED_2,
    ASSERT_LT_FELT,
    SPLIT_INT_ASSERT_RANGE,
    ASSERT_250_BITS,
    IS_250_BITS,
    IS_ADDR_BOUNDED,
    SPLIT_INT,
    SPLIT_64,
    SPLIT_FELT,
    SQRT,
    UNSIGNED_DIV_REM,
    SIGNED_DIV_REM,
    IS_QUAD_RESIDUE,
    FIND_ELEMENT,
    SEARCH_SORTED_LOWER,
    SET_ADD,
    DEFAULT_DICT_NEW,
    DICT_NEW,
    DICT_READ,
    DICT_WRITE,
    DICT_UPDATE,
    SQUASH_DICT,
    SQUASH_DICT_INNER_SKIP_LOOP,
    SQUASH_DICT_INNER_FIRST_ITERATION,
    SQUASH_DICT_INNER_CHECK_ACCESS_INDEX,
    SQUASH_DICT_INNER_CONTINUE_LOOP,
    SQUASH_DICT_INNER_ASSERT_LEN_KEYS,
    SQUASH_DICT_INNER_LEN_ASSERT,
    SQUASH_DICT_INNER_USED_ACCESSES_ASSERT,
    SQUASH_DICT_INNER_NEXT_KEY,
    DICT_SQUASH_COPY_DICT,
    DICT_SQUASH_UPDATE_PTR,
    BIGINT_TO_UINT256,
    UINT256_ADD,
    UINT256_ADD_LOW,
    UINT128_ADD,
    UINT256_SUB,
    UINT256_SQRT,
    UINT256_SQRT_FELT,
    UINT256_SIGNED_NN,
    UINT256_UNSIGNED_DIV_REM,
    UINT256_EXPANDED_UNSIGNED_DIV_REM,
    UINT256_MUL_DIV_MOD,
    USORT_ENTER_SCOPE,
    USORT_BODY,
    USORT_VERIFY,
    USORT_VERIFY_MULTIPLICITY_ASSERT,
    USORT_VERIFY_MULTIPLICITY_BODY,
    BLAKE2S_COMPUTE,
    BLAKE2S_FINALIZE,
    BLAKE2S_FINALIZE_V2,
    BLAKE2S_FINALIZE_V3,
    BLAKE2S_ADD_UINT256,
    BLAKE2S_ADD_UINT256_BIGEND,
    EXAMPLE_BLAKE2S_COMPRESS,
    NONDET_BIGINT3_V1,
    NONDET_BIGINT3_V2,
    VERIFY_ZERO_V1,
    VERIFY_ZERO_V2,
    VERIFY_ZERO_V3,
    VERIFY_ZERO_EXTERNAL_SECP,
    REDUCE_V1,
    REDUCE_ED25519,
    UNSAFE_KECCAK,
    UNSAFE_KECCAK_FINALIZE,
    IS_ZERO_NONDET,
    IS_ZERO_INT,
    IS_ZERO_PACK_V1,
    IS_ZERO_PACK_V2,
    IS_ZERO_PACK_EXTERNAL_SECP_V1,
    IS_ZERO_PACK_EXTERNAL_SECP_V2,
    IS_ZERO_PACK_ED25519,
    IS_ZERO_ASSIGN_SCOPE_VARS,
    IS_ZERO_ASSIGN_SCOPE_VARS_EXTERNAL_SECP,
    IS_ZERO_ASSIGN_SCOPE_VARS_ED25519,
    DIV_MOD_N_PACKED_DIVMOD_V1,
    DIV_MOD_N_PACKED_DIVMOD_EXTERNAL_N,
    DIV_MOD_N_SAFE_DIV,
    GET_FELT_BIT_LENGTH,
    BIGINT_PACK_DIV_MOD,
    BIGINT_SAFE_DIV,
    DIV_MOD_N_SAFE_DIV_PLUS_ONE,
    GET_POINT_FROM_X,
    EC_NEGATE,
    EC_NEGATE_EMBEDDED_SECP,
    EC_DOUBLE_SLOPE_V1,
    EC_DOUBLE_SLOPE_V2,
    EC_DOUBLE_SLOPE_V3,
    EC_DOUBLE_SLOPE_V4,
    EC_DOUBLE_SLOPE_EXTERNAL_CONSTS,
    COMPUTE_SLOPE_V1,
    COMPUTE_SLOPE_V2,
    COMPUTE_SLOPE_SECP256R1_V1,
    COMPUTE_SLOPE_SECP256R1_V2,
    IMPORT_SECP256R1_P,
    COMPUTE_SLOPE_WHITELIST,
    EC_DOUBLE_ASSIGN_NEW_X_V1,
    EC_DOUBLE_ASSIGN_NEW_X_V2,
    EC_DOUBLE_ASSIGN_NEW_X_V3,
    EC_DOUBLE_ASSIGN_NEW_X_V4,
    EC_DOUBLE_ASSIGN_NEW_Y,
    SHA256_INPUT,
    SHA256_MAIN_CONSTANT_INPUT_LENGTH,
    SHA256_MAIN_ARBITRARY_INPUT_LENGTH,
    SHA256_FINALIZE,
    KECCAK_WRITE_ARGS,
    COMPARE_BYTES_IN_WORD_NONDET,
    COMPARE_KECCAK_FULL_RATE_IN_BYTES_NONDET,
    BLOCK_PERMUTATION,
    BLOCK_PERMUTATION_WHITELIST_V1,
    BLOCK_PERMUTATION_WHITELIST_V2,
    CAIRO_KECCAK_INPUT_IS_FULL_WORD,
    CAIRO_KECCAK_FINALIZE_V1,
    CAIRO_KECCAK_FINALIZE_V2,
    FAST_EC_ADD_ASSIGN_NEW_X,
    FAST_EC_ADD_ASSIGN_NEW_X_V2,
    FAST_EC_ADD_ASSIGN_NEW_X_V3,
    FAST_EC_ADD_ASSIGN_NEW_Y,
    EC_MUL_INNER,
    RELOCATE_SEGMENT,
    TEMPORARY_ARRAY,
    VERIFY_ECDSA_SIGNATURE,
    SPLIT_OUTPUT_0,
    SPLIT_OUTPUT_1,
    SPLIT_INPUT_3,
    SPLIT_INPUT_6,
    SPLIT_INPUT_9,
    SPLIT_INPUT_12,
    SPLIT_INPUT_15,
    SPLIT_N_BYTES,
    SPLIT_OUTPUT_MID_LOW_HIGH,
    NONDET_N_GREATER_THAN_10,
    NONDET_N_GREATER_THAN_2,
    RANDOM_EC_POINT,
    CHAINED_EC_OP_RANDOM_EC_POINT,
    RECOVER_Y,
    PACK_MODN_DIV_MODN,
    XS_SAFE_DIV,
    UINT384_UNSIGNED_DIV_REM,
    UINT384_SPLIT_128,
    ADD_NO_UINT384_CHECK,
    UINT384_SQRT,
    SUB_REDUCED_A_AND_REDUCED_B,
    UNSIGNED_DIV_REM_UINT768_BY_UINT384,
    UNSIGNED_DIV_REM_UINT768_BY_UINT384_STRIPPED,
    UINT384_SIGNED_NN,
    IMPORT_SECP256R1_ALPHA,
    IMPORT_SECP256R1_N,
    UINT384_GET_SQUARE_ROOT,
    UINT256_GET_SQUARE_ROOT,
    UINT384_DIV,
    INV_MOD_P_UINT256,
    HI_MAX_BITLEN,
    QUAD_BIT,
    INV_MOD_P_UINT512,
    DI_BIT,
    EC_RECOVER_DIV_MOD_N_PACKED,
    UINT512_UNSIGNED_DIV_REM,
    EC_RECOVER_SUB_A_B,
    A_B_BITAND_1,
    EC_RECOVER_PRODUCT_MOD,
    UINT256_MUL_INV_MOD_P,
    EC_RECOVER_PRODUCT_DIV_M,
    SQUARE_SLOPE_X_MOD_P,
    SPLIT_XX,
    BLAKE2S_UNPACK_FELTS,
    EC_DOUBLE_SLOPE_V5,
    EXCESS_BALANCE,
    IS_LESS_THAN_63_BITS_AND_NOT_END,
    NONDET_ELEMENTS_OVER_TEN,
    NONDET_ELEMENTS_OVER_TWO,
    PRINT_ARR,
    PRINT_DICT,
    PRINT_FELT,
    REDUCE_V2,
    RUN_P_CIRCUIT,
    RUN_P_CIRCUIT_WITH_LARGE_BATCH_SIZE,
    SKIP_NEXT_INSTRUCTION,
];

#[derive(Arbitrary, Serialize, Deserialize)]
struct ProgramJson {
    attributes: Vec<Attribute>,
    #[arbitrary(with = arbitrary_builtins)]
    builtins: Vec<String>,
    #[arbitrary(value = "0.11.0".to_string())]
    compiler_version: String,
    data: Vec<TextFelt>,
    debug_info: DebugInfo,
    #[arbitrary(with = prepend_main_identifier)]
    identifiers: HashMap<String, TextIdentifier>,
    hints: HashMap<usize, Vec<TextHintParams>>,
    #[arbitrary(value = "__main__".to_string())]
    main_scope: String,
    #[arbitrary(value = "0x800000000000011000000000000000000000000000000000000000000000001".to_string())]
    prime: String,
    reference_manager: ReferenceManager,
}

#[derive(Deserialize)]
struct TextFelt {
    value: String,
}

#[derive(Serialize, Deserialize, Arbitrary)]
struct TextIdentifier {
    #[serde(skip_serializing_if = "Option::is_none")]
    pc: Option<usize>,
    #[serde(rename(serialize = "type"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    type_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Felt252>,
    #[serde(skip_serializing_if = "Option::is_none")]
    full_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    members: Option<HashMap<String, Member>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cairo_type: Option<String>,
}

#[derive(Serialize, Deserialize, Arbitrary)]
pub struct TextHintParams {
    #[arbitrary(with = get_hint_code)]
    pub code: String,
    #[arbitrary(with = prepend_mod_name)]
    pub accessible_scopes: Vec<String>,
    pub flow_tracking_data: FlowTrackingData,
}

impl<'a> Arbitrary<'a> for TextFelt {
    fn arbitrary(u: &mut Unstructured) -> arbitrary::Result<TextFelt> {
        let felt_size = 16;
        let mut digits = Vec::with_capacity(felt_size);
        for _ in 0..felt_size {
            digits.push(*u.choose(&HEX_SYMBOLS)?)
        }
        Ok(TextFelt {
            value: digits.join(""),
        })
    }
}

impl Serialize for TextFelt {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(&format!("0x{}", self.value))
    }
}

fn arbitrary_builtins(u: &mut Unstructured) -> arbitrary::Result<Vec<String>> {
    let builtin_total = u.choose_index(BUILTIN_NAMES.len())?;
    let mut selected_builtins = Vec::new();

    for builtin_name in BUILTIN_NAMES.iter().take(builtin_total + 1) {
        if u.ratio(2, 3)? {
            selected_builtins.push(builtin_name.to_string())
        }
    }

    Ok(selected_builtins)
}

fn prepend_main_identifier(
    _u: &mut Unstructured,
) -> arbitrary::Result<HashMap<String, TextIdentifier>> {
    let mut identifiers = HashMap::new();
    identifiers.insert(
        String::from("__main__.main"),
        TextIdentifier {
            pc: Some(0),
            type_: Some(String::from("function")),
            value: None,
            full_name: None,
            members: None,
            cairo_type: None,
        },
    );
    Ok(identifiers)
}

fn get_hint_code(u: &mut Unstructured) -> arbitrary::Result<String> {
    Ok(u.choose(&HINTS_CODE)?.to_string())
}

fn prepend_mod_name(u: &mut Unstructured) -> arbitrary::Result<Vec<String>> {
    let accessible_scopes: Vec<String> = Vec::<String>::arbitrary(u)?
        .iter()
        .map(|scope| "starkware.common.".to_string() + scope)
        .collect();
    Ok(accessible_scopes)
}

fn main() {
    loop {
        fuzz!(|data: (Cairo0RunConfig, ProgramJson)| {
            let (cairo_run_config, program_json) = data;
            if let Ok(program_raw) = serde_json::to_string_pretty(&program_json) {
                let _ = cairo_run(
                    program_raw.as_bytes(),
                    &Cairo0RunConfig::default(),
                    &mut BuiltinHintProcessor::new_empty(),
                );
                let _ = cairo_run(
                    program_raw.as_bytes(),
                    &cairo_run_config,
                    &mut BuiltinHintProcessor::new_empty(),
                );
            }
        });
    }
}
