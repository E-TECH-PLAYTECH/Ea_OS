// muscle-compiler/src/codegen/aarch64.rs
// Eä AArch64 Code Generation v5.0 — Optimized for Cortex-A76/A78

use crate::error::CompileError;
use crate::parser::Weights;
use bytemuck::cast_slice;

/// Generate AArch64 machine code for neural network inference
pub fn emit(weights: &Weights) -> Result<Vec<u8>, CompileError> {
    let mut code = Vec::with_capacity(1024);

    // Function prologue: preserve link register and frame pointer
    // stp x29, x30, [sp, #-16]!
    code.extend_from_slice(&[0xFD, 0x7B, 0xBF, 0xA9]);

    // Set up frame pointer: mov x29, sp
    code.extend_from_slice(&[0xFD, 0x03, 0x00, 0x91]);

    // Load input vector (x0 points to 4 f32 inputs)
    // ld1 {v0.4s}, [x0]
    code.extend_from_slice(&[0x00, 0x68, 0x68, 0x4C]);

    // Generate weight loading and computation
    emit_layer1(&mut code, weights);
    emit_activation(&mut code);
    emit_layer2(&mut code, weights);

    // Store result back to x0 (single f32)
    // str s0, [x0]
    code.extend_from_slice(&[0x00, 0x00, 0x00, 0x1E]);

    // Function epilogue: restore and return
    // ldp x29, x30, [sp], #16
    code.extend_from_slice(&[0xFD, 0x7B, 0xC1, 0xA8]);
    // ret
    code.extend_from_slice(&[0xC0, 0x03, 0x5F, 0xD6]);

    // Add weights data section
    emit_weights_data(&mut code, weights);

    Ok(code)
}

/// Emit Layer 1 computation: 4x3 matrix multiplication + bias
fn emit_layer1(code: &mut Vec<u8>, weights: &Weights) {
    // We'll use immediate loads for weights for maximum performance
    // This is a simplified version - real implementation would use
    // literal pools or data section loads

    // For each of the 3 hidden neurons
    for i in 0..3 {
        // Load weights for this neuron (from W1 columns)
        let w0 = weights.w1[0][i];
        let w1 = weights.w1[1][i];
        let w2 = weights.w1[2][i];
        let w3 = weights.w1[3][i];

        // Load bias for this neuron
        let bias = weights.b1[i];

        // Emit FMADD instructions (simplified - real code would use proper encoding)
        // This is placeholder - actual instruction encoding would go here
        emit_fmadd_sequence(code, w0, w1, w2, w3, bias, i);
    }
}

/// Emit ReLU activation
fn emit_activation(code: &mut Vec<u8>) {
    // fmax v0.4s, v0.4s, v8.4s  (where v8 contains zeros)
    code.extend_from_slice(&[0x00, 0x79, 0xE8, 0x4E]);
    // fmax v1.4s, v1.4s, v8.4s
    code.extend_from_slice(&[0x21, 0x79, 0xE8, 0x4E]);
    // fmax v2.4s, v2.4s, v8.4s
    code.extend_from_slice(&[0x42, 0x79, 0xE8, 0x4E]);
}

/// Emit Layer 2 computation: 3x1 vector multiplication + bias
fn emit_layer2(code: &mut Vec<u8>, weights: &Weights) {
    // Load W2 weights and compute weighted sum
    let w0 = weights.w2[0];
    let w1 = weights.w2[1];
    let w2 = weights.w2[2];

    // Emit scalar FMADD sequence
    emit_scalar_fmadd(code, w0, w1, w2, weights.b2);
}

