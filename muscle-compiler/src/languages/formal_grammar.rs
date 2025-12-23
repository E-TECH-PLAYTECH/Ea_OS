//! Complete EBNF parser for Muscle.ea language specification v1.0
//! Implements the full Wizard Stack grammar with capability security

use crate::ast::full_ast::*;
use crate::error::CompileError;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while, take_while1},
    character::complete::{
        alpha1, alphanumeric1, char, digit1, hex_digit1, multispace0, multispace1,
    },
    combinator::{map, opt, recognize, value},
    multi::{many0, many1, separated_list0},
    sequence::{delimited, pair, preceded, separated_pair, terminated, tuple},
    IResult, Parser,
};
use std::collections::HashMap;

pub struct FormalParser;

impl FormalParser {
    /// Parse complete Muscle.ea program according to EBNF specification
    pub fn parse_program(source: &str) -> Result<Program, CompileError> {
        let (remaining, program) = parse_program(source)
            .map_err(|e| CompileError::SyntaxError(format!("Parse error: {:?}", e)))?;

        if !remaining.trim().is_empty() {
            return Err(CompileError::SyntaxError(format!(
                "Unexpected content after program: '{}'",
                remaining
            )));
        }

        Ok(program)
    }
}

// EBNF: program = { declaration } , { rule }
fn parse_program(input: &str) -> IResult<&str, Program> {
    let (input, declarations) = many0(parse_declaration).parse(input)?;
    let (input, _) = multispace0.parse(input)?;
    let (input, rules) = many1(parse_rule).parse(input)?;

    Ok((
        input,
        Program {
            declarations,
            rules,
        },
    ))
}

// EBNF: declaration = input_decl | capability_decl | const_decl | metadata_decl
fn parse_declaration(input: &str) -> IResult<&str, Declaration> {
    let (input, _) = multispace0(input)?;
    alt((
        map(parse_input_decl, Declaration::Input),
        map(parse_capability_decl, Declaration::Capability),
        map(parse_const_decl, Declaration::Const),
        map(parse_metadata_decl, Declaration::Metadata),
    ))
    .parse(input)
}

// EBNF: input_decl = "input" identifier "<" type ">"
fn parse_input_decl(input: &str) -> IResult<&str, InputDecl> {
    let (input, _) = tag("input")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("<")(input)?;
    let (input, data_type) = parse_type(input)?;
    let (input, _) = tag(">")(input)?;
    let (input, _) = multispace0(input)?;

    Ok((
        input,
        InputDecl {
            name: name.to_string(),
            data_type,
        },
    ))
}

// EBNF: capability_decl = "capability" identifier "(" [param_list] ")"
fn parse_capability_decl(input: &str) -> IResult<&str, CapabilityDecl> {
    let (input, _) = tag("capability")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("(")(input)?;
    let (input, params) = parse_param_list(input)?;
    let (input, _) = tag(")")(input)?;
    let (input, return_type) = opt(preceded(
        multispace0,
        preceded(tag("->"), preceded(multispace0, parse_type)),
    ))
    .parse(input)?;
    let (input, _) = multispace0(input)?;

    Ok((
        input,
        CapabilityDecl {
            name: name.to_string(),
            parameters: params,
            return_type,
        },
    ))
}

// EBNF: const_decl = "const" identifier ":" type "=" literal
fn parse_const_decl(input: &str) -> IResult<&str, ConstDecl> {
    let (input, _) = tag("const")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag(":")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, const_type) = parse_type(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("=")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, value) = parse_literal(input)?;
    let (input, _) = multispace0(input)?;

    Ok((
        input,
        ConstDecl {
            name: name.to_string(),
            const_type,
            value,
        },
    ))
}

// EBNF: metadata_decl = identifier ":" string_literal
fn parse_metadata_decl(input: &str) -> IResult<&str, MetadataDecl> {
    let (input, name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag(":")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, value) = parse_string_literal(input)?;
    let (input, _) = multispace0(input)?;

    Ok((
        input,
        MetadataDecl {
            name: name.to_string(),
            value,
        },
    ))
}

