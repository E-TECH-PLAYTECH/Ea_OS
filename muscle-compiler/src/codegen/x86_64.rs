// muscle-compiler/src/codegen/x86_64.rs
// Eä x86_64 Code Generation v5.0 — SSE/AVX optimized

use crate::error::CompileError;
use crate::parser::Weights;

/// Generate x86_64 machine code for neural network inference
pub fn emit(weights: &Weights) -> Result<Vec<u8>, CompileError> {
    let mut code = Vec::with_capacity(1024);

    // Function prologue
    // push rbp
    code.push(0x55);
    // mov rbp, rsp
    code.extend_from_slice(&[0x48, 0x89, 0xE5]);

    // Load input vector (rdi points to 4 f32 inputs)
    // movups xmm0, [rdi]
    code.extend_from_slice(&[0x0F, 0x10, 0x07]);

    // Generate computation (placeholder)
    emit_computation(&mut code, weights);

    // Store result back to rdi (single f32)
    // movss [rdi], xmm0
    code.extend_from_slice(&[0xF3, 0x0F, 0x11, 0x07]);

    // Function epilogue
    // pop rbp
    code.push(0x5D);
    // ret
    code.push(0xC3);

    Ok(code)
}

/// Emit computation logic (placeholder)
fn emit_computation(code: &mut Vec<u8>, _weights: &Weights) {
    // Placeholder for actual SSE/AVX computation
    // This would include:
    // - Matrix multiplication with W1
    // - Bias addition with b1
    // - ReLU activation
    // - Vector multiplication with W2
    // - Final bias addition with b2

    code.extend_from_slice(&[0x90; 64]); // NOP padding for now
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_x86_code_generation() {
        let weights = Weights {
            w1: [
                [0.1, 0.2, 0.3],
                [0.4, 0.5, 0.6],
                [0.7, 0.8, 0.9],
                [1.0, 1.1, 1.2],
            ],
            b1: [0.1, 0.2, 0.3],
            w2: [0.4, 0.5, 0.6],
            b2: 0.7,
        };

        let code = emit(&weights);

        assert!(code.len() >= 16, "Generated code too small");
        assert!(code.len() <= 1024, "Generated code too large");

        // Check prologue
        assert_eq!(code[0], 0x55); // push rbp

        // Check epilogue
        assert_eq!(code[code.len() - 2], 0x5D); // pop rbp
        assert_eq!(code[code.len() - 1], 0xC3); // ret
    }
}
