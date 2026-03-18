use crate::utils::PRIME_STR;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProgramError {
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Parse(#[from] serde_json::Error),
    #[error("The \"{0}\" operation is not supported")]
    OperationNotSupported(String),
    #[error("Entrypoint {0} not found")]
    EntrypointNotFound(String),
    #[error("Constant {0} has no value")]
    ConstWithoutValue(String),
    #[error("Expected prime {PRIME_STR}, got {0}")]
    PrimeDiffers(String),
    #[error("Can't build a StrippedProgram from a Program without main")]
    StrippedProgramNoMain,
    #[error("Hint PC ({0}) is greater or equal to program length ({1})")]
    InvalidHintPc(usize, usize),
    #[error("Identifier \"{0}\" is type alias but has no destination")]
    AliasMissingDestination(String),
    #[error("invalid identifier type \"{1}\" for \"{0}\": expected \"alias\" or \"function\"")]
    InvalidIdentifierTypeForPc(String, String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_entrypoint_not_found_error() {
        let error = ProgramError::EntrypointNotFound(String::from("my_function"));
        let formatted_error = format!("{error}");
        assert_eq!(formatted_error, "Entrypoint my_function not found");
    }

    #[test]
    fn format_alias_missing_destination_error() {
        let error = ProgramError::AliasMissingDestination(String::from("__main__.assert_nn"));
        let formatted_error = format!("{error}");
        assert_eq!(
            formatted_error,
            "Identifier \"__main__.assert_nn\" is type alias but has no destination"
        );
    }

    #[test]
    fn format_invalid_identifier_type_for_pc_error() {
        let error = ProgramError::InvalidIdentifierTypeForPc(
            String::from("__main__.my_struct"),
            String::from("struct"),
        );
        let formatted_error = format!("{error}");
        assert_eq!(
            formatted_error,
            "invalid identifier type \"struct\" for \"__main__.my_struct\": expected \"alias\" or \"function\""
        );
    }
}