// EBNF: rule = "rule" event_name ":" { statement }
fn parse_rule(input: &str) -> IResult<&str, Rule> {
    let (input, _) = tag("rule")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, event) = parse_event_name(input)?;
    let (input, _) = tag(":")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, body) = many1(parse_statement)(input)?;

    // Ensure no leftover content after parsing a rule and prepare for the next rule
    let (input, _) = multispace0(input)?;

    Ok((input, Rule { event, body }))
}

// EBNF: event_name = "on_boot" | "on_lattice_update(" identifier ":" type ")" | "on_timer_1hz" | "on_self_integrity_failure" | identifier
fn parse_event_name(input: &str) -> IResult<&str, Event> {
    alt((
        value(Event::OnBoot, tag("on_boot")),
        value(Event::OnTimer1Hz, tag("on_timer_1hz")),
        value(
            Event::OnSelfIntegrityFailure,
            tag("on_self_integrity_failure"),
        ),
        parse_lattice_update_event,
        map(parse_identifier, |id| Event::Custom(id.to_string())),
    ))
    .parse(input)
}

fn parse_lattice_update_event(input: &str) -> IResult<&str, Event> {
    dbg!(input);
    let (input, _) = tag("on_lattice_update(")(input)?;
    let (input, param_name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag(":")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, param_type) = parse_type(input)?;
    let (input, _) = tag(")")(input)?;

    Ok((
        input,
        Event::OnLatticeUpdate {
            param_name: param_name.to_string(),
            param_type,
        },
    ))
}

// EBNF: statement = verify_stmt | let_stmt | if_stmt | emit_stmt | schedule_stmt | unschedule_stmt | static_decl | expression
fn parse_statement(input: &str) -> IResult<&str, Statement> {
    // Added debug instrumentation to trace parsing issues
    dbg!(input); // Debug the input before parsing
    let (input, _) = multispace0(input)?;
    let result = alt((
        map(parse_verify_stmt, Statement::Verify),
        map(parse_let_stmt, Statement::Let),
        map(parse_if_stmt, Statement::If),
        map(parse_emit_stmt, Statement::Emit),
        map(parse_schedule_stmt, Statement::Schedule),
        map(parse_unschedule_stmt, Statement::Unschedule),
        map(parse_expression, Statement::Expr),
    ))
    .parse(input)?;

    let (input, _) = multispace0(input)?;
    Ok(result)
}

// EBNF: verify_stmt = "verify" expression
fn parse_verify_stmt(input: &str) -> IResult<&str, VerifyStmt> {
    let (input, _) = tag("verify")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, condition) = parse_expression(input)?;

    Ok((input, VerifyStmt { condition }))
}

// EBNF: let_stmt = "let" identifier [ "=" expression ]
fn parse_let_stmt(input: &str) -> IResult<&str, LetStmt> {
    let (input, _) = tag("let")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = parse_identifier(input)?;
    let (input, value) = opt(preceded(
        multispace0,
        preceded(tag("="), preceded(multispace0, parse_expression)),
    ))
    .parse(input)?;

    Ok((
        input,
        LetStmt {
            name: name.to_string(),
            value,
        },
    ))
}

// EBNF: if_stmt = "if" expression "->" action [ "else" "->" action ]
fn parse_if_stmt(input: &str) -> IResult<&str, IfStmt> {
    dbg!(input); // Debug the input before parsing
    let (input, _) = tag("if")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, condition) = parse_expression(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("->")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, then_branch) = parse_action(input)?;
    let (input, else_branch) = opt(preceded(
        multispace0,
        preceded(
            tag("else"),
            preceded(
                multispace0,
                preceded(tag("->"), preceded(multispace0, parse_action)),
            ),
        ),
    ))(input)?;

    // Ensure proper block termination and avoid consuming content for the next rule
    let (input, _) = multispace0(input)?;

    Ok((
        input,
        IfStmt {
            condition,
            then_branch,
            else_branch,
        },
    ))
}

fn parse_action(input: &str) -> IResult<&str, Vec<Statement>> {
    // Action is one or more statements, typically on same line
    many1(terminated(parse_statement, opt(multispace1))).parse(input)
}

