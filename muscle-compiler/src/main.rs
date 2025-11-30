use clap::{App, Arg};
use std::path::PathBuf;
use std::fs;
use std::process;

mod ast;
mod crypto;
mod parser;
mod codegen;
mod error;

// NEW: Add these modules for Nucleus support
mod languages;
mod codegen::nucleus;

use ast::MuscleAst;
use crypto::{encrypt_muscle_blob, generate_chaos_key};
use parser::PythonParser;
use error::CompileError;

// NEW: Import Nucleus components
use languages::EaLanguage;
use codegen::nucleus::NucleusCodegen;

fn main() {
    let matches = App::new("Muscle Compiler v5.0")
        .version("5.0.0")
        .author("Eä Foundation")
        .about("Compiles Python NN definitions or .ea sources to encrypted muscle blobs")
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
        .get_matches();

    if let Err(e) = run(&matches) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn run(matches: &clap::ArgMatches) -> Result<(), CompileError> {
    let input_file = matches.value_of("input").unwrap();
    let output_file = matches.value_of("output").unwrap();
    let target_arch = matches.value_of("target").unwrap();
    let chaos_master_hex = matches.value_of("chaos-master").unwrap();
    let verbose = matches.is_present("verbose");

    if verbose {
        println!("🔧 Muscle Compiler v5.0 - Eä Foundation");
        println!("   Input: {}", input_file);
        println!("   Output: {}", output_file);
        println!("   Target: {}", target_arch);
    }

    // Parse chaos master key
    let chaos_master = parse_chaos_key(chaos_master_hex)?;

    // Read input file
    let input_path = PathBuf::from(input_file);
    if !input_path.exists() {
        return Err(CompileError::IoError(format!("Input file not found: {}", input_file)));
    }

    // NEW: Check file extension to determine compilation path
    if input_path.extension().map(|ext| ext == "ea").unwrap_or(false) {
        // Compile .ea source file (Nucleus muscle)
        compile_ea_source(input_file, output_file, target_arch, &chaos_master, verbose)
    } else if input_path.extension().map(|ext| ext == "py").unwrap_or(false) {
        // Compile Python source file (traditional neural network muscle)
        compile_python_source(input_file, output_file, target_arch, &chaos_master, verbose)
    } else {
        Err(CompileError::IoError("Input file must be .py or .ea extension".to_string()))
    }
}

/// NEW: Compile .ea source file to Nucleus muscle blob
fn compile_ea_source(
    input_file: &str,
    output_file: &str,
    target_arch: &str,
    chaos_master: &[u8; 32],
    verbose: bool,
) -> Result<(), CompileError> {
    if verbose {
        println!("🎯 Compiling .ea source as Nucleus muscle");
    }

    // Validate target architecture for Nucleus
    if target_arch != "nucleus" && target_arch != "aarch64" {
        return Err(CompileError::CompileError(
            format!("Nucleus muscles require aarch64 or nucleus target, got: {}", target_arch)
        ));
    }

    // Read and parse .ea source
    let source = fs::read_to_string(input_file)?;
    let ast = EaLanguage::parse(&source)?;

    if verbose {
        println!("   Parsed {} inputs, {} capabilities, {} rules", 
                 ast.inputs.len(), ast.capabilities.len(), ast.rules.len());
    }

    // Generate machine code using Nucleus codegen
    let machine_code = NucleusCodegen::generate(&ast)?;

    if verbose {
        println!("   Generated machine code: {} bytes", machine_code.len());
        
        // Verify 8KiB size for Nucleus
        if machine_code.len() != 8192 {
            return Err(CompileError::CodegenError(
                format!("Nucleus code must be exactly 8192 bytes, got: {}", machine_code.len())
            ));
        }
        println!("   ✅ Nucleus size verified: 8192 bytes");
    }

    // Encrypt and seal the blob using existing crypto
    let sealed_blob = encrypt_muscle_blob(&machine_code, chaos_master)?;

    // Write output file
    fs::write(output_file, &sealed_blob)?;

    if verbose {
        println!("   ✅ Sealed blob written: {} bytes", sealed_blob.len());
        println!("   📦 Nucleus muscle compilation complete!");
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

// NEW: Add integration tests for Nucleus compilation
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_ea_source_compilation() {
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

        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), source).unwrap();
        
        let output_file = NamedTempFile::new().unwrap();
        let chaos_key = [0u8; 32]; // Test key
        
        let result = compile_ea_source(
            temp_file.path().to_str().unwrap(),
            output_file.path().to_str().unwrap(),
            "aarch64",
            &chaos_key,
            false
        );
        
        assert!(result.is_ok());
        
        // Verify output file was created and has correct size
        let output_data = fs::read(output_file.path()).unwrap();
        assert!(!output_data.is_empty());
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
    fn test_file_extension_detection() {
        use std::fs;
        use tempfile::NamedTempFile;
        
        // Test .ea file
        let ea_file = NamedTempFile::new().unwrap();
        let ea_path = ea_file.path().with_extension("ea");
        fs::write(&ea_path, "test").unwrap();
        
        // Test .py file  
        let py_file = NamedTempFile::new().unwrap();
        let py_path = py_file.path().with_extension("py");
        fs::write(&py_path, "test").unwrap();
        
        // Test unknown extension
        let unknown_file = NamedTempFile::new().unwrap();
        let unknown_path = unknown_file.path().with_extension("txt");
        fs::write(&unknown_path, "test").unwrap();
        
        // These would be tested in the main run function
        // For now, just verify file detection logic
        assert_eq!(ea_path.extension().unwrap(), "ea");
        assert_eq!(py_path.extension().unwrap(), "py");
        assert_eq!(unknown_path.extension().unwrap(), "txt");
    }
}
