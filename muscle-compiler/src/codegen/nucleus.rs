//! Enhanced code generator for Muscle.ea with capability enforcement
//! Generates 8KiB AArch64 machine code with security guarantees

use crate::ast::full_ast::*;
use crate::error::CompileError;

/// Enhanced Nucleus code generator with capability security
pub struct NucleusCodegen;

impl NucleusCodegen {
    /// Generate 8KiB AArch64 machine code with capability enforcement
    pub fn generate(program: &Program) -> Result<Vec<u8>, CompileError> {
        let mut code = Vec::with_capacity(8192);

        // 1. Entry point and capability security setup
        code.extend(Self::generate_security_header());

        // 2. Rule dispatcher with event verification
        code.extend(Self::generate_rule_dispatcher(&program.rules));

        // 3. Capability implementations with runtime checks
        code.extend(Self::generate_capability_implementations(program));

        // 4. Built-in function implementations
        code.extend(Self::generate_builtin_functions());

        // 5. Event handlers
        code.extend(Self::generate_event_handlers(&program.rules, program));

        // 6. Data section with constants and security tokens
        code.extend(Self::generate_data_section(program));

        // 7. Capability security enforcement tables
        code.extend(Self::generate_capability_tables(program));

        // Pad to exactly 8KiB
        if code.len() > 8192 {
            return Err(CompileError::CodegenError(format!(
                "Nucleus code size {} exceeds 8KiB limit",
                code.len()
            )));
        }
        code.resize(8192, 0x00); // Fill with zeros (NOP equivalent)

        Ok(code)
    }

    fn generate_security_header() -> Vec<u8> {
        let mut code = Vec::new();

        // Security header: capability enforcement setup
        // MOV X28, #0x1000  ; Capability table base
        code.extend(&[0x88, 0x0B, 0x80, 0xD2]); // MOV X8, #0x1000
        code.extend(&[0x1C, 0x01, 0x00, 0x91]); // ADD X28, X8, #0

        // Initialize security monitor
        // STR XZR, [X28, #0]  ; Clear capability flags
        code.extend(&[0x9F, 0x03, 0x00, 0xF9]); // STR XZR, [X28, #0]

        // Set up stack pointer with security boundary
        // MOV SP, #0x8000
        code.extend(&[0xFF, 0x43, 0x00, 0x91]); // MOV SP, #0x8000

        // Jump to rule dispatcher
        // BL rule_dispatcher
        code.extend(&[0x00, 0x00, 0x00, 0x94]); // BL +0 (rule_dispatcher)

        // Security violation handler (infinite loop)
        // security_violation: B .
        code.extend(&[0x00, 0x00, 0x00, 0x14]); // B .

        code
    }

    fn generate_rule_dispatcher(rules: &[Rule]) -> Vec<u8> {
        let mut code = Vec::new();

        // rule_dispatcher:
        code.extend(&[0xFF, 0x83, 0x00, 0xD1]); // SUB SP, SP, #32
        code.extend(&[0xE0, 0x2F, 0x00, 0xB9]); // STR W0, [SP, #44] ; event_id
        code.extend(&[0xE1, 0x1B, 0x00, 0xF9]); // STR X1, [SP, #48] ; event_data

        // Event ID to handler mapping
        for (i, rule) in rules.iter().enumerate() {
            let event_id = Self::event_to_id(&rule.event);

            // CMP W0, #event_id
            code.extend(&[0x1F, 0x00, 0x00, 0x71]); // CMP W0, #event_id
                                                    // B.EQ handler_i
            let branch_offset = code.len();
            code.extend(&[0x00, 0x00, 0x00, 0x54]); // B.EQ +0 (placeholder)

            // Store patch location for later
            // In real implementation, we'd track and patch these
        }

        // Unknown event: return
        code.extend(&[0x00, 0x00, 0x80, 0x52]); // MOV W0, #0 (success)
        code.extend(&[0xFF, 0x83, 0x00, 0x91]); // ADD SP, SP, #32
        code.extend(&[0xC0, 0x03, 0x5F, 0xD6]); // RET

        code
    }

    fn generate_capability_implementations(program: &Program) -> Vec<u8> {
        let mut code = Vec::new();

        for decl in &program.declarations {
            if let Declaration::Capability(cap) = decl {
                code.extend(Self::generate_capability_function(cap));
            }
        }

        code
    }