// EBNF: emit_stmt = "emit" identifier "(" [arg_list] ")"
fn parse_emit_stmt(input: &str) -> IResult<&str, EmitStmt> {
    let (input, _) = tag("emit")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, event) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("(")(input)?;
    let (input, args) = parse_arg_list(input)?;
    let (input, _) = tag(")")(input)?;

    Ok((
        input,
        EmitStmt {
            event: event.to_string(),
            arguments: args,
        },
    ))
}

// EBNF: schedule_stmt = "schedule(" expression "," "priority:" literal ")"
fn parse_schedule_stmt(input: &str) -> IResult<&str, ScheduleStmt> {
    let (input, _) = tag("schedule(")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, muscle) = parse_expression(input)?;
    let (input, _) = tag(",")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("priority:")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, priority) = parse_literal(input)?;
    let (input, _) = tag(")")(input)?;

    Ok((input, ScheduleStmt { muscle, priority }))
}

// EBNF: unschedule_stmt = "unschedule(" "muscle_id:" expression ")"
fn parse_unschedule_stmt(input: &str) -> IResult<&str, UnscheduleStmt> {
    let (input, _) = tag("unschedule(")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("muscle_id:")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, muscle_id) = parse_expression(input)?;
    let (input, _) = tag(")")(input)?;

    Ok((input, UnscheduleStmt { muscle_id }))
}

// EBNF: expression = literal | identifier | field_access | call_expr | binary_expr | "self.id" | "self.version"
fn parse_expression(input: &str) -> IResult<&str, Expression> {
    alt((
        map(parse_literal, Expression::Literal),
        map(parse_self_reference, Expression::SelfRef),
        map(parse_call_expr, Expression::Call),
        map(parse_field_access, Expression::FieldAccess),
        map(parse_binary_expr, Expression::Binary),
        map(parse_identifier, |id| Expression::Variable(id.to_string())),
    ))
    .parse(input)
}

fn parse_self_reference(input: &str) -> IResult<&str, SelfReference> {
    alt((
        value(SelfReference::Id, tag("self.id")),
        value(SelfReference::Version, tag("self.version")),
    ))
    .parse(input)
}

// EBNF: type = "MuscleUpdate" | "DeviceProof" | "SealedBlob" | "ExecutableMuscle" | "muscle_id" | "u8" | "u64" | "[u8; 32]"
fn parse_type(input: &str) -> IResult<&str, Type> {
    alt((
        value(Type::MuscleUpdate, tag("MuscleUpdate")),
        value(Type::DeviceProof, tag("DeviceProof")),
        value(Type::SealedBlob, tag("SealedBlob")),
        value(Type::ExecutableMuscle, tag("ExecutableMuscle")),
        value(Type::MuscleId, tag("muscle_id")),
        value(Type::U8, tag("u8")),
        value(Type::U64, tag("u64")),
        value(Type::ByteArray32, tag("[u8; 32]")),
    ))
    .parse(input)
}

// EBNF: literal = hex_literal | integer_literal | string_literal
fn parse_literal(input: &str) -> IResult<&str, Literal> {
    alt((
        map(parse_hex_literal, Literal::Hex),
        map(parse_integer_literal, Literal::Integer),
        map(parse_string_literal, Literal::String),
    ))
    .parse(input)
}

// EBNF: hex_literal = "0x" [0-9a-fA-F]+
fn parse_hex_literal(input: &str) -> IResult<&str, String> {
    let (input, _) = tag("0x")(input)?;
    let (input, digits) = hex_digit1(input)?;
    Ok((input, format!("0x{}", digits)))
}

fn parse_integer_literal(input: &str) -> IResult<&str, u64> {
    let (input, digits) = digit1(input)?;
    let value = digits.parse().unwrap_or(0);
    Ok((input, value))
}

fn parse_string_literal(input: &str) -> IResult<&str, String> {
    let (input, s) = delimited(char('"'), take_while(|c| c != '"'), char('"')).parse(input)?;
    Ok((input, s.to_string()))
}

fn parse_identifier(input: &str) -> IResult<&str, &str> {
    recognize(pair(
        alt((alpha1, tag("_"))),
        many0(alt((alphanumeric1, tag("_")))),
    ))
    .parse(input)
}

fn parse_param_list(input: &str) -> IResult<&str, Vec<Parameter>> {
    separated_list0(
        preceded(multispace0, tag(",")),
        preceded(multispace0, parse_parameter),
    )
    .parse(input)
}

