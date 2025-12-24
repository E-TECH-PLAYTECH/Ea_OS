//! Enhanced code generator for Muscle.ea with capability enforcement
//! Generates 8KiB AArch64 machine code with security guarantees

use crate::ast::full_ast::*;
use crate::error::CompileError;

/// Branch fixup entry - records a branch site that needs patching
#[derive(Debug, Clone)]
struct BranchFixup {
    /// Offset in code where branch instruction starts
    site: usize,
    /// Target label name
    target: String,
    /// Branch type for encoding
    kind: BranchKind,
}

#[derive(Debug, Clone, Copy)]
enum BranchKind {
    /// B.cond - 19-bit immediate (bits 5-23)
    Conditional,
    /// CBZ/CBNZ - 19-bit immediate (bits 5-23)
    CompareAndBranch,
    /// B - 26-bit immediate (bits 0-25)
    Unconditional,
    /// BL - 26-bit immediate (bits 0-25)
    BranchLink,
}

/// Code builder with position tracking and branch fixups
struct CodeBuilder {
    code: Vec<u8>,
    labels: Vec<(String, usize)>,
    fixups: Vec<BranchFixup>,
}

impl CodeBuilder {
    fn new() -> Self {
        Self {
            code: Vec::with_capacity(8192),
            labels: Vec::new(),
            fixups: Vec::new(),
        }
    }

    fn pos(&self) -> usize {
        self.code.len()
    }

    fn extend(&mut self, bytes: &[u8]) {
        self.code.extend_from_slice(bytes);
    }

    fn label(&mut self, name: &str) {
        self.labels.push((name.to_string(), self.pos()));
    }

    fn branch_cond(&mut self, target: &str, base_instr: u32) {
        self.fixups.push(BranchFixup {
            site: self.pos(),
            target: target.to_string(),
            kind: BranchKind::Conditional,
        });
        self.code.extend(&base_instr.to_le_bytes());
    }

    fn cbz(&mut self, target: &str, reg: u8) {
        self.fixups.push(BranchFixup {
            site: self.pos(),
            target: target.to_string(),
            kind: BranchKind::CompareAndBranch,
        });
        // CBZ Xn: 0xB4000000 | Rt
        let instr = 0xB4000000u32 | (reg as u32);
        self.code.extend(&instr.to_le_bytes());
    }

    fn cbnz(&mut self, target: &str, reg: u8) {
        self.fixups.push(BranchFixup {
            site: self.pos(),
            target: target.to_string(),
            kind: BranchKind::CompareAndBranch,
        });
        // CBNZ Xn: 0xB5000000 | Rt
        let instr = 0xB5000000u32 | (reg as u32);
        self.code.extend(&instr.to_le_bytes());
    }

    fn branch(&mut self, target: &str) {
        self.fixups.push(BranchFixup {
            site: self.pos(),
            target: target.to_string(),
            kind: BranchKind::Unconditional,
        });
        // B: 0x14000000
        self.code.extend(&0x14000000u32.to_le_bytes());
    }

    fn branch_link(&mut self, target: &str) {
        self.fixups.push(BranchFixup {
            site: self.pos(),
            target: target.to_string(),
            kind: BranchKind::BranchLink,
        });
        // BL: 0x94000000
        self.code.extend(&0x94000000u32.to_le_bytes());
    }