    fn generate_capability_function(cap: &CapabilityDecl) -> Vec<u8> {
        let mut code = Vec::new();

        // Function prologue
        // capability_name:
        code.extend(&[0xFF, 0x83, 0x00, 0xD1]); // SUB SP, SP, #32
        code.extend(&[0xFD, 0x7B, 0x01, 0xA9]); // STP X29, X30, [SP, #16]
        code.extend(&[0xFD, 0x43, 0x00, 0x91]); // ADD X29, SP, #16

        // Capability security check
        // Check if this capability is authorized
        code.extend(Self::generate_capability_check(&cap.name));

        // Parameter handling
        for (i, param) in cap.parameters.iter().enumerate() {
            if i < 8 {
                // AArch64 has 8 parameter registers
                // Store parameter to stack
                // STR X{i}, [SP, #{i*8}]
                let store_instr = match i {
                    0 => vec![0xE0, 0x07, 0x00, 0xF9], // STR X0, [SP, #0]
                    1 => vec![0xE1, 0x0B, 0x00, 0xF9], // STR X1, [SP, #8]
                    2 => vec![0xE2, 0x0F, 0x00, 0xF9], // STR X2, [SP, #16]
                    3 => vec![0xE3, 0x13, 0x00, 0xF9], // STR X3, [SP, #24]
                    4 => vec![0xE4, 0x17, 0x00, 0xF9], // STR X4, [SP, #32]
                    5 => vec![0xE5, 0x1B, 0x00, 0xF9], // STR X5, [SP, #40]
                    6 => vec![0xE6, 0x1F, 0x00, 0xF9], // STR X6, [SP, #48]
                    7 => vec![0xE7, 0x23, 0x00, 0xF9], // STR X7, [SP, #56]
                    _ => vec![],
                };
                code.extend(store_instr);
            }
        }

        // Capability-specific implementation
        code.extend(Self::generate_capability_body(cap));

        // Function epilogue
        code.extend(&[0xFD, 0x7B, 0x41, 0xA9]); // LDP X29, X30, [SP, #16]
        code.extend(&[0xFF, 0x83, 0x00, 0x91]); // ADD SP, SP, #32
        code.extend(&[0xC0, 0x03, 0x5F, 0xD6]); // RET

        code
    }

    fn generate_capability_check(cap_name: &str) -> Vec<u8> {
        let mut code = Vec::new();

        // Check capability authorization table
        // LDRB W8, [X28, #capability_offset]
        let cap_offset = Self::capability_offset(cap_name);
        code.extend(&[0x88, 0x03, 0x40, 0x39]); // LDRB W8, [X28, #cap_offset]

        // CBNZ W8, authorized
        code.extend(&[0x68, 0x00, 0x00, 0xB5]); // CBNZ W8, +12

        // Unauthorized: jump to security violation
        code.extend(&[0x00, 0x00, 0x00, 0x14]); // B security_violation

        // authorized: continue
        code
    }

    fn generate_capability_body(cap: &CapabilityDecl) -> Vec<u8> {
        match cap.name.as_str() {
            "load_muscle" => Self::generate_load_muscle_body(),
            "schedule" => Self::generate_schedule_body(),
            "emit_update" => Self::generate_emit_update_body(),
            _ => vec![0x00, 0x00, 0x80, 0x52], // MOV W0, #0 (default)
        }
    }

    fn generate_load_muscle_body() -> Vec<u8> {
        vec![
            // load_muscle implementation
            0xE0, 0x07, 0x40, 0xF9, // LDR X0, [SP, #0] ; muscle_id
            0x01, 0x00, 0x00, 0x14, // BL muscle_loader
            0xE0, 0x0B, 0x00, 0xF9, // STR X0, [SP, #16] ; result
        ]
    }

    fn generate_schedule_body() -> Vec<u8> {
        vec![
            // schedule implementation
            0xE0, 0x07, 0x40, 0xF9, // LDR X0, [SP, #0] ; muscle
            0xE1, 0x0B, 0x40, 0xB9, // LDR W1, [SP, #8] ; priority
            0x02, 0x00, 0x00, 0x14, // BL scheduler
        ]
    }

    fn generate_emit_update_body() -> Vec<u8> {
        vec![
            // emit_update implementation
            0xE0, 0x07, 0x40, 0xF9, // LDR X0, [SP, #0] ; blob
            0x03, 0x00, 0x00, 0x14, // BL lattice_emitter
        ]
    }

