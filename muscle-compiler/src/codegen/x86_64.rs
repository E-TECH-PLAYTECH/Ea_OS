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

/// Emit computation logic using SSE instructions
/// Computes: output = ReLU(input * W1 + b1) * W2 + b2
fn emit_computation(code: &mut Vec<u8>, weights: &Weights) {
    // SSE approach: use XMM registers for computation
    // XMM0 = input vector (4 floats)
    // XMM1-XMM3 = hidden layer outputs
    // XMM4-XMM7 = temporary/weight registers

    // Layer 1: For each of 3 hidden neurons, compute dot product + bias
    for i in 0..3 {
        emit_dot_product_sse(
            code,
            weights.w1[0][i],
            weights.w1[1][i],
            weights.w1[2][i],
            weights.w1[3][i],
            weights.b1[i],
            i + 1, // output to XMM1, XMM2, XMM3
        );
    }

    // ReLU activation on XMM1, XMM2, XMM3
    // XORPS XMM7, XMM7 (zero register)
    code.extend_from_slice(&[0x0F, 0x57, 0xFF]);
    // MAXSS XMM1, XMM7
    code.extend_from_slice(&[0xF3, 0x0F, 0x5F, 0xCF]);
    // MAXSS XMM2, XMM7
    code.extend_from_slice(&[0xF3, 0x0F, 0x5F, 0xD7]);
    // MAXSS XMM3, XMM7
    code.extend_from_slice(&[0xF3, 0x0F, 0x5F, 0xDF]);

    // Layer 2: Compute weighted sum of hidden outputs + bias
    emit_layer2_sse(code, weights.w2[0], weights.w2[1], weights.w2[2], weights.b2);
}

/// Emit SSE dot product: result = w0*in0 + w1*in1 + w2*in2 + w3*in3 + bias
fn emit_dot_product_sse(
    code: &mut Vec<u8>,
    w0: f32,
    w1: f32,
    w2: f32,
    w3: f32,
    bias: f32,
    dest_xmm: usize,
) {
    // Store weights as immediates (will be loaded via RIP-relative)
    // For simplicity, embed weights in instruction stream and skip over them

    // JMP over weight data
    code.push(0xEB); // JMP rel8
    code.push(20);   // skip 20 bytes (5 floats)

    // Weight data
    let weights_offset = code.len();
    code.extend_from_slice(&w0.to_le_bytes());
    code.extend_from_slice(&w1.to_le_bytes());
    code.extend_from_slice(&w2.to_le_bytes());
    code.extend_from_slice(&w3.to_le_bytes());
    code.extend_from_slice(&bias.to_le_bytes());

    // Load weights into XMM4 as packed floats
    // MOVUPS XMM4, [RIP - offset_to_weights]
    let current_pos = code.len();
    let rel_offset = -((current_pos - weights_offset + 7) as i32);
    code.extend_from_slice(&[0x0F, 0x10, 0x25]); // MOVUPS XMM4, [RIP+disp32]
    code.extend_from_slice(&rel_offset.to_le_bytes());

    // Multiply XMM0 (input) by XMM4 (weights)
    // MULPS XMM4, XMM0
    code.extend_from_slice(&[0x0F, 0x59, 0xE0]);

    // Horizontal add to sum all elements
    // HADDPS XMM4, XMM4 (requires SSE3)
    code.extend_from_slice(&[0xF2, 0x0F, 0x7C, 0xE4]);
    // HADDPS XMM4, XMM4 again
    code.extend_from_slice(&[0xF2, 0x0F, 0x7C, 0xE4]);

    // Load bias and add
    let bias_offset = weights_offset + 16; // bias is at offset +16 from weights
    let bias_rel = -((code.len() - bias_offset + 8) as i32);
    // MOVSS XMM5, [RIP+disp32]
    code.extend_from_slice(&[0xF3, 0x0F, 0x10, 0x2D]);
    code.extend_from_slice(&bias_rel.to_le_bytes());
    // ADDSS XMM4, XMM5
    code.extend_from_slice(&[0xF3, 0x0F, 0x58, 0xE5]);

    // Move result to destination XMM register
    // MOVAPS XMMd, XMM4
    let dest_byte = 0xC4 | ((dest_xmm as u8) << 3);
    code.extend_from_slice(&[0x0F, 0x28, dest_byte]);
}

/// Emit Layer 2 computation: output = w0*h0 + w1*h1 + w2*h2 + bias
fn emit_layer2_sse(code: &mut Vec<u8>, w0: f32, w1: f32, w2: f32, bias: f32) {
    // JMP over weight data
    code.push(0xEB);
    code.push(16); // skip 16 bytes (4 floats)

    let weights_offset = code.len();
    code.extend_from_slice(&w0.to_le_bytes());
    code.extend_from_slice(&w1.to_le_bytes());
    code.extend_from_slice(&w2.to_le_bytes());
    code.extend_from_slice(&bias.to_le_bytes());

    // Load w0, multiply by XMM1 (h0)
    let w0_rel = -((code.len() - weights_offset + 8) as i32);
    code.extend_from_slice(&[0xF3, 0x0F, 0x10, 0x05]); // MOVSS XMM0, [RIP+disp32]
    code.extend_from_slice(&w0_rel.to_le_bytes());
    code.extend_from_slice(&[0xF3, 0x0F, 0x59, 0xC1]); // MULSS XMM0, XMM1

    // Load w1, multiply by XMM2, add to XMM0
    let w1_rel = -((code.len() - weights_offset - 4 + 8) as i32);
    code.extend_from_slice(&[0xF3, 0x0F, 0x10, 0x25]); // MOVSS XMM4, [RIP+disp32]
    code.extend_from_slice(&w1_rel.to_le_bytes());
    code.extend_from_slice(&[0xF3, 0x0F, 0x59, 0xE2]); // MULSS XMM4, XMM2
    code.extend_from_slice(&[0xF3, 0x0F, 0x58, 0xC4]); // ADDSS XMM0, XMM4

    // Load w2, multiply by XMM3, add to XMM0
    let w2_rel = -((code.len() - weights_offset - 8 + 8) as i32);
    code.extend_from_slice(&[0xF3, 0x0F, 0x10, 0x25]); // MOVSS XMM4, [RIP+disp32]
    code.extend_from_slice(&w2_rel.to_le_bytes());
    code.extend_from_slice(&[0xF3, 0x0F, 0x59, 0xE3]); // MULSS XMM4, XMM3
    code.extend_from_slice(&[0xF3, 0x0F, 0x58, 0xC4]); // ADDSS XMM0, XMM4

    // Add bias
    let bias_rel = -((code.len() - weights_offset - 12 + 8) as i32);
    code.extend_from_slice(&[0xF3, 0x0F, 0x10, 0x25]); // MOVSS XMM4, [RIP+disp32]
    code.extend_from_slice(&bias_rel.to_le_bytes());
    code.extend_from_slice(&[0xF3, 0x0F, 0x58, 0xC4]); // ADDSS XMM0, XMM4
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

        let code = emit(&weights).expect("code generation should succeed");

        assert!(code.len() >= 16, "Generated code too small");
        assert!(code.len() <= 1024, "Generated code too large");

        // Check prologue
        assert_eq!(code[0], 0x55); // push rbp

        // Check epilogue
        assert_eq!(code[code.len() - 2], 0x5D); // pop rbp
        assert_eq!(code[code.len() - 1], 0xC3); // ret
    }
}
