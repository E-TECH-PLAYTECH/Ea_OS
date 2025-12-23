use std::fmt;

/// Shared compiler error type for the muscle compiler and its language modules.
#[derive(Debug)]
pub enum CompileError {
    IoError(String),
    SyntaxError(String),
    CapabilityError(String),
    CompileError(String),
    CodegenError(String),
    CryptoError(String),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::IoError(msg) => write!(f, "I/O error: {}", msg),
            CompileError::SyntaxError(msg) => write!(f, "Syntax error: {}", msg),
            CompileError::CapabilityError(msg) => write!(f, "Capability error: {}", msg),
            CompileError::CompileError(msg) => write!(f, "Compile error: {}", msg),
            CompileError::CodegenError(msg) => write!(f, "Codegen error: {}", msg),
            CompileError::CryptoError(msg) => write!(f, "Crypto error: {}", msg),
        }
    }
}

impl std::error::Error for CompileError {}

impl From<std::io::Error> for CompileError {
    fn from(value: std::io::Error) -> Self {
        CompileError::IoError(value.to_string())
    }
}

impl From<crate::parser::ParseError> for CompileError {
    fn from(error: crate::parser::ParseError) -> Self {
        CompileError::SyntaxError(error.to_string())
    }
}
