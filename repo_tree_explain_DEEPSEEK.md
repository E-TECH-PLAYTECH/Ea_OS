## **COMPLETE REPOSITORY STRUCTURE v5.0**

```
eä-ecosystem-v5.0/
├── muscle-compiler/                 # Muscle Compiler v5.0
│   ├── Cargo.toml                  # v5.0 dependencies + features
│   ├── src/
│   │   ├── main.rs                 # CLI driver with v5.0 crypto integration
│   │   ├── crypto.rs               # Cryptographic Engine v5.0 (Pathfinder Edition)
│   │   ├── parser.rs               # Robust Python weight extraction
│   │   ├── blob.rs                 # v5.0 blob format with integrity protection
│   │   ├── codegen/
│   │   │   ├── mod.rs              # Platform abstraction
│   │   │   ├── aarch64.rs          # AArch64 machine code generation
│   │   │   └── x86_64.rs           # x86_64 machine code generation
│   │   └── lib.rs                  # Library exports
│   ├── examples/
│   │   └── feanor.py               # Example muscle with v5.0 format
│   ├── tests/
│   │   └── integration_test.rs     # Crypto + parser tests
│   └── target/                     # Build output
│
├── referee/                        # Referee Bootloader v5.0
│   ├── Cargo.toml                  # UEFI + v5.0 crypto dependencies
│   ├── .cargo/
│   │   └── config.toml             # Cross-compilation configuration
│   ├── build.rs                    # UEFI build configuration
│   ├── src/
│   │   ├── main.rs                 # UEFI entry point with v5.0 integration
│   │   ├── crypto.rs               # Referee crypto (compatible with compiler)
│   │   ├── muscle_loader.rs        # v5.0 blob loading & validation
│   │   ├── uart.rs                 # Robust serial logging
│   │   ├── errors.rs               # Comprehensive error types
│   │   └── lib.rs                  # Library exports
│   ├── tests/
│   │   └── integration_test.rs     # System integration tests
│   └── target/                     # UEFI binary output
│
└── documentation/
    ├── ARCHITECTURE.md             # v5.0 system architecture
    ├── CRYPTO_SPEC.md              # Cryptographic protocol specification
    ├── INTEGRATION_GUIDE.md        # Compiler ↔ Referee integration
    └── SECURITY_AUDIT.md           # Security considerations
```

## **MUSCLE-COMPILER DETAILED STRUCTURE**

```
muscle-compiler/
├── Cargo.toml
├── src/
│   ├── main.rs                     # CLI: --chaos-master, --target, file I/O
│   ├── crypto.rs                   # seal()/open() with AES-GCM-SIV + Blake3
│   ├── parser.rs                   # extract_weights() from Python numpy arrays
│   ├── blob.rs                     # forge()/parse() v5.0 container format
│   └── codegen/
│       ├── mod.rs                  # CodeGenerator trait + dispatch
│       ├── aarch64.rs              # AArch64 SIMD with weight embedding
│       └── x86_64.rs               # x86_64 SSE/AVX with weight embedding
├── examples/
│   └── feanor.py                   # 4→3→1 network with ReLU activation
└── tests/
    └── integration_test.rs         # Property-based crypto tests
```

## **REFEREE DETAILED STRUCTURE**

```
referee/
├── Cargo.toml
├── .cargo/
│   └── config.toml                 # x86_64-unknown-uefi, aarch64-unknown-uefi
├── build.rs
├── src/
│   ├── main.rs                     # efi_main(), load_all_muscles(), run_scheduler()
│   ├── crypto.rs                   # open() only (compatible with compiler)
│   ├── muscle_loader.rs            # load_muscle(), parse_blob_header()
│   ├── uart.rs                     # UART init, write_str(), with timeouts
│   └── errors.rs                   # BootError, MuscleError enums
└── tests/
    └── integration_test.rs         # Round-trip crypto tests
```

## **KEY FILE DEPENDENCIES**

### **Muscle Compiler Workflow:**
```
feanor.py → parser.rs → Weights → codegen/ → Vec<u8> → crypto.rs → Vec<u8> → blob.rs → feanor.muscle
```

### **Referee Workflow:**
```
feanor.muscle → blob.rs → Vec<u8> → crypto.rs → Vec<u8> → muscle_loader.rs → LoadedMuscle → main.rs → execution
```

## **BUILD ARTIFACTS**

### **Muscle Compiler Outputs:**
```
muscle-compiler/target/release/muscle-compiler     # Binary
muscle-compiler/feanor.muscle                      # Generated muscle blob
```

### **Referee Outputs:**
```
referee/target/x86_64-unknown-uefi/release/referee.efi  # UEFI bootloader
```

## **CRITICAL PATHS**

### **Cryptographic Compatibility:**
- `muscle-compiler/src/crypto.rs` ↔ `referee/src/crypto.rs`
- Same `PROTOCOL_VERSION`, `DOMAIN_KDF`, `DOMAIN_MAC`
- Identical `derive()` function implementation

### **Blob Format Compatibility:**
- `muscle-compiler/src/blob.rs` ↔ `referee/src/muscle_loader.rs`
- Same magic "EaM5", header structure, integrity checking

### **Cross-Platform Support:**
- `muscle-compiler/src/codegen/aarch64.rs` ↔ AArch64 referee
- `muscle-compiler/src/codegen/x86_64.rs` ↔ x86_64 referee

This structure represents a **complete, production-ready v5.0 ecosystem** with proper separation of concerns, comprehensive testing, and robust integration between all components.
