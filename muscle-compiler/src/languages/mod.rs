//! Language frontends for Muscle Compiler
//! Now includes full Wizard Stack specification support
//! 
//! This module provides the complete Muscle.ea language implementation
//! according to the Wizard Stack specification v1.0

pub mod formal_grammar;
pub mod capability_checker;

// Re-export the main components for easy access
pub use formal_grammar::FormalParser;
pub use capability_checker::{CapabilityChecker, verify_sacred_rules};

/// Language detection and dispatch for different source file types
pub struct LanguageFrontend;

impl LanguageFrontend {
    /// Detect and parse source code based on file extension and content
    pub fn parse_source(source: &str, file_extension: &str) -> Result<crate::ast::full_ast::Program, crate::error::CompileError> {
        match file_extension {
            "ea" => {
                // Parse as Muscle.ea language with full specification
                FormalParser::parse_program(source)
            }
            "py" => {
                // Python files are handled by the existing PythonParser
                // This converts Python AST to the unified Muscle AST
                Self::convert_python_to_muscle_ast(source)
            }
            _ => {
                Err(crate::error::CompileError::SyntaxError(
                    format!("Unsupported file extension: .{}", file_extension)
                ))
            }
        }
    }

    /// Convert Python neural network definition to Muscle AST
    fn convert_python_to_muscle_ast(source: &str) -> Result<crate::ast::full_ast::Program, crate::error::CompileError> {
        use crate::parser::PythonParser;
        use crate::ast::full_ast::*;
        
        let python_ast = PythonParser::parse(source)?;
        
        // Convert Python NN AST to Muscle.ea program structure
        let mut declarations = Vec::new();
        let mut rules = Vec::new();
        
        // Add input declaration for neural network data
        declarations.push(Declaration::Input(InputDecl {
            name: "neural_input".to_string(),
            data_type: Type::ByteArray32,
        }));
        
        // Add capability for inference
        declarations.push(Declaration::Capability(CapabilityDecl {
            name: "perform_inference".to_string(),
            parameters: vec![Parameter {
                name: "input_data".to_string(),
                param_type: Type::ByteArray32,
            }],
            return_type: Some(Type::ByteArray32),
        }));
        
        // Create inference rule
        rules.push(Rule {
            event: Event::Custom("on_inference_request".to_string()),
            body: vec![
                Statement::Expr(Expression::Call(CallExpr {
                    function: "perform_inference".to_string(),
                    arguments: vec![Expression::Variable("neural_input".to_string())],
                })),
            ],
        });
        
        // Add metadata about the neural network
        declarations.push(Declaration::Metadata(MetadataDecl {
            name: "network_type".to_string(),
            value: "neural_network".to_string(),
        }));
        
        if let Some(layers) = python_ast.metadata.get("layers") {
            declarations.push(Declaration::Metadata(MetadataDecl {
                name: "architecture".to_string(),
                value: layers.clone(),
            }));
        }
        
        Ok(Program { declarations, rules })
    }

    /// Validate that a program meets all language-specific constraints
    pub fn validate_program(program: &crate::ast::full_ast::Program, file_extension: &str) -> Result<(), crate::error::CompileError> {
        match file_extension {
            "ea" => {
                // For Muscle.ea files, enforce full specification
                let mut checker = CapabilityChecker::new();
                checker.verify_program(program)?;
                verify_sacred_rules(program)?;
                Ok(())
            }
            "py" => {
                // Python files have different validation rules
                // Mainly check that it's a valid neural network
                Self::validate_neural_network(program)
            }
            _ => Ok(()) // Other types may have no specific validation
        }
    }

    /// Validate that a Python-derived program represents a valid neural network
    fn validate_neural_network(program: &crate::ast::full_ast::Program) -> Result<(), crate::error::CompileError> {
        // Check that it has the required neural network structure
        let has_inference_capability = program.declarations.iter().any(|decl| {
            if let Declaration::Capability(cap) = decl {
                cap.name == "perform_inference"
            } else {
                false
            }
        });

        if !has_inference_capability {
            return Err(crate::error::CompileError::CompileError(
                "Neural network must have 'perform_inference' capability".to_string()
            ));
        }

        let has_inference_rule = program.rules.iter().any(|rule| {
            matches!(&rule.event, Event::Custom(name) if name == "on_inference_request")
        });

        if !has_inference_rule {
            return Err(crate::error::CompileError::CompileError(
                "Neural network must have 'on_inference_request' rule".to_string()
            ));
        }

        Ok(())
    }

    /// Get the target code generator for a specific file type
    pub fn get_code_generator(file_extension: &str) -> Result<Box<dyn CodeGenerator>, crate::error::CompileError> {
        match file_extension {
            "ea" => Ok(Box::new(crate::codegen::nucleus::NucleusCodegen)),
            "py" => {
                // Python files use architecture-specific generators
                // These are selected based on target flag in main.rs
                Err(crate::error::CompileError::CompileError(
                    "Python generators are architecture-specific".to_string()
                ))
            }
            _ => Err(crate::error::CompileError::CompileError(
                format!("No code generator for file extension: .{}", file_extension)
            ))
        }
    }
}

/// Trait for code generators that can produce machine code from Muscle AST
pub trait CodeGenerator {
    fn generate(&self, program: &crate::ast::full_ast::Program) -> Result<Vec<u8>, crate::error::CompileError>;
}

// Implement CodeGenerator for NucleusCodegen
impl CodeGenerator for crate::codegen::nucleus::NucleusCodegen {
    fn generate(&self, program: &crate::ast::full_ast::Program) -> Result<Vec<u8>, crate::error::CompileError> {
        crate::codegen::nucleus::NucleusCodegen::generate(program)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_detection() {
        let ea_source = r#"
input lattice_stream<MuscleUpdate>
capability emit_update(blob: SealedBlob)

rule on_boot:
    emit heartbeat("test")
"#;

        let result = LanguageFrontend::parse_source(ea_source, "ea");
        assert!(result.is_ok());
        
        let program = result.unwrap();
        assert_eq!(program.declarations.len(), 2);
        assert_eq!(program.rules.len(), 1);
    }

    #[test]
    fn test_ea_validation() {
        let valid_ea_source = r#"
input lattice_stream<MuscleUpdate>
capability emit_update(blob: SealedBlob)

rule on_boot:
    emit heartbeat("valid")
"#;

        let program = FormalParser::parse_program(valid_ea_source).unwrap();
        let result = LanguageFrontend::validate_program(&program, "ea");
        assert!(result.is_ok());
    }

    #[test]
    fn test_ea_validation_failure() {
        let invalid_ea_source = r#"
input lattice_stream<MuscleUpdate>
# Missing capability declaration

rule on_boot:
    emit heartbeat("invalid")  # Uses undeclared capability
"#;

        let program = FormalParser::parse_program(invalid_ea_source).unwrap();
        let result = LanguageFrontend::validate_program(&program, "ea");
        assert!(result.is_err());
    }

    #[test]
    fn test_code_generator_selection() {
        let generator = LanguageFrontend::get_code_generator("ea");
        assert!(generator.is_ok());
        
        let invalid_generator = LanguageFrontend::get_code_generator("unknown");
        assert!(invalid_generator.is_err());
    }
}
