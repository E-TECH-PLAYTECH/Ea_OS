//! Code generator for Nucleus muscles - produces 8KiB AArch64 machine code

use crate::ast::{MuscleAst, Rule, Capability};
use crate::error::CompileError;

/// Generates 8KiB Nucleus muscle binaries
pub struct NucleusCodegen;

impl NucleusCodegen {
    /// Generate 8KiB AArch64 machine code for Nucleus muscle
    pub fn generate(ast: &MuscleAst) -> Result<Vec<u8>, CompileError> {
        let mut code = Vec::with_capacity(8192);
        
        // 1. Entry point and initialization
        code.extend(Self::generate_entry_point());
        
        // 2. Rule dispatcher
        code.extend(Self::generate_rule_engine(&ast.rules));
        
        // 3. Capability implementations
        code.extend(Self::generate_capabilities(&ast.capabilities));
        
        // 4. Input handlers
        code.extend(Self::generate_input_handlers(&ast.inputs));
        
        // 5. Data section with fixed addresses
        code.extend(Self::generate_data_section());
        
        // Pad to exactly 8KiB
        if code.len() > 8192 {
            return Err(CompileError::CodegenError(
                format!("Nucleus code size {} exceeds 8KiB limit", code.len())
            ));
        }
        code.resize(8192, 0);
        
        Ok(code)
    }
    
    fn generate_entry_point() -> Vec<u8> {
        // AArch64 entry point that sets up stack and calls rule dispatcher
        let mut code = Vec::new();
        
        // Entry point at offset 0
        // MOV SP, #0x8000 (set stack pointer)
        code.extend(&[0xE0, 0x03, 0x00, 0x91]); // MOV X0, #0x8000
        code.extend(&[0xFF, 0x03, 0x00, 0x91]); // MOV SP, X0
        
        // BL rule_dispatcher
        code.extend(&[0x00, 0x00, 0x00, 0x94]); // BL +0 (rule_dispatcher)
        
        // Infinite loop if return (should never happen)
        code.extend(&[0x00, 0x00, 0x00, 0x14]); // B . (infinite loop)
        
        code
    }
    
    fn generate_rule_engine(rules: &[Rule]) -> Vec<u8> {
        let mut code = Vec::new();
        
        // Rule dispatcher function
        // rule_dispatcher:
        code.extend(&[0xFF, 0x43, 0x00, 0xD1]); // SUB SP, SP, #16
        code.extend(&[0xE0, 0x0F, 0x00, 0xB9]); // STR W0, [SP, #12]
        
        // Check rule type and jump to appropriate handler
        for (i, rule) in rules.iter().enumerate() {
            // CMP W0, #rule_id
            code.extend(&[0x1F, 0x00, 0x00, 0x71]); // CMP W0, #i
            // B.EQ rule_handler
            code.extend(&[0x00, 0x00, 0x00, 0x54]); // B.EQ +0 (placeholder)
            
            // Store offset to fix up later
            let branch_offset = code.len() - 4;
            // We'll fix these up after generating rule handlers
        }
        
        // Default: return
        code.extend(&[0xFF, 0x43, 0x00, 0x91]); // ADD SP, SP, #16
        code.extend(&[0xC0, 0x03, 0x5F, 0xD6]); // RET
        
        // Generate individual rule handlers
        for (i, rule) in rules.iter().enumerate() {
            let handler_start = code.len();
            
            // rule_handler_i:
            match rule.name.as_str() {
                "on_boot" => code.extend(Self::generate_boot_rule(rule)),
                "on_lattice_update" => code.extend(Self::generate_update_rule(rule)),
                "on_timer_1hz" => code.extend(Self::generate_timer_rule(rule)),
                _ => code.extend(Self::generate_generic_rule(rule)),
            }
            
            // Fix up branch offsets (simplified - real implementation would track and patch)
        }
        
        code
    }
    
    fn generate_boot_rule(rule: &Rule) -> Vec<u8> {
        let mut code = Vec::new();
        
        // verify hardware_attestation
        code.extend(&[0x00, 0x00, 0x80, 0x52]); // MOV W0, #0
        code.extend(&[0x01, 0x00, 0x00, 0x14]); // BL verify_attestation
        
        // verify lattice_root == genesis
        code.extend(&[0x00, 0x00, 0x80, 0x52]); // MOV W0, #0  
        code.extend(&[0x02, 0x00, 0x00, 0x14]); // BL verify_lattice_root
        
        // load_muscle(symbiote_id)
        code.extend(&[0xE0, 0xFF, 0xFF, 0x10]); // ADRL X0, symbiote_id
        code.extend(&[0x03, 0x00, 0x00, 0x14]); // BL load_muscle
        
        // schedule(symbiote, 255)
        code.extend(&[0x20, 0x00, 0x80, 0x52]); // MOV W0, #1 (symbiote slot)
        code.extend(&[0xE1, 0xFF, 0x9F, 0x52]); // MOV W1, #255
        code.extend(&[0x04, 0x00, 0x00, 0x14]); // BL schedule
        
        code
    }
    