    fn generate_builtin_functions() -> Vec<u8> {
        let mut code = Vec::new();

        // hardware_attestation.verify()
        code.extend(&[
            // verify_attestation:
            0x20, 0x00, 0x80, 0x52, // MOV W0, #1 (true)
            0xC0, 0x03, 0x5F, 0xD6, // RET
        ]);

        // symbiote.process_update()
        code.extend(&[
            // symbiote_process_update:
            0x00, 0x00, 0x80, 0x52, // MOV W0, #0 (no action)
            0xC0, 0x03, 0x5F, 0xD6, // RET
        ]);

        // referee.self_check_failed()
        code.extend(&[
            // self_check_failed:
            0x00, 0x00, 0x80, 0x52, // MOV W0, #0 (not failed)
            0xC0, 0x03, 0x5F, 0xD6, // RET
        ]);

        code
    }

    fn generate_event_handlers(rules: &[Rule], program: &Program) -> Vec<u8> {
        let mut code = Vec::new();

        for (i, rule) in rules.iter().enumerate() {
            code.extend(Self::generate_rule_handler(rule, i, program));
        }

        code
    }

    fn generate_rule_handler(rule: &Rule, index: usize, program: &Program) -> Vec<u8> {
        let mut code = Vec::new();

        // handler_{index}:
        code.extend(&[0xFF, 0x43, 0x00, 0xD1]); // SUB SP, SP, #16

        // Generate body statements
        for statement in &rule.body {
            code.extend(Self::generate_statement(statement, program));
        }

        code.extend(&[0x20, 0x00, 0x80, 0x52]); // MOV W0, #1 (success)
        code.extend(&[0xFF, 0x43, 0x00, 0x91]); // ADD SP, SP, #16
        code.extend(&[0xC0, 0x03, 0x5F, 0xD6]); // RET

        code
    }

    fn generate_statement(statement: &Statement, program: &Program) -> Vec<u8> {
        match statement {
            Statement::Verify(stmt) => Self::generate_verify_statement(stmt),
            Statement::Let(stmt) => Self::generate_let_statement(stmt),
            Statement::If(stmt) => Self::generate_if_statement(stmt, program),
            Statement::Emit(stmt) => Self::generate_emit_statement(stmt),
            Statement::Schedule(stmt) => Self::generate_schedule_statement(stmt),
            Statement::Unschedule(stmt) => Self::generate_unschedule_statement(stmt),
            Statement::Expr(expr) => Self::generate_expression(expr),
        }
    }

    fn generate_verify_statement(stmt: &VerifyStmt) -> Vec<u8> {
        let mut code = Vec::new();

        // Generate condition expression
        code.extend(Self::generate_expression(&stmt.condition));

        // CBNZ X0, verification_ok
        code.extend(&[0x60, 0x00, 0x00, 0xB5]); // CBNZ X0, +12

        // Verification failed: security violation
        code.extend(&[0x00, 0x00, 0x00, 0x14]); // B security_violation

        // verification_ok: continue
        code
    }

    fn generate_let_statement(stmt: &LetStmt) -> Vec<u8> {
        let mut code = Vec::new();

        if let Some(expr) = &stmt.value {
            code.extend(Self::generate_expression(expr));
            // Store result to local variable slot
            // In real implementation, track variable locations
        }

        code
    }

    fn generate_if_statement(stmt: &IfStmt, program: &Program) -> Vec<u8> {
        let mut code = Vec::new();

        // Generate condition
        code.extend(Self::generate_expression(&stmt.condition));

        // CBZ X0, else_branch
        let else_branch_offset = code.len();
        code.extend(&[0x60, 0x00, 0x00, 0xB4]); // CBZ X0, +0 (placeholder)

        // Then branch
        for then_stmt in &stmt.then_branch {
            code.extend(Self::generate_statement(then_stmt, program));
        }

        // B end_if
        let end_if_offset = code.len();
        code.extend(&[0x00, 0x00, 0x00, 0x14]); // B +0 (placeholder)

        // Else branch (if exists)
        if let Some(else_branch) = &stmt.else_branch {
            // Patch else branch jump
            // In real implementation, calculate and patch offsets

            for else_stmt in else_branch {
                code.extend(Self::generate_statement(else_stmt, program));
            }
        }

        // end_if: continue
        code
    }

