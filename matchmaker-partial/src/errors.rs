use std::fmt;
use thiserror::Error;

#[derive(Debug, PartialEq, Error)]
pub enum SimpleError {
    #[error("expected a single value")]
    ExpectedSingle,

    #[error("invalid type: expected {expected}, got `{found}`")]
    InvalidType {
        expected: &'static str,
        found: String,
    },

    #[error("{0}")]
    Custom(String),

    #[error("Parse failure: {0}")]
    ParseFailure(String),
}

impl serde::de::Error for SimpleError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        SimpleError::Custom(msg.to_string())
    }
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum PartialSetError {
    #[error("Unknown field: {0}")]
    Missing(String),
    #[error("Expected more paths after: {0}")]
    EarlyEnd(String),
    #[error("Unexpected paths after a concrete field: {0:?}")]
    ExtraPaths(Vec<String>),
    #[error(transparent)]
    Deserialization(#[from] SimpleError),
}