    fn generate_update_rule(rule: &Rule) -> Vec<u8> {
        let mut code = Vec::new();
        
        // if symbiote.process(update) -> healing:
        code.extend(&[0xE0, 0x0F, 0x40, 0xB9]); // LDR W0, [SP, #12] (update param)
        code.extend(&[0x05, 0x00, 0x00, 0x14]); // BL symbiote_process
        
        // CBNZ X0, healing_branch
        code.extend(&[0x60, 0x00, 0x00, 0xB5]); // CBNZ X0, +8
        
        // RET (no healing)
        code.extend(&[0xFF, 0x43, 0x00, 0x91]); // ADD SP, SP, #16
        code.extend(&[0xC0, 0x03, 0x5F, 0xD6]); // RET
        
        // healing_branch:
        // emit_update(healing.blob)
        code.extend(&[0xE0, 0x03, 0x00, 0xAA]); // MOV X0, X0 (healing.blob)
        code.extend(&[0x06, 0x00, 0x00, 0x14]); // BL emit_update
        
        code
    }
    
    fn generate_timer_rule(rule: &Rule) -> Vec<u8> {
        let mut code = Vec::new();
        
        // emit heartbeat(self.id, self.version)
        code.extend(&[0xE0, 0x03, 0x1F, 0xAA]); // MOV X0, XZR (self.id - will be patched)
        code.extend(&[0xE1, 0x03, 0x1F, 0xAA]); // MOV X1, XZR (self.version - will be patched)
        code.extend(&[0x07, 0x00, 0x00, 0x14]); // BL emit_heartbeat
        
        code
    }
    
    fn generate_generic_rule(rule: &Rule) -> Vec<u8> {
        // Generic rule implementation
        vec![0xFF, 0x43, 0x00, 0x91, 0xC0, 0x03, 0x5F, 0xD6] // ADD SP, SP, #16; RET
    }
    
    fn generate_capabilities(capabilities: &[Capability]) -> Vec<u8> {
        let mut code = Vec::new();
        
        for cap in capabilities {
            match cap.name.as_str() {
                "load_muscle" => code.extend(Self::generate_load_muscle_cap()),
                "schedule" => code.extend(Self::generate_schedule_cap()),
                "emit_update" => code.extend(Self::generate_emit_update_cap()),
                _ => {}
            }
        }
        
        code
    }
    
    fn generate_load_muscle_cap() -> Vec<u8> {
        // load_muscle implementation
        vec![
            // load_muscle:
            0xFF, 0x43, 0x00, 0xD1, // SUB SP, SP, #16
            0xE0, 0x0F, 0x00, 0xF9, // STR X0, [SP, #8]
            // ... actual implementation
            0xFF, 0x43, 0x00, 0x91, // ADD SP, SP, #16
            0xC0, 0x03, 0x5F, 0xD6, // RET
        ]
    }
    
    fn generate_schedule_cap() -> Vec<u8> {
        // schedule implementation  
        vec![
            // schedule:
            0xFF, 0x83, 0x00, 0xD1, // SUB SP, SP, #32
            0xE0, 0x0B, 0x00, 0xB9, // STR W0, [SP, #8]
            0xE1, 0x07, 0x00, 0xB9, // STR W1, [SP, #4]
            // ... actual implementation
            0xFF, 0x83, 0x00, 0x91, // ADD SP, SP, #32
            0xC0, 0x03, 0x5F, 0xD6, // RET
        ]
    }
    
    fn generate_emit_update_cap() -> Vec<u8> {
        // emit_update implementation
        vec![
            // emit_update:
            0xFF, 0x43, 0x00, 0xD1, // SUB SP, SP, #16
            0xE0, 0x0F, 0x00, 0xF9, // STR X0, [SP, #8]
            // ... actual implementation
            0xFF, 0x43, 0x00, 0x91, // ADD SP, SP, #16
            0xC0, 0x03, 0x5F, 0xD6, // RET
        ]
    }
    
    fn generate_input_handlers(inputs: &[InputDeclaration]) -> Vec<u8> {
        let mut code = Vec::new();
        
        for input in inputs {
            match input.name.as_str() {
                "lattice_stream" => code.extend(Self::generate_lattice_handler()),
                "hardware_attestation" => code.extend(Self::generate_attestation_handler()),
                "symbiote" => code.extend(Self::generate_symbiote_handler()),
                _ => {}
            }
        }
        
        code
    }
    
    fn generate_lattice_handler() -> Vec<u8> {
        vec![
            // get_lattice_update:
            0x00, 0x00, 0x80, 0x52, // MOV W0, #0
            0xC0, 0x03, 0x5F, 0xD6, // RET
        ]
    }
    
    fn generate_attestation_handler() -> Vec<u8> {
        vec![
            // verify_attestation:
            0x20, 0x00, 0x80, 0x52, // MOV W0, #1 (true)
            0xC0, 0x03, 0x5F, 0xD6, // RET
        ]
    }
    
    fn generate_symbiote_handler() -> Vec<u8> {
        vec![
            // symbiote_process:
            0x00, 0x00, 0x80, 0x52, // MOV W0, #0 (false/no healing)
            0xC0, 0x03, 0x5F, 0xD6, // RET
        ]
    }
    
    fn generate_data_section() -> Vec<u8> {
        let mut data = Vec::new();
        
        // Alignment to 8-byte boundary
        while data.len() % 8 != 0 {
            data.push(0);
        }
        
        // symbiote_id: u64 = 0xFFFF_FFFF_FFFF_FFFF
        data.extend(&0xFFFF_FFFF_FFFF_FFFFu64.to_le_bytes());
        
        // genesis_root: [u8; 32] = 0xEA...
        let mut genesis = [0u8; 32];
        genesis[0] = 0xEA;
        data.extend(&genesis);
        
        data
    }
}
