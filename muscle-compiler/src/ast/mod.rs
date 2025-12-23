//! Abstract Syntax Tree representations for Muscle Compiler
//!
//! This module provides complete AST definitions for:
//! - Full Muscle.ea language specification (Wizard Stack)
//! - Python neural network definitions
//! - Unified intermediate representation

pub mod full_ast;

// Re-export all AST types for easy access
pub use full_ast::{
    BinaryExpr, BinaryOperator, CallExpr, CapabilityDecl, ConstDecl, Declaration, EmitStmt, Event,
    Expression, FieldAccess, IfStmt, InputDecl, LetStmt, Literal, MetadataDecl, Parameter, Program,
    Rule, ScheduleStmt, SelfReference, Statement, Type, UnscheduleStmt, VerifyStmt,
};

/// Unified AST representation that can handle both Muscle.ea and Python sources
#[derive(Debug, Clone)]
pub struct MuscleAst {
    /// The source language (ea, python, etc.)
    pub language: String,
    /// All declarations in the program
    pub declarations: Vec<Declaration>,
    /// All rules in the program  
    pub rules: Vec<Rule>,
    /// Language-specific metadata
    pub metadata: std::collections::HashMap<String, String>,
    /// Source code location information for error reporting
    pub source_locations: std::collections::HashMap<String, (usize, usize)>,
}

impl MuscleAst {
    /// Create a new empty Muscle AST
    pub fn new(language: String) -> Self {
        Self {
            language,
            declarations: Vec::new(),
            rules: Vec::new(),
            metadata: std::collections::HashMap::new(),
            source_locations: std::collections::HashMap::new(),
        }
    }

    /// Convert from full Muscle.ea program AST
    pub fn from_ea_program(program: Program) -> Self {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("specification".to_string(), "muscle.ea_v1.0".to_string());

        // Extract metadata declarations
        for decl in &program.declarations {
            if let Declaration::Metadata(meta) = decl {
                metadata.insert(meta.name.clone(), meta.value.clone());
            }
        }

        Self {
            language: "ea".to_string(),
            declarations: program.declarations,
            rules: program.rules,
            metadata,
            source_locations: std::collections::HashMap::new(),
        }
    }

    /// Convert to full Muscle.ea program AST
    pub fn to_ea_program(&self) -> Result<Program, crate::error::CompileError> {
        if self.language != "ea" {
            return Err(crate::error::CompileError::CompileError(format!(
                "Cannot convert {} AST to Muscle.ea program",
                self.language
            )));
        }

        Ok(Program {
            declarations: self.declarations.clone(),
            rules: self.rules.clone(),
        })
    }

    /// Add a declaration to the AST
    pub fn add_declaration(&mut self, declaration: Declaration) {
        self.declarations.push(declaration);
    }

    /// Add a rule to the AST
    pub fn add_rule(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// Set metadata value
    pub fn set_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }

    /// Check if the AST has a specific capability declared
    pub fn has_capability(&self, capability_name: &str) -> bool {
        self.declarations.iter().any(|decl| {
            if let Declaration::Capability(cap) = decl {
                cap.name == capability_name
            } else {
                false
            }
        })
    }

    /// Check if the AST has a specific input declared
    pub fn has_input(&self, input_name: &str) -> bool {
        self.declarations.iter().any(|decl| {
            if let Declaration::Input(input) = decl {
                input.name == input_name
            } else {
                false
            }
        })
    }

