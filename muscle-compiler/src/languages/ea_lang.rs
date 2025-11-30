//! Parser for .ea language - enables Nucleus as Muscle source code

use crate::ast::{MuscleAst, InputDeclaration, Capability, Rule};
use crate::error::CompileError;
use std::collections::HashMap;

/// Parser for .ea language used by Nucleus and core muscles
pub struct EaLanguage;

impl EaLanguage {
    /// Parse .ea source code into Muscle AST
    pub fn parse(source: &str) -> Result<MuscleAst, CompileError> {
        let mut lines = source.lines().peekable();
        let mut inputs = Vec::new();
        let mut capabilities = Vec::new();
        let mut rules = Vec::new();
        let mut current_rule = None;

        while let Some(line) = lines.next() {
            let line = line.trim();
            
            if line.starts_with("input ") {
                inputs.push(Self::parse_input(line)?);
            } else if line.starts_with("capability ") {
                capabilities.push(Self::parse_capability(line)?);
            } else if line.starts_with("rule ") {
                if let Some(rule) = current_rule.take() {
                    rules.push(rule);
                }
                current_rule = Some(Self::parse_rule_header(line)?);
            } else if line.starts_with("    ") && current_rule.is_some() {
                // Rule body line
                Self::parse_rule_body(line, current_rule.as_mut().unwrap())?;
            } else if line.is_empty() {
                // Blank line, skip
            } else {
                return Err(CompileError::SyntaxError(format!("Unexpected line: {}", line)));
            }
        }

        // Don't forget the last rule
        if let Some(rule) = current_rule.take() {
            rules.push(rule);
        }

        Ok(MuscleAst {
            language: "ea".to_string(),
            inputs,
            capabilities,
            rules,
            // Nucleus-specific metadata
            metadata: HashMap::from([
                ("type".to_string(), "nucleus".to_string()),
                ("size".to_string(), "8192".to_string()),
            ]),
        })
    }

    fn parse_input(line: &str) -> Result<InputDeclaration, CompileError> {
        // input lattice_stream<MuscleUpdate>
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(CompileError::SyntaxError("Invalid input declaration".to_string()));
        }
        
        let type_part = parts[1];
        let (name, data_type) = if let Some(pos) = type_part.find('<') {
            let name = &type_part[..pos];
            let data_type = &type_part[pos+1..type_part.len()-1];
            (name, data_type)
        } else {
            return Err(CompileError::SyntaxError("Input missing type parameter".to_string()));
        };

        Ok(InputDeclaration {
            name: name.to_string(),
            data_type: data_type.to_string(),
        })
    }

    fn parse_capability(line: &str) -> Result<Capability, CompileError> {
        // capability load_muscle(id)
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(CompileError::SyntaxError("Invalid capability declaration".to_string()));
        }

        let name = parts[1];
        let params = if let Some(pos) = name.find('(') {
            let actual_name = &name[..pos];
            let params_str = &name[pos+1..name.len()-1];
            (actual_name, params_str.split(',').map(|s| s.trim().to_string()).collect())
        } else {
            (name, Vec::new())
        };

        Ok(Capability {
            name: params.0.to_string(),
            parameters: params.1,
        })
    }

    fn parse_rule_header(line: &str) -> Result<Rule, CompileError> {
        // rule on_boot:
        // rule on_lattice_update(update):
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(CompileError::SyntaxError("Invalid rule declaration".to_string()));
        }

        let name = parts[1].trim_end_matches(':');
        let parameters = if let Some(pos) = name.find('(') {
            let actual_name = &name[..pos];
            let params_str = &name[pos+1..name.len()-1];
            (actual_name, params_str.split(',').map(|s| s.trim().to_string()).collect())
        } else {
            (name, Vec::new())
        };

        Ok(Rule {
            name: parameters.0.to_string(),
            parameters: parameters.1,
            body: Vec::new(),
        })
    }

    fn parse_rule_body(line: &str, rule: &mut Rule) -> Result<(), CompileError> {
        let statement = line.trim().to_string();
        if !statement.is_empty() {
            rule.body.push(statement);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nucleus_source() {
        let source = r#"input lattice_stream<MuscleUpdate>
input hardware_attestation<DeviceProof>
input symbiote<SealedBlob>

capability load_muscle(id)
capability schedule(id, priority)
capability emit_update(blob)

rule on_boot:
    verify hardware_attestation
    verify lattice_root == 0xEA...genesis
    load_muscle(symbiote_id) -> symbiote
    schedule(symbiote, 255)

rule on_lattice_update(update):
    if symbiote.process(update) -> healing:
        emit_update(healing.blob)

rule on_timer_1hz:
    emit heartbeat(self.id, self.version)"#;

        let ast = EaLanguage::parse(source).unwrap();
        assert_eq!(ast.inputs.len(), 3);
        assert_eq!(ast.capabilities.len(), 3);
        assert_eq!(ast.rules.len(), 3);
        
        let boot_rule = &ast.rules[0];
        assert_eq!(boot_rule.name, "on_boot");
        assert_eq!(boot_rule.body.len(), 4);
    }
}