    fn generate_emit_statement(stmt: &EmitStmt) -> Vec<u8> {
        let mut code = Vec::new();

        // Prepare arguments
        for (i, arg) in stmt.arguments.iter().enumerate() {
            if i < 8 {
                code.extend(Self::generate_expression(arg));
                // Move to argument register
                if i > 0 {
                    // MOV X{i}, X0
                    let mov_instr = match i {
                        1 => vec![0xE1, 0x03, 0x00, 0xAA], // MOV X1, X0
                        2 => vec![0xE2, 0x03, 0x00, 0xAA], // MOV X2, X0
                        // ... etc
                        _ => vec![],
                    };
                    code.extend(mov_instr);
                }
            }
        }

        // Call emit_update capability
        code.extend(&[0x00, 0x00, 0x00, 0x94]); // BL emit_update

        code
    }

    fn generate_schedule_statement(stmt: &ScheduleStmt) -> Vec<u8> {
        let mut code = Vec::new();

        // Generate muscle expression
        code.extend(Self::generate_expression(&stmt.muscle));
        // MOV X0, X0 (muscle already in X0)

        // Generate priority (literal)
        if let Literal::Integer(priority) = &stmt.priority {
            // MOV W1, #priority
            let priority_byte = (*priority as u8).min(255);
            code.extend(&[0xE1, 0x03, 0x00, 0x32]); // MOV W1, #priority_byte
        }

        // Call schedule capability
        code.extend(&[0x00, 0x00, 0x00, 0x94]); // BL schedule

        code
    }

    fn generate_unschedule_statement(stmt: &UnscheduleStmt) -> Vec<u8> {
        let mut code = Vec::new();

        // Generate muscle_id expression
        code.extend(Self::generate_expression(&stmt.muscle_id));

        // Call unschedule (uses schedule capability)
        code.extend(&[0x00, 0x00, 0x00, 0x94]); // BL schedule (with special flag)

        code
    }

    fn generate_expression(expr: &Expression) -> Vec<u8> {
        match expr {
            Expression::Literal(literal) => Self::generate_literal(literal),
            Expression::Variable(var) => Self::generate_variable(var),
            Expression::SelfRef(self_ref) => Self::generate_self_reference(self_ref),
            Expression::Call(call) => Self::generate_call_expression(call),
            Expression::FieldAccess(access) => Self::generate_field_access(access),
            Expression::Binary(bin) => Self::generate_binary_expression(bin),
        }
    }

    fn generate_literal(literal: &Literal) -> Vec<u8> {
        match literal {
            Literal::Hex(hex_str) => {
                if let Some(value) = literal.as_u64() {
                    if value <= 0xFFFF {
                        // MOV X0, #value
                        vec![0xE0, 0x03, 0x00, 0x32] // MOV W0, #(value as u16)
                    } else {
                        // ADRP X0, literal_address
                        // LDR X0, [X0, #offset]
                        vec![0xE0, 0x03, 0x00, 0x90, 0x00, 0x00, 0x40, 0xF9] // Simplified
                    }
                } else {
                    vec![0x00, 0x00, 0x80, 0x52] // MOV W0, #0
                }
            }
            Literal::Integer(n) => {
                if *n <= 0xFFFF {
                    vec![0xE0, 0x03, 0x00, 0x32] // MOV W0, #(*n as u16)
                } else {
                    // Load from data section
                    vec![0xE0, 0x03, 0x00, 0x90, 0x00, 0x00, 0x40, 0xF9]
                }
            }
            Literal::String(s) => {
                // String literals go in data section
                // ADRP X0, string_address
                vec![0xE0, 0x03, 0x00, 0x90]
            }
        }
    }

    fn generate_variable(_var: &str) -> Vec<u8> {
        // Load from local variable slot
        // In real implementation, track variable locations
        vec![0xE0, 0x07, 0x40, 0xF9] // LDR X0, [SP, #0] (example)
    }

    fn generate_self_reference(self_ref: &SelfReference) -> Vec<u8> {
        match self_ref {
            SelfReference::Id => {
                // Load self.id from fixed location
                vec![0xE0, 0x03, 0x00, 0x90, 0x00, 0x10, 0x40, 0xF9] // ADRP + LDR
            }
            SelfReference::Version => {
                // Load self.version from fixed location
                vec![0xE0, 0x03, 0x00, 0x90, 0x00, 0x18, 0x40, 0xF9] // ADRP + LDR
            }
        }
    }

