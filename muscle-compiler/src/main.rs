use clap::{App, Arg, ArgMatches};
use std::path::PathBuf;
use std::fs;
use std::process;

mod ast;
mod crypto;
mod parser;
mod codegen;
mod error;

// UPDATED: Enhanced modules for full Wizard Stack specification
mod languages;
mod codegen::nucleus;

use ast::full_ast::{Program, Declaration};
use crypto::{encrypt_muscle_blob, generate_chaos_key};
use parser::PythonParser;
use error::CompileError;

// UPDATED: Import full specification components
use languages::formal_grammar::FormalParser;
use languages::capability_checker::{CapabilityChecker, verify_sacred_rules};
use codegen::nucleus::NucleusCodegen;

fn main() {
    let matches = App::new("Muscle Compiler v5.0 - Wizard Stack")
        .version("5.0.0")
        .author("Eä Foundation")
        .about("Compiles Python NN definitions or Muscle.ea sources to encrypted muscle blobs")
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .value_name("FILE")
                .help("Input Python file (.py) or Nucleus source (.ea)")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output") 
                .value_name("FILE")
                .help("Output encrypted blob file")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::new("target")
                .short('t')
                .long("target")
                .value_name("ARCH")
                .help("Target architecture (aarch64, x86_64, nucleus)")
                .default_value("aarch64")
                .takes_value(true),
        )
        .arg(
            Arg::new("chaos-master")
                .long("chaos-master")
                .value_name("KEY")
                .help("32-byte hex chaos master key for encryption")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Enable verbose output"),
        )
        .arg(
            Arg::new("verify-only")
                .long("verify-only")
                .help("Only verify the source code, don't compile"),
        )
        .arg(
            Arg::new("dump-ast")
                .long("dump-ast")
                .help("Dump the parsed AST for debugging"),
        )
        .get_matches();

    if let Err(e) = run(&matches) {
        eprintln!("❌ Error: {}", e);
        process::exit(1);
    }
}

fn run(matches: &ArgMatches) -> Result<(), CompileError> {
    let input_file = matches.value_of("input").unwrap();
    let output_file = matches.value_of("output").unwrap();
    let target_arch = matches.value_of("target").unwrap();
    let chaos_master_hex = matches.value_of("chaos-master").unwrap();
    let verbose = matches.is_present("verbose");
    let verify_only = matches.is_present("verify-only");
    let dump_ast = matches.is_present("dump-ast");

    if verbose {
        println!("🔧 Muscle Compiler v5.0 - Wizard Stack Specification");
        println!("   Input: {}", input_file);
        println!("   Output: {}", output_file);
        println!("   Target: {}", target_arch);
        println!("   Mode: {}", if verify_only { "verify-only" } else { "compile" });
    }

    // Parse chaos master key
    let chaos_master = parse_chaos_key(chaos_master_hex)?;

    // Read input file
    let input_path = PathBuf::from(input_file);
    if !input_path.exists() {
        return Err(CompileError::IoError(format!("Input file not found: {}", input_file)));
    }

    // UPDATED: Enhanced file type detection with full spec support
    if input_path.extension().map(|ext| ext == "ea").unwrap_or(false) {
        // Compile .ea source file with full Wizard Stack specification
        compile_ea_source_full_spec(input_file, output_file, target_arch, &chaos_master, verbose, verify_only, dump_ast)
    } else if input_path.extension().map(|ext| ext == "py").unwrap_or(false) {
        // Compile Python source file (traditional neural network muscle)
        compile_python_source(input_file, output_file, target_arch, &chaos_master, verbose)
    } else {
        Err(CompileError::IoError("Input file must be .py or .ea extension".to_string()))
    }
}

