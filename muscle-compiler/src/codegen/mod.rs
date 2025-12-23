// muscle-compiler/src/codegen/mod.rs
// Eä Code Generation v5.0 — Platform Abstraction

pub mod aarch64;
pub mod nucleus;
pub mod x86_64;

use crate::{error::CompileError, parser::Weights};

/// Trait for platform-specific code generation
pub trait CodeGenerator {
    /// Generate executable machine code for the given weights
    fn emit(weights: &Weights) -> Result<Vec<u8>, CompileError>;

    /// Get the target architecture name
    fn target_name() -> &'static str;
}

/// Dispatch to appropriate code generator
pub fn emit(weights: &Weights, target_arch: &str) -> Result<Vec<u8>, CompileError> {
    match target_arch {
        "aarch64" => aarch64::emit(weights),
        "x86_64" => x86_64::emit(weights),
        _ => Err(CompileError::CodegenError(format!(
            "Unsupported architecture: {}",
            target_arch
        ))),
    }
}