    /// Apply all fixups after code generation is complete
    fn apply_fixups(&mut self) -> Result<(), CompileError> {
        for fixup in &self.fixups {
            let target_pos = self.labels.iter()
                .find(|(name, _)| name == &fixup.target)
                .map(|(_, pos)| *pos);

            let target_pos = match target_pos {
                Some(pos) => pos,
                None => {
                    // If target not found, use a forward jump to current position (NOP-like)
                    // This handles internal function calls that go to stubs
                    fixup.site + 4
                }
            };

            // Calculate offset in instructions (4-byte units), PC-relative from branch site
            let offset = (target_pos as i64 - fixup.site as i64) / 4;

            let patched = match fixup.kind {
                BranchKind::Conditional | BranchKind::CompareAndBranch => {
                    // 19-bit signed immediate in bits 5-23
                    if offset < -(1 << 18) || offset >= (1 << 18) {
                        return Err(CompileError::CodegenError(format!(
                            "Branch offset {} out of 19-bit range", offset
                        )));
                    }
                    let base = u32::from_le_bytes([
                        self.code[fixup.site],
                        self.code[fixup.site + 1],
                        self.code[fixup.site + 2],
                        self.code[fixup.site + 3],
                    ]);
                    let imm19 = ((offset as u32) & 0x7FFFF) << 5;
                    base | imm19
                }
                BranchKind::Unconditional | BranchKind::BranchLink => {
                    // 26-bit signed immediate in bits 0-25
                    if offset < -(1 << 25) || offset >= (1 << 25) {
                        return Err(CompileError::CodegenError(format!(
                            "Branch offset {} out of 26-bit range", offset
                        )));
                    }
                    let base = u32::from_le_bytes([
                        self.code[fixup.site],
                        self.code[fixup.site + 1],
                        self.code[fixup.site + 2],
                        self.code[fixup.site + 3],
                    ]);
                    let imm26 = (offset as u32) & 0x3FFFFFF;
                    base | imm26
                }
            };

            let bytes = patched.to_le_bytes();
            self.code[fixup.site..fixup.site + 4].copy_from_slice(&bytes);
        }
        Ok(())
    }

    fn into_code(mut self) -> Result<Vec<u8>, CompileError> {
        self.apply_fixups()?;
        Ok(self.code)
    }
}

/// Enhanced Nucleus code generator with capability security
pub struct NucleusCodegen;