/// UPDATED: Compile .ea source file with full Wizard Stack specification
fn compile_ea_source_full_spec(
    input_file: &str,
    output_file: &str,
    target_arch: &str,
    chaos_master: &[u8; 32],
    verbose: bool,
    verify_only: bool,
    dump_ast: bool,
) -> Result<(), CompileError> {
    if verbose {
        println!("🎯 Compiling .ea source with Wizard Stack Specification");
        println!("   Language: Muscle.ea v1.0 - The Language of Life");
    }

    // Validate target architecture for Nucleus
    if target_arch != "nucleus" && target_arch != "aarch64" {
        return Err(CompileError::CompileError(
            format!("Nucleus muscles require aarch64 or nucleus target, got: {}", target_arch)
        ));
    }

    // Read and parse .ea source with full EBNF grammar
    let source = fs::read_to_string(input_file)?;
    
    if verbose {
        println!("   📖 Parsing source code ({} bytes)", source.len());
    }

    let program = FormalParser::parse_program(&source)?;

    if dump_ast {
        println!("{:#?}", program);
    }

    if verbose {
        println!("   ✅ Parsed successfully:");
        println!("      - {} declarations", program.declarations.len());
        println!("      - {} rules", program.rules.len());
        
        // Count declaration types
        let mut input_count = 0;
        let mut capability_count = 0;
        let mut const_count = 0;
        
        for decl in &program.declarations {
            match decl {
                Declaration::Input(_) => input_count += 1,
                Declaration::Capability(_) => capability_count += 1,
                Declaration::Const(_) => const_count += 1,
                Declaration::Metadata(_) => {},
            }
        }
        
        println!("      - {} inputs, {} capabilities, {} constants", 
                 input_count, capability_count, const_count);
    }

    // UPDATED: Enhanced security verification
    if verbose {
        println!("   🔒 Verifying capability security...");
    }
    
    let mut capability_checker = CapabilityChecker::new();
    capability_checker.verify_program(&program)?;
    
    if verbose {
        println!("   ✅ Capability security verified");
    }

    // UPDATED: Verify the Three Sacred Rules
    if verbose {
        println!("   📜 Verifying Sacred Rules of Muscle.ea...");
    }
    
    verify_sacred_rules(&program)?;
    
    if verbose {
        println!("   ✅ Sacred Rules verified:");
        println!("      - Append-only semantics");
        println!("      - Event-driven architecture"); 
        println!("      - Capability-security enforced");
        println!("      - No polling constructs");
    }

    if verify_only {
        println!("🎉 Verification completed successfully - program is valid Muscle.ea");
        return Ok(());
    }

    // UPDATED: Generate machine code with enhanced codegen
    if verbose {
        println!("   🔨 Generating machine code with capability enforcement...");
    }

    let machine_code = NucleusCodegen::generate(&program)?;

    if verbose {
        println!("   ✅ Generated machine code: {} bytes", machine_code.len());
        
        // Verify 8KiB size for Nucleus
        if machine_code.len() != 8192 {
            return Err(CompileError::CodegenError(
                format!("Nucleus code must be exactly 8192 bytes, got: {}", machine_code.len())
            ));
        }
        println!("   📏 Nucleus size verified: 8192 bytes");
    }

    // Encrypt and seal the blob using existing crypto
    if verbose {
        println!("   🔐 Encrypting and sealing blob...");
    }

    let sealed_blob = encrypt_muscle_blob(&machine_code, chaos_master)?;

    // Write output file
    fs::write(output_file, &sealed_blob)?;

    if verbose {
        println!("   💾 Sealed blob written: {} bytes", sealed_blob.len());
        
        // Show security summary
        println!("   🛡️  Security Summary:");
        println!("      - Capability security: ENFORCED");
        println!("      - Sacred Rules: VERIFIED");
        println!("      - Cryptographic sealing: COMPLETE");
        println!("      - Biological integrity: MAINTAINED");
        
        println!("   📦 Nucleus muscle compilation complete!");
        println!("   🧬 Every valid program is a living cell ✓");
    }

    Ok(())
}

