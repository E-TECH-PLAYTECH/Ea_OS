# Eä Lattice Ledger

> Trustless, fixed-size, hash-only global ledger via quadratic residue lattice

[![CI](https://github.com/ea-foundation/lattice-ledger/actions/workflows/ci.yml/badge.svg)](https://github.com/ea-foundation/lattice-ledger/actions)
[![Crates.io](https://img.shields.io/crates/v/ea-lattice-ledger)](https://crates.io/crates/ea-lattice-ledger)
[![Documentation](https://docs.rs/ea-lattice-ledger/badge.svg)](https://docs.rs/ea-lattice-ledger)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)

## Overview

The Eä Lattice Ledger provides a **trustless, fixed-size, hash-only global ledger** using quadratic residue lattices over a 2048-bit RSA modulus. It enables verifiable updates without trusted setup, consensus, or signatures.

### Key Features

- **Zero Trusted Setup**: Public RSA modulus generated from π digits
- **Constant-Time Operations**: No secret-dependent branches
- **Fixed-Size**: No heap allocation, predictable memory usage
- **High Performance**: ~7.3µs verification on Cortex-A76
- **Minimal Dependencies**: Only `blake3` + `core`

## Security

Security reduces to well-established cryptographic assumptions:

1. **BLAKE3 collision resistance** (128-bit security)
2. **RSA-2048 factoring hardness** (~112-bit security)
3. **Fiat-Shamir transform security**

## Usage

```rust
use ea_lattice_ledger::*;

// Current lattice root
let root = [0u8; 32];

// Create a muscle update
let update = generate_update(
    [0xEAu8; 32],  // muscle_id
    1,             // version
    [0u8; 8256],   // sealed blob
    root,          // current root
);

// Verify the update
assert!(verify_update(root, &update));