/// Emit FMADD sequence for Layer 1
/// Computes: result[reg_idx] = w0*input[0] + w1*input[1] + w2*input[2] + w3*input[3] + bias
///
/// Uses AArch64 SIMD: loads weights into vector register, does dot product with input v0,
/// then adds bias.
fn emit_fmadd_sequence(
    code: &mut Vec<u8>,
    w0: f32,
    w1: f32,
    w2: f32,
    w3: f32,
    bias: f32,
    reg_idx: usize,
) {
    // We'll use a literal pool approach: store weights, load them, compute
    // For now, encode the computation as scalar FMADD chain
    //
    // AArch64 scalar FMADD: FMADD Sd, Sn, Sm, Sa
    // Encoding: 0x1F000000 | (Rm << 16) | (Ra << 10) | (Rn << 5) | Rd
    //
    // Strategy:
    // 1. Load weights from literal pool into s16-s20
    // 2. Extract input scalars from v0 into s8-s11
    // 3. Compute: acc = bias; acc = fmadd(w0, in0, acc); acc = fmadd(w1, in1, acc); ...
    // 4. Store result in output register

    let dest_reg = (reg_idx + 1) as u32; // v1, v2, v3 for the 3 hidden neurons

    // DUP Sd, V0.S[0] - extract first element: 0x5E040400 | (Vn << 5) | Vd
    // Actually use: MOV Sd, Vn.S[idx] which is alias for DUP

    // For simplicity in this implementation, we'll use a data-driven approach:
    // Store the weights in the code stream and reference them via PC-relative load

    // FMOV S16, #w0 (if w0 is representable) or LDR S16, [PC, #offset]
    // For arbitrary floats, we need literal pool

    // Emit literal pool reference instructions
    // ADR X9, weights_label  (we'll patch this)
    // LDR S16, [X9, #0]   ; w0
    // LDR S17, [X9, #4]   ; w1
    // LDR S18, [X9, #8]   ; w2
    // LDR S19, [X9, #12]  ; w3
    // LDR S20, [X9, #16]  ; bias

    // For now, emit a simplified version using immediate moves where possible
    // and NOP otherwise (to be filled by literal pool loader)

    // Extract input elements from v0
    // DUP S8, V0.S[0]: 0x5E040008
    code.extend(&[0x08, 0x04, 0x04, 0x5E]);
    // DUP S9, V0.S[1]: 0x5E0C0009
    code.extend(&[0x09, 0x04, 0x0C, 0x5E]);
    // DUP S10, V0.S[2]: 0x5E140010
    code.extend(&[0x0A, 0x04, 0x14, 0x5E]);
    // DUP S11, V0.S[3]: 0x5E1C0011
    code.extend(&[0x0B, 0x04, 0x1C, 0x5E]);

    // Store weights in code for literal pool (will be loaded by data section)
    let weight_offset = code.len();
    code.extend_from_slice(cast_slice(&[w0, w1, w2, w3, bias]));

    // Load weights using PC-relative addressing
    // LDR S16, [PC, #offset] - we need to calculate offset from current PC
    // For simplicity, use ADR + LDR sequence
    // ADR X9, #-20 (back to weights): 0x10FFFFB9
    let offset_back = -20i32; // 5 floats * 4 bytes = 20
    let adr_imm = ((offset_back >> 2) & 0x7FFFF) as u32;
    let adr_inst = 0x10000009 | (adr_imm << 5);
    code.extend(&adr_inst.to_le_bytes());

    // LDR S16, [X9, #0]: 0xBD400130
    code.extend(&[0x30, 0x01, 0x40, 0xBD]);
    // LDR S17, [X9, #4]: 0xBD400531
    code.extend(&[0x31, 0x05, 0x40, 0xBD]);
    // LDR S18, [X9, #8]: 0xBD400932
    code.extend(&[0x32, 0x09, 0x40, 0xBD]);
    // LDR S19, [X9, #12]: 0xBD400D33
    code.extend(&[0x33, 0x0D, 0x40, 0xBD]);
    // LDR S20, [X9, #16]: 0xBD401134
    code.extend(&[0x34, 0x11, 0x40, 0xBD]);

    // Compute: result = bias + w0*in0 + w1*in1 + w2*in2 + w3*in3
    // FMOV Sd, S20 (copy bias to dest): 0x1E204280 | dest_reg
    let fmov_inst = 0x1E204280 | dest_reg;
    code.extend(&fmov_inst.to_le_bytes());

    // FMADD Sd, S16, S8, Sd: Sd = S16*S8 + Sd
    // Encoding: 0x1F080000 | (Sm << 16) | (Sa << 10) | (Sn << 5) | Sd
    // FMADD Sd, S16, S8, Sd
    let fmadd1 = 0x1F080000 | (8 << 16) | (dest_reg << 10) | (16 << 5) | dest_reg;
    code.extend(&fmadd1.to_le_bytes());
    // FMADD Sd, S17, S9, Sd
    let fmadd2 = 0x1F090000 | (9 << 16) | (dest_reg << 10) | (17 << 5) | dest_reg;
    code.extend(&fmadd2.to_le_bytes());
    // FMADD Sd, S18, S10, Sd
    let fmadd3 = 0x1F0A0000 | (10 << 16) | (dest_reg << 10) | (18 << 5) | dest_reg;
    code.extend(&fmadd3.to_le_bytes());
    // FMADD Sd, S19, S11, Sd
    let fmadd4 = 0x1F0B0000 | (11 << 16) | (dest_reg << 10) | (19 << 5) | dest_reg;
    code.extend(&fmadd4.to_le_bytes());
}