/// EXISTING: Compile Python source file to neural network muscle blob
fn compile_python_source(
    input_file: &str,
    output_file: &str,
    target_arch: &str,
    chaos_master: &[u8; 32],
    verbose: bool,
) -> Result<(), CompileError> {
    if verbose {
        println!("🐍 Compiling Python source as neural network muscle");
    }

    // Read and parse Python source
    let source = fs::read_to_string(input_file)?;
    let ast = PythonParser::parse(&source)?;

    if verbose {
        println!("   Parsed neural network with {} weights", ast.weights.len());
        
        // Show architecture info if available
        if let Some(layers) = &ast.metadata.get("layers") {
            println!("   Network architecture: {}", layers);
        }
    }

    // Generate machine code based on target architecture
    let machine_code = match target_arch {
        "aarch64" => codegen::aarch64::generate(&ast),
        "x86_64" => codegen::x86_64::generate(&ast),
        _ => return Err(CompileError::CompileError(
            format!("Unsupported target architecture: {}", target_arch)
        )),
    }?;

    if verbose {
        println!("   Generated machine code: {} bytes", machine_code.len());
    }

    // Encrypt and seal the blob
    let sealed_blob = encrypt_muscle_blob(&machine_code, chaos_master)?;

    // Write output file
    fs::write(output_file, &sealed_blob)?;

    if verbose {
        println!("   ✅ Sealed blob written: {} bytes", sealed_blob.len());
        println!("   📦 Neural network muscle compilation complete!");
    }

    Ok(())
}

/// Parse 32-byte hex chaos master key
fn parse_chaos_key(hex_str: &str) -> Result<[u8; 32], CompileError> {
    if hex_str.len() != 64 {
        return Err(CompileError::CryptoError(
            "Chaos master key must be 64 hex characters (32 bytes)".to_string()
        ));
    }

    let mut key = [0u8; 32];
    hex::decode_to_slice(hex_str, &mut key)
        .map_err(|e| CompileError::CryptoError(format!("Invalid hex key: {}", e)))?;

    Ok(key)
}

// UPDATED: Enhanced integration tests for full specification
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_full_spec_nucleus_compilation() {
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

        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), source).unwrap();
        
        let output_file = NamedTempFile::new().unwrap();
        let chaos_key = [0u8; 32];
        
        let result = compile_ea_source_full_spec(
            temp_file.path().to_str().unwrap(),
            output_file.path().to_str().unwrap(),
            "aarch64",
            &chaos_key,
            false,
            false,
            false
        );
        
        assert!(result.is_ok());
        
        let output_data = fs::read(output_file.path()).unwrap();
        assert_eq!(output_data.len(), 8256); // Standard sealed blob size
    }

    #[test]
    fn test_minimal_living_cell() {
        let source = r#"
input lattice_stream<MuscleUpdate>
capability emit_update(blob: SealedBlob)

rule on_boot:
    emit heartbeat("I am alive")

rule on_timer_1hz:
    emit heartbeat("Still breathing")
"#;

        let program = FormalParser::parse_program(source).unwrap();
        let mut checker = CapabilityChecker::new();
        assert!(checker.verify_program(&program).is_ok());
        assert!(verify_sacred_rules(&program).is_ok());
    }

    #[test]
    fn test_capability_enforcement_failure() {
        let source = r#"
input lattice_stream<MuscleUpdate>
# Missing capability declaration for emit_update

rule on_boot:
    emit heartbeat("This should fail")  # Uses undeclared capability
"#;

        let program = FormalParser::parse_program(source).unwrap();
        let mut checker = CapabilityChecker::new();
        assert!(checker.verify_program(&program).is_err());
    }

    #[test]
    fn test_chaos_key_parsing() {
        let valid_key = "a".repeat(64);
        let result = parse_chaos_key(&valid_key);
        assert!(result.is_ok());
        
        let invalid_key = "a".repeat(63);
        let result = parse_chaos_key(&invalid_key);
        assert!(result.is_err());
    }

    #[test]
    fn test_verification_only_mode() {
        let source = r#"
input lattice_stream<MuscleUpdate>
capability emit_update(blob: SealedBlob)

rule on_boot:
    emit heartbeat("Verification test")
"#;

        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), source).unwrap();
        
        let output_file = NamedTempFile::new().unwrap();
        let chaos_key = [0u8; 32];
        
        let result = compile_ea_source_full_spec(
            temp_file.path().to_str().unwrap(),
            output_file.path().to_str().unwrap(),
            "aarch64",
            &chaos_key,
            false,
            true,  // verify-only
            false
        );
        
        assert!(result.is_ok());
        
        // Output file should not exist in verify-only mode
        assert!(!output_file.path().exists());
    }
}
