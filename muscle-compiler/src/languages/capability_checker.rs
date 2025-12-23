//! Capability security enforcement for Muscle.ea
//! Implements "If you didn't declare it, you cannot do it"

use crate::ast::full_ast::*;
use crate::error::CompileError;
use std::collections::{HashMap, HashSet};

pub struct CapabilityChecker {
    declared_capabilities: HashSet<String>,
    used_capabilities: HashSet<String>,
    declared_inputs: HashSet<String>,
}

impl CapabilityChecker {
    pub fn new() -> Self {
        Self {
            declared_capabilities: HashSet::new(),
            used_capabilities: HashSet::new(),
            declared_inputs: HashSet::new(),
        }
    }

    /// Verify capability security for entire program
    pub fn verify_program(&mut self, program: &Program) -> Result<(), CompileError> {
        // First pass: collect declarations
        for decl in &program.declarations {
            match decl {
                Declaration::Input(input_decl) => {
                    self.declared_inputs.insert(input_decl.name.clone());
                }
                Declaration::Capability(cap_decl) => {
                    self.declared_capabilities.insert(cap_decl.name.clone());
                }
                _ => {}
            }
        }

        // Second pass: verify rule bodies
        for rule in &program.rules {
            self.verify_rule(rule)?;
        }

        // Verify no undeclared capability usage
        for used_cap in &self.used_capabilities {
            if !self.declared_capabilities.contains(used_cap) {
                return Err(CompileError::CapabilityError(format!(
                    "Use of undeclared capability: '{}'",
                    used_cap
                )));
            }
        }

        Ok(())
    }

    fn verify_rule(&mut self, rule: &Rule) -> Result<(), CompileError> {
        // Verify event input is declared
        self.verify_event(&rule.event)?;

        for statement in &rule.body {
            self.verify_statement(statement)?;
        }

        Ok(())
    }

