use crate::types::layout_name::LayoutName;

use super::instance_definitions::{
    builtins_instance_def::BuiltinsInstanceDef, diluted_pool_instance_def::DilutedPoolInstanceDef,
};

pub(crate) const MEMORY_UNITS_PER_STEP: u32 = 8;

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Serialize, Debug)]
pub struct CairoLayout {
    pub(crate) name: LayoutName,
    pub(crate) rc_units: u32,
    pub(crate) builtins: BuiltinsInstanceDef,
    pub(crate) public_memory_fraction: u32,
    pub(crate) diluted_pool_instance_def: Option<DilutedPoolInstanceDef>,
}

impl CairoLayout {
    pub(crate) fn plain_instance() -> CairoLayout {
        CairoLayout {
            name: LayoutName::plain,
            rc_units: 16,
            builtins: BuiltinsInstanceDef::plain(),
            public_memory_fraction: 4,
            diluted_pool_instance_def: None,
        }
    }

    pub(crate) fn small_instance() -> CairoLayout {
        CairoLayout {
            name: LayoutName::small,
            rc_units: 16,
            builtins: BuiltinsInstanceDef::small(),
            public_memory_fraction: 4,
            diluted_pool_instance_def: None,
        }
    }

    pub(crate) fn dex_instance() -> CairoLayout {
        CairoLayout {
            name: LayoutName::dex,
            rc_units: 4,
            builtins: BuiltinsInstanceDef::dex(),
            public_memory_fraction: 4,
            diluted_pool_instance_def: None,
        }
    }

    pub(crate) fn recursive_instance() -> CairoLayout {
        CairoLayout {
            name: LayoutName::recursive,
            rc_units: 4,
            builtins: BuiltinsInstanceDef::recursive(),
            public_memory_fraction: 8,
            diluted_pool_instance_def: Some(DilutedPoolInstanceDef::default()),
        }
    }

    pub(crate) fn starknet_instance() -> CairoLayout {
        CairoLayout {
            name: LayoutName::starknet,
            rc_units: 4,
            builtins: BuiltinsInstanceDef::starknet(),
            public_memory_fraction: 8,
            diluted_pool_instance_def: Some(DilutedPoolInstanceDef::new(2, 4, 16)),
        }
    }

    pub(crate) fn starknet_with_keccak_instance() -> CairoLayout {
        CairoLayout {
            name: LayoutName::starknet_with_keccak,
            rc_units: 4,
            builtins: BuiltinsInstanceDef::starknet_with_keccak(),
            public_memory_fraction: 8,
            diluted_pool_instance_def: Some(DilutedPoolInstanceDef::default()),
        }
    }

    pub(crate) fn recursive_large_output_instance() -> CairoLayout {
        CairoLayout {
            name: LayoutName::recursive_large_output,
            rc_units: 4,
            builtins: BuiltinsInstanceDef::recursive_large_output(),
            public_memory_fraction: 8,
            diluted_pool_instance_def: Some(DilutedPoolInstanceDef::default()),
        }
    }
    pub(crate) fn recursive_with_poseidon() -> CairoLayout {
        CairoLayout {
            name: LayoutName::recursive_with_poseidon,
            rc_units: 4,
            builtins: BuiltinsInstanceDef::recursive_with_poseidon(),
            public_memory_fraction: 8,
            diluted_pool_instance_def: Some(DilutedPoolInstanceDef::new(8, 4, 16)),
        }
    }

    pub(crate) fn all_cairo_instance() -> CairoLayout {
        CairoLayout {
            name: LayoutName::all_cairo,
            rc_units: 4,
            builtins: BuiltinsInstanceDef::all_cairo(),
            public_memory_fraction: 8,
            diluted_pool_instance_def: Some(DilutedPoolInstanceDef::default()),
        }
    }

    pub(crate) fn all_solidity_instance() -> CairoLayout {
        CairoLayout {
            name: LayoutName::all_solidity,
            rc_units: 8,
            builtins: BuiltinsInstanceDef::all_solidity(),
            public_memory_fraction: 8,
            diluted_pool_instance_def: Some(DilutedPoolInstanceDef::default()),
        }
    }

    pub(crate) fn dynamic_instance(params: CairoLayoutParams) -> CairoLayout {
        CairoLayout {
            name: LayoutName::dynamic,
            rc_units: params.rc_units,
            public_memory_fraction: 8,
            diluted_pool_instance_def: Some(DilutedPoolInstanceDef {
                units_per_step: 2_u32.pow(params.log_diluted_units_per_step),
                ..DilutedPoolInstanceDef::default()
            }),
            builtins: BuiltinsInstanceDef::dynamic(params),
        }
    }
}

#[cfg(feature = "test_utils")]
use arbitrary::{self, Arbitrary};