fn parse_parameter(input: &str) -> IResult<&str, Parameter> {
    let (input, name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag(":")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, param_type) = parse_type(input)?;

    Ok((
        input,
        Parameter {
            name: name.to_string(),
            param_type,
        },
    ))
}

fn parse_arg_list(input: &str) -> IResult<&str, Vec<Expression>> {
    separated_list0(
        preceded(multispace0, tag(",")),
        preceded(multispace0, parse_expression),
    )
    .parse(input)
}

fn parse_call_expr(input: &str) -> IResult<&str, CallExpr> {
    let (input, name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("(")(input)?;
    let (input, args) = parse_arg_list(input)?;
    let (input, _) = tag(")")(input)?;

    Ok((
        input,
        CallExpr {
            function: name.to_string(),
            arguments: args,
        },
    ))
}

fn parse_field_access(input: &str) -> IResult<&str, FieldAccess> {
    let (input, object) = parse_identifier(input)?;
    let (input, _) = tag(".")(input)?;
    let (input, field) = parse_identifier(input)?;

    Ok((
        input,
        FieldAccess {
            object: object.to_string(),
            field: field.to_string(),
        },
    ))
}

fn parse_binary_expr(input: &str) -> IResult<&str, BinaryExpr> {
    let (input, left) = parse_primary_expression(input)?;
    let (input, _) = multispace0(input)?;
    let (input, op) = parse_operator(input)?;
    let (input, _) = multispace0(input)?;
    let (input, right) = parse_primary_expression(input)?;

    Ok((
        input,
        BinaryExpr {
            left: Box::new(left),
            op,
            right: Box::new(right),
        },
    ))
}

fn parse_primary_expression(input: &str) -> IResult<&str, Expression> {
    alt((
        map(parse_literal, Expression::Literal),
        map(parse_self_reference, Expression::SelfRef),
        map(parse_identifier, |id| Expression::Variable(id.to_string())),
    ))
    .parse(input)
}

fn parse_operator(input: &str) -> IResult<&str, BinaryOperator> {
    alt((
        value(BinaryOperator::Eq, tag("==")),
        value(BinaryOperator::Ne, tag("!=")),
        value(BinaryOperator::Lt, tag("<")),
        value(BinaryOperator::Gt, tag(">")),
        value(BinaryOperator::Le, tag("<=")),
        value(BinaryOperator::Ge, tag(">=")),
        value(BinaryOperator::Add, tag("+")),
        value(BinaryOperator::Sub, tag("-")),
    ))
    .parse(input)
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn test_parse_complete_nucleus() {
        let source = r#"
input lattice_stream<MuscleUpdate>
input hardware_attestation<DeviceProof>
input symbiote<SealedBlob>

capability load_muscle(id: muscle_id) -> ExecutableMuscle
capability schedule(muscle: ExecutableMuscle, priority: u8) 
capability emit_update(blob: SealedBlob)

const SYMBIOTE_ID: muscle_id = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF

rule on_boot:
    verify hardware_attestation.verify()
    verify lattice_root == 0xEA0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f
    let symbiote_instance = load_muscle(SYMBIOTE_ID)
    schedule(symbiote_instance, priority: 255)

rule on_lattice_update(update: MuscleUpdate):
    if symbiote.process_update(update) -> healing:
        emit_update(healing.blob)

rule on_timer_1hz:
    emit heartbeat(self.id, self.version)

rule on_self_integrity_failure:
    emit corruption_report(self.id, referee.self_check_failed())
"#;

        let program = FormalParser::parse_program(source).unwrap();
        assert_eq!(program.declarations.len(), 8); // 3 inputs + 3 capabilities + 1 const + 1 metadata
        assert_eq!(program.rules.len(), 4);
    }

    #[test]
    fn test_parse_minimal_cell() {
        let source = r#"
input lattice_stream<MuscleUpdate>
capability emit_update(blob: SealedBlob)

rule on_boot:
    emit heartbeat("I am alive")

rule on_timer_1hz:
    emit heartbeat("Still breathing")
"#;

        let program = FormalParser::parse_program(source).unwrap();
        assert_eq!(program.declarations.len(), 2);
        assert_eq!(program.rules.len(), 2);
    }
}