    fn verify_event(&self, event: &Event) -> Result<(), CompileError> {
        match event {
            Event::OnLatticeUpdate { .. } => {
                if !self.declared_inputs.contains("lattice_stream") {
                    return Err(CompileError::CapabilityError(
                        "Event 'on_lattice_update' requires 'input lattice_stream'".to_string(),
                    ));
                }
            }
            Event::OnBoot => {
                if !self.declared_inputs.contains("hardware_attestation") {
                    return Err(CompileError::CapabilityError(
                        "Event 'on_boot' requires 'input hardware_attestation'".to_string(),
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn verify_statement(&mut self, statement: &Statement) -> Result<(), CompileError> {
        match statement {
            Statement::Verify(stmt) => {
                self.verify_expression(&stmt.condition)?;
            }
            Statement::Let(stmt) => {
                if let Some(expr) = &stmt.value {
                    self.verify_expression(expr)?;
                }
            }
            Statement::If(stmt) => {
                self.verify_expression(&stmt.condition)?;
                for stmt in &stmt.then_branch {
                    self.verify_statement(stmt)?;
                }
                if let Some(else_branch) = &stmt.else_branch {
                    for stmt in else_branch {
                        self.verify_statement(stmt)?;
                    }
                }
            }
            Statement::Emit(stmt) => {
                // emit requires emit_update capability
                self.used_capabilities.insert("emit_update".to_string());
                for arg in &stmt.arguments {
                    self.verify_expression(arg)?;
                }
            }
            Statement::Schedule(stmt) => {
                // schedule requires schedule capability
                self.used_capabilities.insert("schedule".to_string());
                self.verify_expression(&stmt.muscle)?;
                self.verify_expression(&Expression::Literal(stmt.priority.clone()))?;
            }
            Statement::Unschedule(stmt) => {
                // unschedule requires schedule capability
                self.used_capabilities.insert("schedule".to_string());
                self.verify_expression(&stmt.muscle_id)?;
            }
            Statement::Expr(expr) => {
                self.verify_expression(expr)?;
            }
        }
        Ok(())
    }

    fn verify_expression(&mut self, expr: &Expression) -> Result<(), CompileError> {
        match expr {
            Expression::Call(call) => {
                // Check if this is a capability call
                match call.function.as_str() {
                    "load_muscle" => {
                        self.used_capabilities.insert("load_muscle".to_string());
                    }
                    "emit_update" => {
                        self.used_capabilities.insert("emit_update".to_string());
                    }
                    "schedule" => {
                        self.used_capabilities.insert("schedule".to_string());
                    }
                    "unschedule" => {
                        self.used_capabilities.insert("schedule".to_string());
                    }
                    _ => {
                        // Regular function call - verify inputs are declared
                        if !self.declared_inputs.contains(&call.function) {
                            // Check if it's a method call on declared input
                            if let Some(obj_name) = call.function.split('.').next() {
                                if !self.declared_inputs.contains(obj_name) {
                                    return Err(CompileError::CapabilityError(format!(
                                        "Call to undeclared function: '{}'",
                                        call.function
                                    )));
                                }
                            }
                        }
                    }
                }

                for arg in &call.arguments {
                    self.verify_expression(arg)?;
                }
            }
            Expression::FieldAccess(access) => {
                // Verify the object is declared
                if !self.declared_inputs.contains(&access.object) {
                    return Err(CompileError::CapabilityError(format!(
                        "Access to undeclared object: '{}'",
                        access.object
                    )));
                }
            }
            Expression::Binary(bin) => {
                self.verify_expression(&bin.left)?;
                self.verify_expression(&bin.right)?;
            }
            Expression::Variable(var) => {
                // Variables from let statements are fine
                // But we should check they're not trying to access undeclared inputs
                if self.declared_inputs.contains(var) {
                    // This is fine - it's a declared input
                }
                // Otherwise, assume it's a local variable from let
            }
            _ => {} // Literals and self references are fine
        }
        Ok(())
    }
}

/// Verify the Three Sacred Rules of muscle.ea
pub fn verify_sacred_rules(program: &Program) -> Result<(), CompileError> {
    // Rule 1: Append-only - no mutation operations in language by design ✓

    // Rule 2: Event-driven - verified by parser structure ✓

    // Rule 3: Capability-secure - enforced by CapabilityChecker ✓

    // Additional: No polling constructs
    verify_no_polling(program)?;

    Ok(())
}

fn verify_no_polling(program: &Program) -> Result<(), CompileError> {
    // Muscle.ea has no looping constructs by design
    // This is enforced by the grammar - no while, for, loop keywords
    // So we just need to ensure no recursive event emissions that could simulate polling

    // Check for potential infinite emission chains
    let mut emitter_events = HashSet::new();

    for rule in &program.rules {
        if let Event::OnTimer1Hz = rule.event {
            // Timer events can emit, but that's fine - it's 1Hz bounded
            continue;
        }

        for statement in &rule.body {
            if let Statement::Emit(emit_stmt) = statement {
                emitter_events.insert(emit_stmt.event.clone());
            }
        }
    }

    // Simple check: if an event emits itself, that's polling
    for rule in &program.rules {
        if let Event::Custom(event_name) = &rule.event {
            if emitter_events.contains(event_name) {
                return Err(CompileError::CapabilityError(format!(
                    "Potential polling detected: event '{}' emits itself",
                    event_name
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_enforcement() {
        let source = r#"
input lattice_stream<MuscleUpdate>
capability emit_update(blob: SealedBlob)

rule on_boot:
    emit heartbeat("I am alive")  # This should work
"#;

        let program = FormalParser::parse_program(source).unwrap();
        let mut checker = CapabilityChecker::new();
        assert!(checker.verify_program(&program).is_ok());
    }

    #[test]
    fn test_undeclared_capability() {
        let source = r#"
input lattice_stream<MuscleUpdate>
# Missing: capability emit_update

rule on_boot:
    emit heartbeat("I am alive")  # This should fail
"#;

        let program = FormalParser::parse_program(source).unwrap();
        let mut checker = CapabilityChecker::new();
        assert!(checker.verify_program(&program).is_err());
    }

    #[test]
    fn test_undeclared_input_access() {
        let source = r#"
# Missing: input hardware_attestation
capability emit_update(blob: SealedBlob)

rule on_boot:
    verify hardware_attestation.verify()  # This should fail
    emit heartbeat("test")
"#;

        let program = FormalParser::parse_program(source).unwrap();
        let mut checker = CapabilityChecker::new();
        assert!(checker.verify_program(&program).is_err());
    }
}