#[cfg_attr(feature = "test_utils", derive(Arbitrary))]
#[derive(Deserialize, Debug, Default, Clone)]
pub struct CairoLayoutParams {
    pub rc_units: u32,
    pub log_diluted_units_per_step: u32,
    #[serde(deserialize_with = "bool_from_int_or_bool")]
    pub uses_pedersen_builtin: bool,
    pub pedersen_ratio: u32,
    #[serde(deserialize_with = "bool_from_int_or_bool")]
    pub uses_range_check_builtin: bool,
    pub range_check_ratio: u32,
    #[serde(deserialize_with = "bool_from_int_or_bool")]
    pub uses_ecdsa_builtin: bool,
    pub ecdsa_ratio: u32,
    #[serde(deserialize_with = "bool_from_int_or_bool")]
    pub uses_bitwise_builtin: bool,
    pub bitwise_ratio: u32,
    #[serde(deserialize_with = "bool_from_int_or_bool")]
    pub uses_ec_op_builtin: bool,
    pub ec_op_ratio: u32,
    #[serde(deserialize_with = "bool_from_int_or_bool")]
    pub uses_keccak_builtin: bool,
    pub keccak_ratio: u32,
    #[serde(deserialize_with = "bool_from_int_or_bool")]
    pub uses_poseidon_builtin: bool,
    pub poseidon_ratio: u32,
    #[serde(deserialize_with = "bool_from_int_or_bool")]
    pub uses_range_check96_builtin: bool,
    pub range_check96_ratio: u32,
    #[serde(deserialize_with = "bool_from_int_or_bool")]
    pub uses_add_mod_builtin: bool,
    pub add_mod_ratio: u32,
    #[serde(deserialize_with = "bool_from_int_or_bool")]
    pub uses_mul_mod_builtin: bool,
    pub mul_mod_ratio: u32,
    // the following are not used right now
    pub cpu_component_step: u32,
    pub memory_units_per_step: u32,
    pub range_check96_ratio_den: u32,
    pub mul_mod_ratio_den: u32,
    pub add_mod_ratio_den: u32,
}

impl CairoLayoutParams {
    #[cfg(feature = "std")]
    pub fn from_file(params_path: &std::path::Path) -> std::io::Result<Self> {
        let params_file = std::fs::File::open(params_path)?;
        let params = serde_json::from_reader(params_file)?;
        Ok(params)
    }
}

fn bool_from_int_or_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum IntOrBool {
        Int(i64),
        Boolean(bool),
    }

    match IntOrBool::deserialize(deserializer)? {
        IntOrBool::Int(0) => Ok(false),
        IntOrBool::Int(_) => Ok(true),
        IntOrBool::Boolean(v) => Ok(v),
    }
}

#[cfg(test)]
mod tests {
    use crate::types::instance_definitions::{
        bitwise_instance_def::BitwiseInstanceDef, ec_op_instance_def::EcOpInstanceDef,
        ecdsa_instance_def::EcdsaInstanceDef, keccak_instance_def::KeccakInstanceDef,
        mod_instance_def::ModInstanceDef, pedersen_instance_def::PedersenInstanceDef,
        range_check_instance_def::RangeCheckInstanceDef,
    };