/// Emit scalar FMADD for Layer 2
/// Computes: output = w0*h0 + w1*h1 + w2*h2 + bias
/// where h0, h1, h2 are in S1, S2, S3 (from Layer 1 after ReLU)
fn emit_scalar_fmadd(code: &mut Vec<u8>, w0: f32, w1: f32, w2: f32, bias: f32) {
    // Store weights for literal pool
    code.extend_from_slice(cast_slice(&[w0, w1, w2, bias]));

    // ADR X9, #-16 (back to weights)
    code.extend(&[0x89, 0xFF, 0xFF, 0x10]);

    // Load weights
    // LDR S16, [X9, #0]
    code.extend(&[0x30, 0x01, 0x40, 0xBD]);
    // LDR S17, [X9, #4]
    code.extend(&[0x31, 0x05, 0x40, 0xBD]);
    // LDR S18, [X9, #8]
    code.extend(&[0x32, 0x09, 0x40, 0xBD]);
    // LDR S20, [X9, #12] (bias)
    code.extend(&[0x34, 0x0D, 0x40, 0xBD]);

    // FMOV S0, S20 (start with bias)
    code.extend(&[0x80, 0x42, 0x20, 0x1E]);

    // FMADD S0, S16, S1, S0: S0 = S16*S1 + S0
    let fmadd1: u32 = 0x1F010000 | (1 << 16) | (0 << 10) | (16 << 5) | 0;
    code.extend(&fmadd1.to_le_bytes());
    // FMADD S0, S17, S2, S0
    let fmadd2: u32 = 0x1F020000 | (2 << 16) | (0 << 10) | (17 << 5) | 0;
    code.extend(&fmadd2.to_le_bytes());
    // FMADD S0, S18, S3, S0
    let fmadd3: u32 = 0x1F030000 | (3 << 16) | (0 << 10) | (18 << 5) | 0;
    code.extend(&fmadd3.to_le_bytes());
}

/// Emit weights data section
fn emit_weights_data(code: &mut Vec<u8>, weights: &Weights) {
    // Align to 16 bytes for SIMD
    while code.len() % 16 != 0 {
        code.push(0x00);
    }

    // Store all weights in data section for reference
    let data_marker = b"WGHTS";
    code.extend_from_slice(data_marker);

    // Store W1 (4x3)
    for row in &weights.w1 {
        code.extend_from_slice(cast_slice(row));
    }

    // Store b1 (3)
    code.extend_from_slice(cast_slice(&weights.b1));

    // Store W2 (3)
    code.extend_from_slice(cast_slice(&weights.w2));

    // Store b2 (1)
    code.extend_from_slice(cast_slice(&[weights.b2]));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_generation() {
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

        // Basic sanity checks
        assert!(code.len() >= 64, "Generated code too small");
        assert!(code.len() <= 1024, "Generated code too large");

        // Check for function prologue
        assert_eq!(&code[0..4], [0xFD, 0x7B, 0xBF, 0xA9]); // stp x29, x30, [sp, #-16]!

        // Check that epilogue bytes exist somewhere in the code (before weights data section)
        // The epilogue is: ldp x29, x30, [sp], #16; ret
        let epilogue_ldp = [0xFD, 0x7B, 0xC1, 0xA8];
        let epilogue_ret = [0xC0, 0x03, 0x5F, 0xD6];

        // Search for the epilogue pattern in the code (it's before the weights data marker)
        let has_epilogue = code.windows(8).any(|w| {
            w[0..4] == epilogue_ldp && w[4..8] == epilogue_ret
        });
        assert!(has_epilogue, "Epilogue not found in generated code");
    }
}
