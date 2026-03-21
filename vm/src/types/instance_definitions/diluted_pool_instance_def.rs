use serde::Serialize;
use thiserror::Error;

#[derive(Debug, PartialEq, Error)]
pub enum DilutedPoolInstanceDefError {
    #[error(
        "Dynamic layout log_diluted_units_per_step is out of range, absolute value must be < 32"
    )]
    DynamicLayoutLogDilutedUnitsPerStepOverflow(i32),
}

#[derive(Clone, Serialize, Debug, PartialEq)]
pub(crate) struct DilutedPoolInstanceDef {
    pub(crate) units_per_step: u32, // 2 ^ log_units_per_step (for cairo_lang comparison)
    pub(crate) fractional_units_per_step: bool, // true when log_units_per_step is negative
    pub(crate) spacing: u32,
    pub(crate) n_bits: u32,
}

impl DilutedPoolInstanceDef {
    pub(crate) fn default() -> Self {
        DilutedPoolInstanceDef {
            units_per_step: 16,
            fractional_units_per_step: false,
            spacing: 4,
            n_bits: 16,
        }
    }

    pub(crate) fn new(units_per_step: u32, spacing: u32, n_bits: u32) -> Self {
        DilutedPoolInstanceDef {
            units_per_step,
            spacing,
            n_bits,
            ..Self::default()
        }
    }

    pub(crate) fn from_log_units_per_step(
        log_units_per_step: i32,
    ) -> Result<Self, DilutedPoolInstanceDefError> {
        let units_per_step = 2_u32.checked_pow(log_units_per_step.unsigned_abs()).ok_or(
            DilutedPoolInstanceDefError::DynamicLayoutLogDilutedUnitsPerStepOverflow(
                log_units_per_step,
            ),
        )?;
        Ok(DilutedPoolInstanceDef {
            units_per_step,
            fractional_units_per_step: log_units_per_step.is_negative(),
            ..DilutedPoolInstanceDef::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{DilutedPoolInstanceDef, DilutedPoolInstanceDefError};

    #[test]
    fn test_default() {
        let diluted_pool = DilutedPoolInstanceDef::default();
        assert_eq!(diluted_pool.units_per_step, 16);
        assert_eq!(diluted_pool.spacing, 4);
        assert_eq!(diluted_pool.n_bits, 16);
    }

    #[test]
    fn test_new() {
        let diluted_pool = DilutedPoolInstanceDef::new(1, 1, 1);
        assert_eq!(diluted_pool.units_per_step, 1);
        assert_eq!(diluted_pool.spacing, 1);
        assert_eq!(diluted_pool.n_bits, 1);
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn test_from_log_units_per_step() {
        let diluted_pool = DilutedPoolInstanceDef::from_log_units_per_step(4).unwrap();
        assert_eq!(diluted_pool.units_per_step, 16);
        assert!(!diluted_pool.fractional_units_per_step);
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn test_from_log_units_per_step_negative() {
        let diluted_pool = DilutedPoolInstanceDef::from_log_units_per_step(-3).unwrap();
        assert_eq!(diluted_pool.units_per_step, 8);
        assert!(diluted_pool.fractional_units_per_step);
    }

    #[test]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn test_from_log_units_per_step_overflow() {
        assert_eq!(
            DilutedPoolInstanceDef::from_log_units_per_step(32),
            Err(DilutedPoolInstanceDefError::DynamicLayoutLogDilutedUnitsPerStepOverflow(32))
        );
    }
}