    use super::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::*;

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn get_plain_instance() {
        let layout = CairoLayout::plain_instance();
        let builtins = BuiltinsInstanceDef::plain();
        assert_eq!(layout.name, LayoutName::plain);
        assert_eq!(layout.rc_units, 16);
        assert_eq!(layout.builtins, builtins);
        assert_eq!(layout.public_memory_fraction, 4);
        assert_eq!(layout.diluted_pool_instance_def, None);
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn get_small_instance() {
        let layout = CairoLayout::small_instance();
        let builtins = BuiltinsInstanceDef::small();
        assert_eq!(layout.name, LayoutName::small);
        assert_eq!(layout.rc_units, 16);
        assert_eq!(layout.builtins, builtins);
        assert_eq!(layout.public_memory_fraction, 4);
        assert_eq!(layout.diluted_pool_instance_def, None);
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn get_dex_instance() {
        let layout = CairoLayout::dex_instance();
        let builtins = BuiltinsInstanceDef::dex();
        assert_eq!(layout.name, LayoutName::dex);
        assert_eq!(layout.rc_units, 4);
        assert_eq!(layout.builtins, builtins);
        assert_eq!(layout.public_memory_fraction, 4);
        assert_eq!(layout.diluted_pool_instance_def, None);
    }

    #[test]
    fn get_recursive_instance() {
        let layout = CairoLayout::recursive_instance();
        let builtins = BuiltinsInstanceDef::recursive();
        assert_eq!(layout.name, LayoutName::recursive);
        assert_eq!(layout.rc_units, 4);
        assert_eq!(layout.builtins, builtins);
        assert_eq!(layout.public_memory_fraction, 8);
        assert_eq!(
            layout.diluted_pool_instance_def,
            Some(DilutedPoolInstanceDef::default())
        );
    }

    #[test]
    fn get_starknet_instance() {
        let layout = CairoLayout::starknet_instance();
        let builtins = BuiltinsInstanceDef::starknet();
        assert_eq!(layout.name, LayoutName::starknet);
        assert_eq!(layout.rc_units, 4);
        assert_eq!(layout.builtins, builtins);
        assert_eq!(layout.public_memory_fraction, 8);
        assert_eq!(
            layout.diluted_pool_instance_def,
            Some(DilutedPoolInstanceDef::new(2, 4, 16))
        );
    }

    #[test]
    fn get_starknet_with_keccak_instance() {
        let layout = CairoLayout::starknet_with_keccak_instance();
        let builtins = BuiltinsInstanceDef::starknet_with_keccak();
        assert_eq!(layout.name, LayoutName::starknet_with_keccak);
        assert_eq!(layout.rc_units, 4);
        assert_eq!(layout.builtins, builtins);
        assert_eq!(layout.public_memory_fraction, 8);
        assert_eq!(
            layout.diluted_pool_instance_def,
            Some(DilutedPoolInstanceDef::default())
        );
    }

    #[test]
    fn get_recursive_large_output_instance() {
        let layout = CairoLayout::recursive_large_output_instance();
        let builtins = BuiltinsInstanceDef::recursive_large_output();
        assert_eq!(layout.name, LayoutName::recursive_large_output);
        assert_eq!(layout.rc_units, 4);
        assert_eq!(layout.builtins, builtins);
        assert_eq!(layout.public_memory_fraction, 8);
        assert_eq!(
            layout.diluted_pool_instance_def,
            Some(DilutedPoolInstanceDef::default())
        );
    }

    #[test]
    fn get_all_cairo_instance() {
        let layout = CairoLayout::all_cairo_instance();
        let builtins = BuiltinsInstanceDef::all_cairo();
        assert_eq!(layout.name, LayoutName::all_cairo);
        assert_eq!(layout.rc_units, 4);
        assert_eq!(layout.builtins, builtins);
        assert_eq!(layout.public_memory_fraction, 8);
        assert_eq!(
            layout.diluted_pool_instance_def,
            Some(DilutedPoolInstanceDef::default())
        );
    }

    #[test]
    fn get_all_solidity_instance() {
        let layout = CairoLayout::all_solidity_instance();
        let builtins = BuiltinsInstanceDef::all_solidity();
        assert_eq!(layout.name, LayoutName::all_solidity);
        assert_eq!(layout.rc_units, 8);
        assert_eq!(layout.builtins, builtins);
        assert_eq!(layout.public_memory_fraction, 8);
        assert_eq!(
            layout.diluted_pool_instance_def,
            Some(DilutedPoolInstanceDef::default())
        );
    }

    #[test]
    fn get_dynamic_instance() {
        // dummy cairo layout params
        let params = CairoLayoutParams {
            rc_units: 32,
            log_diluted_units_per_step: 5,
            uses_pedersen_builtin: true,
            pedersen_ratio: 32,
            uses_range_check_builtin: true,
            range_check_ratio: 32,
            uses_ecdsa_builtin: true,
            ecdsa_ratio: 32,
            uses_bitwise_builtin: true,
            bitwise_ratio: 32,
            uses_ec_op_builtin: true,
            ec_op_ratio: 32,
            uses_keccak_builtin: true,
            keccak_ratio: 32,
            uses_poseidon_builtin: false,
            uses_range_check96_builtin: false,
            uses_add_mod_builtin: false,
            uses_mul_mod_builtin: true,
            mul_mod_ratio: 32,
            ..Default::default() //
                                 // cpu_component_step: todo!(),
                                 // memory_units_per_step: todo!(),
                                 // range_check96_ratio_den: todo!(),
                                 // add_mod_ratio_den: todo!(),
                                 // mul_mod_ratio_den: todo!(),
        };

        let layout = CairoLayout::dynamic_instance(params);

        assert_eq!(layout.name, LayoutName::dynamic);
        assert_eq!(layout.rc_units, 32);
        assert_eq!(layout.public_memory_fraction, 4); // hardcoded
        assert_eq!(
            layout.diluted_pool_instance_def,
            Some(DilutedPoolInstanceDef {
                units_per_step: 32,
                ..DilutedPoolInstanceDef::default() // hardcoded
            })
        );

        assert!(layout.builtins.output);
        assert_eq!(
            layout.builtins.pedersen,
            Some(PedersenInstanceDef { ratio: Some(32) })
        );
        assert_eq!(
            layout.builtins.range_check,
            Some(RangeCheckInstanceDef { ratio: Some(32) })
        );
        assert_eq!(
            layout.builtins.ecdsa,
            Some(EcdsaInstanceDef { ratio: Some(32) })
        );
        assert_eq!(
            layout.builtins.bitwise,
            Some(BitwiseInstanceDef { ratio: Some(32) })
        );
        assert_eq!(
            layout.builtins.ec_op,
            Some(EcOpInstanceDef { ratio: Some(32) })
        );
        assert_eq!(
            layout.builtins.keccak,
            Some(KeccakInstanceDef { ratio: Some(32) })
        );
        assert_eq!(layout.builtins.poseidon, None,);
        assert_eq!(layout.builtins.range_check96, None,);
        assert_eq!(layout.builtins.add_mod, None);
        assert_eq!(
            layout.builtins.mul_mod,
            Some(ModInstanceDef {
                ratio: Some(32),
                word_bit_len: 1, // hardcoded
                batch_size: 96   // hardcoded
            })
        );
    }
}
