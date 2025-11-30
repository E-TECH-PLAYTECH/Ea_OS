# Eä Ecosystem Architecture v5.0

## System Overview

The Eä Ecosystem is a secure, capability-based execution environment consisting of two main components:

1. **Muscle Compiler** - Transforms Python neural network definitions into encrypted, isolated executables
2. **Referee** - Secure UEFI bootloader that loads and executes muscles in cryptographically isolated environments

## Architectural Principles

### Minimal Trusted Computing Base (TCB)
- Referee: 59.8 KiB total binary size
- Zero `unsafe` code in cryptographic core
- Formal verification-ready code structure

### Cryptographic First Principles
- Security derived from cryptographic proofs, not procedural checks
- All components cryptographically bound to master key
- Defense in depth with multiple verification layers

### Capability-Based Security
- Muscles execute with minimal privileges
- No inter-muscle communication by design
- Cryptographic capabilities enforce isolation

## Component Architecture

### Muscle Compiler