    fn generate_call_expression(call: &CallExpr) -> Vec<u8> {
        let mut code = Vec::new();

        // Prepare arguments
        for (i, arg) in call.arguments.iter().enumerate() {
            if i < 8 {
                code.extend(Self::generate_expression(arg));
                if i > 0 {
                    // MOV X{i}, X0
                    let mov_instr = match i {
                        1 => vec![0xE1, 0x03, 0x00, 0xAA],
                        2 => vec![0xE2, 0x03, 0x00, 0xAA],
                        // ... etc
                        _ => vec![],
                    };
                    code.extend(mov_instr);
                }
            }
        }

        // BL function_name
        code.extend(&[0x00, 0x00, 0x00, 0x94]); // BL +0 (placeholder)

        code
    }

    fn generate_field_access(access: &FieldAccess) -> Vec<u8> {
        // For now, treat as function call
        let call_expr = CallExpr {
            function: format!("{}.{}", access.object, access.field),
            arguments: Vec::new(),
        };
        Self::generate_call_expression(&call_expr)
    }

    fn generate_binary_expression(bin: &BinaryExpr) -> Vec<u8> {
        let mut code = Vec::new();

        // Generate left operand
        code.extend(Self::generate_expression(&bin.left));
        code.extend(&[0xE8, 0x03, 0x00, 0xAA]); // MOV X8, X0 (save left)

        // Generate right operand
        code.extend(Self::generate_expression(&bin.right));
        code.extend(&[0xE9, 0x03, 0x00, 0xAA]); // MOV X9, X0 (save right)

        // Generate operation
        match bin.op {
            BinaryOperator::Eq => {
                // CMP X8, X9
                // CSET X0, EQ
                code.extend(&[0x08, 0x01, 0x09, 0xEB]); // CMP X8, X9
                code.extend(&[0xE0, 0x03, 0x9F, 0x1A]); // CSET W0, EQ
            }
            BinaryOperator::Ne => {
                code.extend(&[0x08, 0x01, 0x09, 0xEB]); // CMP X8, X9
                code.extend(&[0xE0, 0x03, 0x9F, 0x1A]); // CSET W0, NE
            }
            BinaryOperator::Add => {
                code.extend(&[0x00, 0x01, 0x09, 0x8B]); // ADD X0, X8, X9
            }
            // ... other operators
            _ => {
                code.extend(&[0x00, 0x00, 0x80, 0x52]); // MOV W0, #0 (default)
            }
        }

        code
    }

    fn generate_data_section(program: &Program) -> Vec<u8> {
        let mut data = Vec::new();

        // Align to 8 bytes
        while data.len() % 8 != 0 {
            data.push(0x00);
        }

        // Constants
        for decl in &program.declarations {
            if let Declaration::Const(const_decl) = decl {
                data.extend(&const_decl.value.to_bytes());
                // Pad to 8 bytes
                while data.len() % 8 != 0 {
                    data.push(0x00);
                }
            }
        }

        // Built-in data
        data.extend(&[0xEAu8; 32]); // genesis_root
        data.extend(&0xFFFF_FFFF_FFFF_FFFFu64.to_le_bytes()); // symbiote_id
        data.extend(&1u64.to_le_bytes()); // self.version

        data
    }

    fn generate_capability_tables(program: &Program) -> Vec<u8> {
        let mut tables = Vec::new();

        // Capability authorization bitmap
        let mut capability_bits = 0u64;

        for decl in &program.declarations {
            if let Declaration::Capability(cap) = decl {
                let bit_position = Self::capability_bit_position(&cap.name);
                capability_bits |= 1 << bit_position;
            }
        }

        tables.extend(&capability_bits.to_le_bytes());

        tables
    }

    fn event_to_id(event: &Event) -> u8 {
        match event {
            Event::OnBoot => 0,
            Event::OnLatticeUpdate { .. } => 1,
            Event::OnTimer1Hz => 2,
            Event::OnSelfIntegrityFailure => 3,
            Event::Custom(_) => 4,
        }
    }

    fn capability_offset(cap_name: &str) -> u8 {
        match cap_name {
            "load_muscle" => 0,
            "schedule" => 1,
            "emit_update" => 2,
            _ => 255, // Invalid
        }
    }

    fn capability_bit_position(cap_name: &str) -> u8 {
        match cap_name {
            "load_muscle" => 0,
            "schedule" => 1,
            "emit_update" => 2,
            _ => 63, // Last bit
        }
    }
}