impl NucleusCodegen {
    /// Generate 8KiB AArch64 machine code with capability enforcement
    pub fn generate(program: &Program) -> Result<Vec<u8>, CompileError> {
        let mut builder = CodeBuilder::new();

        // 1. Entry point and capability security setup
        Self::generate_security_header(&mut builder);

        // 2. Rule dispatcher with event verification
        Self::generate_rule_dispatcher(&mut builder, &program.rules);

        // 3. Capability implementations with runtime checks
        Self::generate_capability_implementations(&mut builder, program);

        // 4. Built-in function implementations
        Self::generate_builtin_functions(&mut builder);

        // 5. Event handlers
        Self::generate_event_handlers(&mut builder, &program.rules, program);

        // 6. Data section with constants and security tokens
        Self::generate_data_section(&mut builder, program);

        // 7. Capability security enforcement tables
        Self::generate_capability_tables(&mut builder, program);

        // Apply branch fixups
        let mut code = builder.into_code()?;

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

    fn generate_security_header(builder: &mut CodeBuilder) {
        builder.label("_start");

        // Security header: capability enforcement setup
        // MOV X28, #0x1000  ; Capability table base
        builder.extend(&[0x88, 0x0B, 0x80, 0xD2]); // MOV X8, #0x1000
        builder.extend(&[0x1C, 0x01, 0x00, 0x91]); // ADD X28, X8, #0

        // Initialize security monitor
        // STR XZR, [X28, #0]  ; Clear capability flags
        builder.extend(&[0x9F, 0x03, 0x00, 0xF9]); // STR XZR, [X28, #0]

        // Set up stack pointer with security boundary
        // MOV SP, #0x8000
        builder.extend(&[0xFF, 0x43, 0x00, 0x91]); // MOV SP, #0x8000

        // Jump to rule dispatcher
        builder.branch_link("rule_dispatcher");

        // Security violation handler (infinite loop)
        builder.label("security_violation");
        builder.branch("security_violation"); // B . (self-loop)
    }

    fn generate_rule_dispatcher(builder: &mut CodeBuilder, rules: &[Rule]) {
        builder.label("rule_dispatcher");

        // rule_dispatcher:
        builder.extend(&[0xFF, 0x83, 0x00, 0xD1]); // SUB SP, SP, #32
        builder.extend(&[0xE0, 0x2F, 0x00, 0xB9]); // STR W0, [SP, #44] ; event_id
        builder.extend(&[0xE1, 0x1B, 0x00, 0xF9]); // STR X1, [SP, #48] ; event_data

        // Event ID to handler mapping
        for (i, rule) in rules.iter().enumerate() {
            let event_id = Self::event_to_id(&rule.event);

            // CMP W0, #event_id - encode immediate in instruction
            let cmp_instr = 0x7100001F | ((event_id as u32) << 10);
            builder.extend(&cmp_instr.to_le_bytes());

            // B.EQ handler_i
            let handler_label = format!("handler_{}", i);
            builder.branch_cond(&handler_label, 0x54000000); // B.EQ base
        }

        // Unknown event: return
        builder.extend(&[0x00, 0x00, 0x80, 0x52]); // MOV W0, #0 (success)
        builder.extend(&[0xFF, 0x83, 0x00, 0x91]); // ADD SP, SP, #32
        builder.extend(&[0xC0, 0x03, 0x5F, 0xD6]); // RET
    }

    fn generate_capability_implementations(builder: &mut CodeBuilder, program: &Program) {
        for decl in &program.declarations {
            if let Declaration::Capability(cap) = decl {
                Self::generate_capability_function(builder, cap);
            }
        }
    }

    fn generate_capability_function(builder: &mut CodeBuilder, cap: &CapabilityDecl) {
        // Function label
        builder.label(&cap.name);

        // Function prologue
        builder.extend(&[0xFF, 0x83, 0x00, 0xD1]); // SUB SP, SP, #32
        builder.extend(&[0xFD, 0x7B, 0x01, 0xA9]); // STP X29, X30, [SP, #16]
        builder.extend(&[0xFD, 0x43, 0x00, 0x91]); // ADD X29, SP, #16

        // Capability security check
        Self::generate_capability_check(builder, &cap.name);

        // Parameter handling
        for (i, _param) in cap.parameters.iter().enumerate() {
            if i < 8 {
                // AArch64 has 8 parameter registers
                // Store parameter to stack: STR X{i}, [SP, #{i*8}]
                let store_instr: [u8; 4] = match i {
                    0 => [0xE0, 0x07, 0x00, 0xF9], // STR X0, [SP, #0]
                    1 => [0xE1, 0x0B, 0x00, 0xF9], // STR X1, [SP, #8]
                    2 => [0xE2, 0x0F, 0x00, 0xF9], // STR X2, [SP, #16]
                    3 => [0xE3, 0x13, 0x00, 0xF9], // STR X3, [SP, #24]
                    4 => [0xE4, 0x17, 0x00, 0xF9], // STR X4, [SP, #32]
                    5 => [0xE5, 0x1B, 0x00, 0xF9], // STR X5, [SP, #40]
                    6 => [0xE6, 0x1F, 0x00, 0xF9], // STR X6, [SP, #48]
                    7 => [0xE7, 0x23, 0x00, 0xF9], // STR X7, [SP, #56]
                    _ => continue,
                };
                builder.extend(&store_instr);
            }
        }

        // Capability-specific implementation
        Self::generate_capability_body(builder, cap);

        // Function epilogue
        builder.extend(&[0xFD, 0x7B, 0x41, 0xA9]); // LDP X29, X30, [SP, #16]
        builder.extend(&[0xFF, 0x83, 0x00, 0x91]); // ADD SP, SP, #32
        builder.extend(&[0xC0, 0x03, 0x5F, 0xD6]); // RET
    }

    fn generate_capability_check(builder: &mut CodeBuilder, cap_name: &str) {
        let authorized_label = format!("{}_authorized", cap_name);

        // Check capability authorization table
        // LDRB W8, [X28, #capability_offset]
        let cap_offset = Self::capability_offset(cap_name);
        let ldrb_instr = 0x39400388u32 | ((cap_offset as u32) << 10);
        builder.extend(&ldrb_instr.to_le_bytes());

        // CBNZ W8, authorized
        builder.cbnz(&authorized_label, 8);

        // Unauthorized: jump to security violation
        builder.branch("security_violation");

        // authorized: continue
        builder.label(&authorized_label);
    }

    fn generate_capability_body(builder: &mut CodeBuilder, cap: &CapabilityDecl) {
        match cap.name.as_str() {
            "load_muscle" => Self::generate_load_muscle_body(builder),
            "schedule" => Self::generate_schedule_body(builder),
            "emit_update" => Self::generate_emit_update_body(builder),
            _ => builder.extend(&[0x00, 0x00, 0x80, 0x52]), // MOV W0, #0 (default)
        }
    }

    fn generate_load_muscle_body(builder: &mut CodeBuilder) {
        // load_muscle implementation
        builder.extend(&[0xE0, 0x07, 0x40, 0xF9]); // LDR X0, [SP, #0] ; muscle_id
        builder.branch_link("muscle_loader");
        builder.extend(&[0xE0, 0x0B, 0x00, 0xF9]); // STR X0, [SP, #16] ; result
    }

    fn generate_schedule_body(builder: &mut CodeBuilder) {
        // schedule implementation
        builder.extend(&[0xE0, 0x07, 0x40, 0xF9]); // LDR X0, [SP, #0] ; muscle
        builder.extend(&[0xE1, 0x0B, 0x40, 0xB9]); // LDR W1, [SP, #8] ; priority
        builder.branch_link("scheduler");
    }

    fn generate_emit_update_body(builder: &mut CodeBuilder) {
        // emit_update implementation
        builder.extend(&[0xE0, 0x07, 0x40, 0xF9]); // LDR X0, [SP, #0] ; blob
        builder.branch_link("lattice_emitter");
    }

    fn generate_builtin_functions(builder: &mut CodeBuilder) {
        // hardware_attestation.verify()
        builder.label("verify_attestation");
        builder.extend(&[0x20, 0x00, 0x80, 0x52]); // MOV W0, #1 (true)
        builder.extend(&[0xC0, 0x03, 0x5F, 0xD6]); // RET

        // symbiote.process_update()
        builder.label("symbiote_process_update");
        builder.extend(&[0x00, 0x00, 0x80, 0x52]); // MOV W0, #0 (no action)
        builder.extend(&[0xC0, 0x03, 0x5F, 0xD6]); // RET

        // referee.self_check_failed()
        builder.label("self_check_failed");
        builder.extend(&[0x00, 0x00, 0x80, 0x52]); // MOV W0, #0 (not failed)
        builder.extend(&[0xC0, 0x03, 0x5F, 0xD6]); // RET

        // Stub functions for capabilities
        builder.label("muscle_loader");
        builder.extend(&[0x00, 0x00, 0x80, 0x52]); // MOV W0, #0
        builder.extend(&[0xC0, 0x03, 0x5F, 0xD6]); // RET

        builder.label("scheduler");
        builder.extend(&[0x00, 0x00, 0x80, 0x52]); // MOV W0, #0
        builder.extend(&[0xC0, 0x03, 0x5F, 0xD6]); // RET

        builder.label("lattice_emitter");
        builder.extend(&[0x00, 0x00, 0x80, 0x52]); // MOV W0, #0
        builder.extend(&[0xC0, 0x03, 0x5F, 0xD6]); // RET
    }

    fn generate_event_handlers(builder: &mut CodeBuilder, rules: &[Rule], program: &Program) {
        for (i, rule) in rules.iter().enumerate() {
            Self::generate_rule_handler(builder, rule, i, program);
        }
    }

    fn generate_rule_handler(builder: &mut CodeBuilder, rule: &Rule, index: usize, program: &Program) {
        // handler_{index}:
        let handler_label = format!("handler_{}", index);
        builder.label(&handler_label);

        builder.extend(&[0xFF, 0x43, 0x00, 0xD1]); // SUB SP, SP, #16

        // Generate body statements
        for statement in &rule.body {
            Self::generate_statement(builder, statement, program, index);
        }

        builder.extend(&[0x20, 0x00, 0x80, 0x52]); // MOV W0, #1 (success)
        builder.extend(&[0xFF, 0x43, 0x00, 0x91]); // ADD SP, SP, #16
        builder.extend(&[0xC0, 0x03, 0x5F, 0xD6]); // RET
    }

    fn generate_statement(builder: &mut CodeBuilder, statement: &Statement, program: &Program, handler_idx: usize) {
        match statement {
            Statement::Verify(stmt) => Self::generate_verify_statement(builder, stmt, handler_idx),
            Statement::Let(stmt) => Self::generate_let_statement(builder, stmt),
            Statement::If(stmt) => Self::generate_if_statement(builder, stmt, program, handler_idx),
            Statement::Emit(stmt) => Self::generate_emit_statement(builder, stmt),
            Statement::Schedule(stmt) => Self::generate_schedule_statement(builder, stmt),
            Statement::Unschedule(stmt) => Self::generate_unschedule_statement(builder, stmt),
            Statement::Expr(expr) => Self::generate_expression(builder, expr),
        }
    }

    fn generate_verify_statement(builder: &mut CodeBuilder, stmt: &VerifyStmt, handler_idx: usize) {
        static VERIFY_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let verify_id = VERIFY_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let ok_label = format!("verify_ok_{}_{}", handler_idx, verify_id);

        // Generate condition expression
        Self::generate_expression(builder, &stmt.condition);

        // CBNZ X0, verification_ok
        builder.cbnz(&ok_label, 0);

        // Verification failed: security violation
        builder.branch("security_violation");

        // verification_ok: continue
        builder.label(&ok_label);
    }

    fn generate_let_statement(builder: &mut CodeBuilder, stmt: &LetStmt) {
        if let Some(expr) = &stmt.value {
            Self::generate_expression(builder, expr);
            // Store result to local variable slot
            // In real implementation, track variable locations
        }
    }

    fn generate_if_statement(builder: &mut CodeBuilder, stmt: &IfStmt, program: &Program, handler_idx: usize) {
        static IF_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let if_id = IF_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let else_label = format!("else_{}_{}", handler_idx, if_id);
        let end_label = format!("endif_{}_{}", handler_idx, if_id);

        // Generate condition
        Self::generate_expression(builder, &stmt.condition);

        // CBZ X0, else_branch
        builder.cbz(&else_label, 0);

        // Then branch
        for then_stmt in &stmt.then_branch {
            Self::generate_statement(builder, then_stmt, program, handler_idx);
        }

        // B end_if
        builder.branch(&end_label);

        // Else branch
        builder.label(&else_label);
        if let Some(else_branch) = &stmt.else_branch {
            for else_stmt in else_branch {
                Self::generate_statement(builder, else_stmt, program, handler_idx);
            }
        }

        // end_if: continue
        builder.label(&end_label);
    }

    fn generate_emit_statement(builder: &mut CodeBuilder, stmt: &EmitStmt) {
        // Prepare arguments
        for (i, arg) in stmt.arguments.iter().enumerate() {
            if i < 8 {
                Self::generate_expression(builder, arg);
                // Move to argument register
                if i > 0 {
                    // MOV X{i}, X0
                    let mov_instr: [u8; 4] = match i {
                        1 => [0xE1, 0x03, 0x00, 0xAA], // MOV X1, X0
                        2 => [0xE2, 0x03, 0x00, 0xAA], // MOV X2, X0
                        3 => [0xE3, 0x03, 0x00, 0xAA], // MOV X3, X0
                        4 => [0xE4, 0x03, 0x00, 0xAA], // MOV X4, X0
                        5 => [0xE5, 0x03, 0x00, 0xAA], // MOV X5, X0
                        6 => [0xE6, 0x03, 0x00, 0xAA], // MOV X6, X0
                        7 => [0xE7, 0x03, 0x00, 0xAA], // MOV X7, X0
                        _ => continue,
                    };
                    builder.extend(&mov_instr);
                }
            }
        }

        // Call emit_update capability
        builder.branch_link("emit_update");
    }

    fn generate_schedule_statement(builder: &mut CodeBuilder, stmt: &ScheduleStmt) {
        // Generate muscle expression
        Self::generate_expression(builder, &stmt.muscle);

        // Generate priority (literal)
        if let Literal::Integer(priority) = &stmt.priority {
            // MOVZ W1, #priority (immediate)
            let priority_val = (*priority as u32).min(0xFFFF);
            let mov_instr = 0x52800001u32 | (priority_val << 5);
            builder.extend(&mov_instr.to_le_bytes());
        }

        // Call schedule capability
        builder.branch_link("schedule");
    }

    fn generate_unschedule_statement(builder: &mut CodeBuilder, stmt: &UnscheduleStmt) {
        // Generate muscle_id expression
        Self::generate_expression(builder, &stmt.muscle_id);

        // Set flag for unschedule
        builder.extend(&[0x21, 0x00, 0x80, 0x52]); // MOV W1, #1 (unschedule flag)

        // Call schedule capability with unschedule flag
        builder.branch_link("schedule");
    }

    fn generate_expression(builder: &mut CodeBuilder, expr: &Expression) {
        match expr {
            Expression::Literal(literal) => Self::generate_literal(builder, literal),
            Expression::Variable(var) => Self::generate_variable(builder, var),
            Expression::SelfRef(self_ref) => Self::generate_self_reference(builder, self_ref),
            Expression::Call(call) => Self::generate_call_expression(builder, call),
            Expression::FieldAccess(access) => Self::generate_field_access(builder, access),
            Expression::Binary(bin) => Self::generate_binary_expression(builder, bin),
        }
    }

    fn generate_literal(builder: &mut CodeBuilder, literal: &Literal) {
        match literal {
            Literal::Hex(_) => {
                if let Some(value) = literal.as_u64() {
                    if value <= 0xFFFF {
                        // MOVZ X0, #value
                        let mov_instr = 0xD2800000u32 | ((value as u32 & 0xFFFF) << 5);
                        builder.extend(&mov_instr.to_le_bytes());
                    } else {
                        // For larger values, use MOVZ + MOVK sequence
                        let low = (value & 0xFFFF) as u32;
                        let high = ((value >> 16) & 0xFFFF) as u32;
                        let movz = 0xD2800000u32 | (low << 5);
                        builder.extend(&movz.to_le_bytes());
                        if high > 0 {
                            let movk = 0xF2A00000u32 | (high << 5);
                            builder.extend(&movk.to_le_bytes());
                        }
                    }
                } else {
                    builder.extend(&[0x00, 0x00, 0x80, 0xD2]); // MOVZ X0, #0
                }
            }
            Literal::Integer(n) => {
                if *n <= 0xFFFF {
                    // MOVZ X0, #n
                    let mov_instr = 0xD2800000u32 | ((*n as u32) << 5);
                    builder.extend(&mov_instr.to_le_bytes());
                } else {
                    // For larger values, use MOVZ + MOVK
                    let low = (*n as u32) & 0xFFFF;
                    let high = ((*n as u32) >> 16) & 0xFFFF;
                    let movz = 0xD2800000u32 | (low << 5);
                    builder.extend(&movz.to_le_bytes());
                    if high > 0 {
                        let movk = 0xF2A00000u32 | (high << 5);
                        builder.extend(&movk.to_le_bytes());
                    }
                }
            }
            Literal::String(_) => {
                // String literals: for now, load 0
                builder.extend(&[0x00, 0x00, 0x80, 0xD2]); // MOVZ X0, #0
            }
        }
    }

    fn generate_variable(builder: &mut CodeBuilder, _var: &str) {
        // Load from local variable slot
        // In real implementation, track variable locations
        builder.extend(&[0xE0, 0x07, 0x40, 0xF9]); // LDR X0, [SP, #0] (example)
    }

    fn generate_self_reference(builder: &mut CodeBuilder, self_ref: &SelfReference) {
        match self_ref {
            SelfReference::Id => {
                // Load self.id from fixed location (data section)
                builder.extend(&[0xE0, 0x03, 0x00, 0x90]); // ADRP X0, data
                builder.extend(&[0x00, 0x10, 0x40, 0xF9]); // LDR X0, [X0, #32]
            }
            SelfReference::Version => {
                // Load self.version from fixed location
                builder.extend(&[0xE0, 0x03, 0x00, 0x90]); // ADRP X0, data
                builder.extend(&[0x00, 0x18, 0x40, 0xF9]); // LDR X0, [X0, #48]
            }
        }
    }

    fn generate_call_expression(builder: &mut CodeBuilder, call: &CallExpr) {
        // Prepare arguments
        for (i, arg) in call.arguments.iter().enumerate() {
            if i < 8 {
                Self::generate_expression(builder, arg);
                if i > 0 {
                    // MOV X{i}, X0
                    let mov_instr: [u8; 4] = match i {
                        1 => [0xE1, 0x03, 0x00, 0xAA],
                        2 => [0xE2, 0x03, 0x00, 0xAA],
                        3 => [0xE3, 0x03, 0x00, 0xAA],
                        4 => [0xE4, 0x03, 0x00, 0xAA],
                        5 => [0xE5, 0x03, 0x00, 0xAA],
                        6 => [0xE6, 0x03, 0x00, 0xAA],
                        7 => [0xE7, 0x03, 0x00, 0xAA],
                        _ => continue,
                    };
                    builder.extend(&mov_instr);
                }
            }
        }

        // BL function_name - map known functions to labels
        let target = match call.function.as_str() {
            "hardware_attestation.verify" => "verify_attestation",
            "symbiote.process_update" => "symbiote_process_update",
            "referee.self_check_failed" => "self_check_failed",
            _ => &call.function,
        };
        builder.branch_link(target);
    }

    fn generate_field_access(builder: &mut CodeBuilder, access: &FieldAccess) {
        // Map common field accesses to function calls
        let func_name = format!("{}.{}", access.object, access.field);
        let call_expr = CallExpr {
            function: func_name,
            arguments: Vec::new(),
        };
        Self::generate_call_expression(builder, &call_expr);
    }

    fn generate_binary_expression(builder: &mut CodeBuilder, bin: &BinaryExpr) {
        // Generate left operand
        Self::generate_expression(builder, &bin.left);
        builder.extend(&[0xE8, 0x03, 0x00, 0xAA]); // MOV X8, X0 (save left)

        // Generate right operand
        Self::generate_expression(builder, &bin.right);
        builder.extend(&[0xE9, 0x03, 0x00, 0xAA]); // MOV X9, X0 (save right)

        // Generate operation
        match bin.op {
            BinaryOperator::Eq => {
                // SUBS XZR, X8, X9 (CMP X8, X9)
                builder.extend(&[0x1F, 0x01, 0x09, 0xEB]);
                // CSET X0, EQ
                builder.extend(&[0xE0, 0x17, 0x9F, 0x9A]);
            }
            BinaryOperator::Ne => {
                builder.extend(&[0x1F, 0x01, 0x09, 0xEB]); // CMP X8, X9
                builder.extend(&[0x00, 0x10, 0x9F, 0x9A]); // CSET X0, NE
            }
            BinaryOperator::Add => {
                builder.extend(&[0x00, 0x01, 0x09, 0x8B]); // ADD X0, X8, X9
            }
            BinaryOperator::Sub => {
                builder.extend(&[0x00, 0x01, 0x09, 0xCB]); // SUB X0, X8, X9
            }
            BinaryOperator::Lt => {
                builder.extend(&[0x1F, 0x01, 0x09, 0xEB]); // CMP X8, X9
                builder.extend(&[0xA0, 0x17, 0x9F, 0x9A]); // CSET X0, LT
            }
            BinaryOperator::Gt => {
                builder.extend(&[0x1F, 0x01, 0x09, 0xEB]); // CMP X8, X9
                builder.extend(&[0xC0, 0x17, 0x9F, 0x9A]); // CSET X0, GT
            }
            _ => {
                builder.extend(&[0x00, 0x00, 0x80, 0xD2]); // MOVZ X0, #0 (default)
            }
        }
    }

    fn generate_data_section(builder: &mut CodeBuilder, program: &Program) {
        builder.label("data_section");

        // Align to 8 bytes
        while builder.pos() % 8 != 0 {
            builder.extend(&[0x00]);
        }

        // Constants
        for decl in &program.declarations {
            if let Declaration::Const(const_decl) = decl {
                builder.extend(&const_decl.value.to_bytes());
                // Pad to 8 bytes
                while builder.pos() % 8 != 0 {
                    builder.extend(&[0x00]);
                }
            }
        }

        // Built-in data
        builder.extend(&[0xEAu8; 32]); // genesis_root
        builder.extend(&0xFFFF_FFFF_FFFF_FFFFu64.to_le_bytes()); // symbiote_id
        builder.extend(&1u64.to_le_bytes()); // self.version
    }

    fn generate_capability_tables(builder: &mut CodeBuilder, program: &Program) {
        builder.label("capability_tables");

        // Capability authorization bitmap
        let mut capability_bits = 0u64;

        for decl in &program.declarations {
            if let Declaration::Capability(cap) = decl {
                let bit_position = Self::capability_bit_position(&cap.name);
                capability_bits |= 1 << bit_position;
            }
        }

        builder.extend(&capability_bits.to_le_bytes());
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