    /// Get all capability declarations
    pub fn get_capabilities(&self) -> Vec<&CapabilityDecl> {
        self.declarations
            .iter()
            .filter_map(|decl| {
                if let Declaration::Capability(cap) = decl {
                    Some(cap)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all input declarations
    pub fn get_inputs(&self) -> Vec<&InputDecl> {
        self.declarations
            .iter()
            .filter_map(|decl| {
                if let Declaration::Input(input) = decl {
                    Some(input)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all constant declarations
    pub fn get_constants(&self) -> Vec<&ConstDecl> {
        self.declarations
            .iter()
            .filter_map(|decl| {
                if let Declaration::Const(const_decl) = decl {
                    Some(const_decl)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Find a rule by event name
    pub fn find_rule(&self, event_name: &str) -> Option<&Rule> {
        self.rules.iter().find(|rule| match &rule.event {
            Event::OnBoot => event_name == "on_boot",
            Event::OnTimer1Hz => event_name == "on_timer_1hz",
            Event::OnSelfIntegrityFailure => event_name == "on_self_integrity_failure",
            Event::OnLatticeUpdate { .. } => event_name == "on_lattice_update",
            Event::Custom(name) => name == event_name,
        })
    }

    /// Validate basic AST structure
    pub fn validate_basic_structure(&self) -> Result<(), crate::error::CompileError> {
        // Check for duplicate declarations
        let mut declared_names = std::collections::HashSet::new();

        for decl in &self.declarations {
            let name = match decl {
                Declaration::Input(input) => &input.name,
                Declaration::Capability(cap) => &cap.name,
                Declaration::Const(const_decl) => &const_decl.name,
                Declaration::Metadata(meta) => &meta.name,
            };

            if declared_names.contains(name) {
                return Err(crate::error::CompileError::CompileError(format!(
                    "Duplicate declaration: '{}'",
                    name
                )));
            }
            declared_names.insert(name.clone());
        }

        // Check for at least one rule
        if self.rules.is_empty() {
            return Err(crate::error::CompileError::CompileError(
                "Program must have at least one rule".to_string(),
            ));
        }

        Ok(())
    }

    /// Get summary information about the AST
    pub fn get_summary(&self) -> AstSummary {
        AstSummary {
            language: self.language.clone(),
            declaration_count: self.declarations.len(),
            rule_count: self.rules.len(),
            input_count: self.get_inputs().len(),
            capability_count: self.get_capabilities().len(),
            constant_count: self.get_constants().len(),
        }
    }
}

/// Summary information about an AST
#[derive(Debug, Clone)]
pub struct AstSummary {
    pub language: String,
    pub declaration_count: usize,
    pub rule_count: usize,
    pub input_count: usize,
    pub capability_count: usize,
    pub constant_count: usize,
}

impl std::fmt::Display for AstSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} program: {} declarations ({} inputs, {} capabilities, {} constants), {} rules",
            self.language,
            self.declaration_count,
            self.input_count,
            self.capability_count,
            self.constant_count,
            self.rule_count
        )
    }
}

// Implement conversion from full_ast::Program to MuscleAst
impl From<Program> for MuscleAst {
    fn from(program: Program) -> Self {
        MuscleAst::from_ea_program(program)
    }
}

// Implement conversion to full_ast::Program for MuscleAst
impl TryFrom<MuscleAst> for Program {
    type Error = crate::error::CompileError;

    fn try_from(ast: MuscleAst) -> Result<Self, Self::Error> {
        ast.to_ea_program()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_muscle_ast_creation() {
        let mut ast = MuscleAst::new("ea".to_string());

        // Add a declaration
        ast.add_declaration(Declaration::Input(InputDecl {
            name: "test_input".to_string(),
            data_type: Type::MuscleUpdate,
        }));

        // Add a rule
        ast.add_rule(Rule {
            event: Event::OnBoot,
            body: vec![],
        });

        // Set metadata
        ast.set_metadata("version".to_string(), "1.0.0".to_string());

        assert_eq!(ast.declarations.len(), 1);
        assert_eq!(ast.rules.len(), 1);
        assert_eq!(ast.get_metadata("version"), Some(&"1.0.0".to_string()));
    }

    #[test]
    fn test_ast_validation() {
        let mut ast = MuscleAst::new("ea".to_string());

        // Add a declaration
        ast.add_declaration(Declaration::Input(InputDecl {
            name: "test_input".to_string(),
            data_type: Type::MuscleUpdate,
        }));

        // Add a rule
        ast.add_rule(Rule {
            event: Event::OnBoot,
            body: vec![],
        });

        let result = ast.validate_basic_structure();
        assert!(result.is_ok());
    }

    #[test]
    fn test_ast_validation_duplicate() {
        let mut ast = MuscleAst::new("ea".to_string());

        // Add duplicate declarations
        ast.add_declaration(Declaration::Input(InputDecl {
            name: "test".to_string(),
            data_type: Type::MuscleUpdate,
        }));

        ast.add_declaration(Declaration::Input(InputDecl {
            name: "test".to_string(), // Same name!
            data_type: Type::DeviceProof,
        }));

        ast.add_rule(Rule {
            event: Event::OnBoot,
            body: vec![],
        });

        let result = ast.validate_basic_structure();
        assert!(result.is_err());
    }

    #[test]
    fn test_ast_validation_no_rules() {
        let mut ast = MuscleAst::new("ea".to_string());

        // Add declaration but no rules
        ast.add_declaration(Declaration::Input(InputDecl {
            name: "test_input".to_string(),
            data_type: Type::MuscleUpdate,
        }));

        let result = ast.validate_basic_structure();
        assert!(result.is_err());
    }

    #[test]
    fn test_ast_summary() {
        let mut ast = MuscleAst::new("ea".to_string());

        ast.add_declaration(Declaration::Input(InputDecl {
            name: "input1".to_string(),
            data_type: Type::MuscleUpdate,
        }));

        ast.add_declaration(Declaration::Capability(CapabilityDecl {
            name: "cap1".to_string(),
            parameters: vec![],
            return_type: None,
        }));

        ast.add_declaration(Declaration::Const(ConstDecl {
            name: "const1".to_string(),
            const_type: Type::U64,
            value: Literal::Integer(42),
        }));

        ast.add_rule(Rule {
            event: Event::OnBoot,
            body: vec![],
        });

        ast.add_rule(Rule {
            event: Event::OnTimer1Hz,
            body: vec![],
        });

        let summary = ast.get_summary();
        assert_eq!(summary.language, "ea");
        assert_eq!(summary.declaration_count, 3);
        assert_eq!(summary.rule_count, 2);
        assert_eq!(summary.input_count, 1);
        assert_eq!(summary.capability_count, 1);
        assert_eq!(summary.constant_count, 1);

        // Test display
        let summary_str = format!("{}", summary);
        assert!(summary_str.contains("ea program"));
    }

    #[test]
    fn test_find_rule() {
        let mut ast = MuscleAst::new("ea".to_string());

        ast.add_rule(Rule {
            event: Event::OnBoot,
            body: vec![],
        });

        ast.add_rule(Rule {
            event: Event::Custom("custom_event".to_string()),
            body: vec![],
        });

        assert!(ast.find_rule("on_boot").is_some());
        assert!(ast.find_rule("custom_event").is_some());
        assert!(ast.find_rule("nonexistent").is_none());
    }

    #[test]
    fn test_capability_check() {
        let mut ast = MuscleAst::new("ea".to_string());

        ast.add_declaration(Declaration::Capability(CapabilityDecl {
            name: "test_cap".to_string(),
            parameters: vec![],
            return_type: None,
        }));

        assert!(ast.has_capability("test_cap"));
        assert!(!ast.has_capability("other_cap"));
    }
}
