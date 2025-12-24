//! Complete AST for Muscle.ea language specification v1.0
//! Represents the full Wizard Stack grammar

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub declarations: Vec<Declaration>,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Declaration {
    Input(InputDecl),
    Capability(CapabilityDecl),
    Const(ConstDecl),
    Metadata(MetadataDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputDecl {
    pub name: String,
    pub data_type: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityDecl {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstDecl {
    pub name: String,
    pub const_type: Type,
    pub value: Literal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetadataDecl {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub param_type: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub event: Event,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    OnBoot,
    OnLatticeUpdate {
        param_name: String,
        param_type: Type,
    },
    OnTimer1Hz,
    OnSelfIntegrityFailure,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Verify(VerifyStmt),
    Let(LetStmt),
    If(IfStmt),
    Emit(EmitStmt),
    Schedule(ScheduleStmt),
    Unschedule(UnscheduleStmt),
    Expr(Expression),
}

#[derive(Debug, Clone, PartialEq)]
pub struct VerifyStmt {
    pub condition: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LetStmt {
    pub name: String,
    pub value: Option<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IfStmt {
    pub condition: Expression,
    /// Optional binding variable for pattern-matching syntax: `if condition -> binding:`
    pub binding: Option<String>,
    pub then_branch: Vec<Statement>,
    pub else_branch: Option<Vec<Statement>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmitStmt {
    pub event: String,
    pub arguments: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScheduleStmt {
    pub muscle: Expression,
    pub priority: Literal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnscheduleStmt {
    pub muscle_id: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Literal(Literal),
    Variable(String),
    SelfRef(SelfReference),
    Call(CallExpr),
    FieldAccess(FieldAccess),
    Binary(BinaryExpr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Hex(String),
    Integer(u64),
    String(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SelfReference {
    Id,
    Version,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallExpr {
    pub function: String,
    pub arguments: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldAccess {
    pub object: String,
    pub field: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BinaryExpr {
    pub left: Box<Expression>,
    pub op: BinaryOperator,
    pub right: Box<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOperator {
    Eq,  // ==
    Ne,  // !=
    Lt,  // <
    Gt,  // >
    Le,  // <=
    Ge,  // >=
    Add, // +
    Sub, // -
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    MuscleUpdate,
    DeviceProof,
    SealedBlob,
    ExecutableMuscle,
    MuscleId,
    U8,
    U64,
    ByteArray32,
}

// Built-in constants and functions
#[derive(Debug, Clone, PartialEq)]
pub enum BuiltIn {
    LatticeRoot,
    RefereeSelfCheckFailed,
    SymbioteProcessUpdate,
}

impl Program {
    pub fn find_capability(&self, name: &str) -> Option<&CapabilityDecl> {
        for decl in &self.declarations {
            if let Declaration::Capability(cap) = decl {
                if cap.name == name {
                    return Some(cap);
                }
            }
        }
        None
    }

    pub fn find_input(&self, name: &str) -> Option<&InputDecl> {
        for decl in &self.declarations {
            if let Declaration::Input(input) = decl {
                if input.name == name {
                    return Some(input);
                }
            }
        }
        None
    }

    pub fn find_const(&self, name: &str) -> Option<&ConstDecl> {
        for decl in &self.declarations {
            if let Declaration::Const(const_decl) = decl {
                if const_decl.name == name {
                    return Some(const_decl);
                }
            }
        }
        None
    }
}

impl Literal {
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Literal::Hex(hex_str) => {
                let hex_digits = hex_str.trim_start_matches("0x");
                hex::decode(hex_digits).unwrap_or_default()
            }
            Literal::Integer(n) => n.to_le_bytes().to_vec(),
            Literal::String(s) => s.as_bytes().to_vec(),
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Literal::Integer(n) => Some(*n),
            Literal::Hex(hex_str) => u64::from_str_radix(hex_str.trim_start_matches("0x"), 16).ok(),
            _ => None,
        }
    }
}

impl Type {
    pub fn size_bytes(&self) -> usize {
        match self {
            Type::MuscleId => 32,     // 256 bits
            Type::SealedBlob => 8256, // Max size
            Type::DeviceProof => 512,
            Type::ByteArray32 => 32,
            Type::U8 => 1,
            Type::U64 => 8,
            Type::MuscleUpdate => 128,   // Approximate
            Type::ExecutableMuscle => 8, // Handle size
        }
    }

    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            Type::U8 | Type::U64 | Type::MuscleId | Type::ByteArray32
        )
    }
}
