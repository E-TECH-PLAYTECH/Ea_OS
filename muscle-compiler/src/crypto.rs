// muscle-compiler/src/crypto.rs
// Eä Muscle Compiler Crypto Engine v5.0 — Compatible with Referee v5.0

use aes_gcm_siv::{aead::Aead, Aes256GcmSiv, KeyInit, Nonce};
use blake3::{Hasher, OUT_LEN};
use rand::RngCore;
use zeroize::Zeroize;

use crate::error::CompileError;

/// Protocol version — must match referee
const PROTOCOL_VERSION: &[u8] = b"Ea/muscle/v5.0";

/// Domain separation constants (must match referee exactly)
const DOMAIN_KDF: [u8; 32] = *b"\xde\xad\xbe\xef\xca\xfe\xba\xbe\x01\x23\x45\x67\x89\xab\xcd\xef\
                                 \xde\xad\xbe\xef\xca\xfe\xba\xbe\x01\x23\x45\x67\x89\xab\xcd\xef";
const DOMAIN_MAC: [u8; 32] = *b"\xf0\x0d\xfa\xce\xfe\xed\xba\xbe\x88\x77\x66\x55\x44\x33\x22\x11\
                                 \xf0\x0d\xfa\xce\xfe\xed\xba\xbe\x88\x77\x66\x55\x44\x33\x22\x11";

pub type MuscleSalt = [u8; 16];
pub type MuscleVersion = u64;

const MAX_MACHINE_CODE: usize = 8192;
const NONCE_LEN: usize = 12;

/// Derive key with domain separation (matches referee)
fn derive(key_material: &[u8; 32], salt: &MuscleSalt, domain: &[u8; 32]) -> [u8; 32] {
    let mut h = Hasher::new_keyed(key_material);
    h.update(PROTOCOL_VERSION);
    h.update(domain);
    h.update(salt);
    *h.finalize().as_bytes()
}

/// Seal muscle blob with v5.0 crypto (matches referee's open())
///
/// Output format: [version:8][nonce:12][AES-GCM-SIV ciphertext][BLAKE3 MAC:32]
pub fn seal_muscle_blob(
    machine_code: &[u8],
    chaos_master: &[u8; 32],
    salt: &MuscleSalt,
    version: MuscleVersion,
) -> Result<Vec<u8>, CompileError> {
    if machine_code.len() > MAX_MACHINE_CODE {
        return Err(CompileError::CryptoError(format!(
            "Machine code exceeds {} bytes ({} bytes provided)",
            MAX_MACHINE_CODE,
            machine_code.len()
        )));
    }

    // Generate random nonce
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Derive keys (matches referee's derivation chain)
    let mut shared_secret = derive(chaos_master, salt, &DOMAIN_KDF);
    let mut enc_key = derive(&shared_secret, salt, &DOMAIN_KDF);
    let mut mac_key = derive(&shared_secret, salt, &DOMAIN_MAC);

    // Encrypt with AES-256-GCM-SIV
    let ciphertext = Aes256GcmSiv::new(&enc_key.into())
        .encrypt(nonce, machine_code)
        .map_err(|e| CompileError::CryptoError(format!("encryption failed: {:?}", e)))?;

    // Compute MAC over: PROTOCOL_VERSION || salt || kem_ct || version || nonce || ciphertext
    // Note: kem_ct is empty in classical mode
    let kem_ct: &[u8] = &[];
    let mut h = Hasher::new_keyed(&mac_key);
    h.update(PROTOCOL_VERSION);
    h.update(salt);
    h.update(kem_ct);
    h.update(&version.to_le_bytes());
    h.update(&nonce_bytes);
    h.update(&ciphertext);
    let mac = h.finalize();

    // Build output: [version:8][nonce:12][ciphertext][mac:32]
    let mut sealed = Vec::with_capacity(8 + NONCE_LEN + ciphertext.len() + OUT_LEN);
    sealed.extend_from_slice(&version.to_le_bytes());
    sealed.extend_from_slice(&nonce_bytes);
    sealed.extend_from_slice(&ciphertext);
    sealed.extend_from_slice(mac.as_bytes());

    // Cleanup sensitive material
    shared_secret.zeroize();
    enc_key.zeroize();
    mac_key.zeroize();

    Ok(sealed)
}

/// Legacy function name for backward compatibility
/// Delegates to seal_muscle_blob with default version
pub fn encrypt_muscle_blob(
    machine_code: &[u8],
    chaos_master: &[u8; 32],
) -> Result<Vec<u8>, CompileError> {
    // Use default salt and version 1 for legacy calls
    let salt = [0u8; 16];
    seal_muscle_blob(machine_code, chaos_master, &salt, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seal_produces_correct_format() {
        let machine_code = b"test muscle code";
        let chaos_master = [0x42u8; 32];
        let salt = [0x13u8; 16];
        let version: u64 = 1;

        let sealed = seal_muscle_blob(machine_code, &chaos_master, &salt, version).unwrap();

        // Check minimum size: 8 (version) + 12 (nonce) + 16 (min ciphertext with GCM tag) + 32 (mac)
        assert!(sealed.len() >= 8 + 12 + 16 + 32);

        // Check version field
        let version_bytes = &sealed[..8];
        let parsed_version = u64::from_le_bytes(version_bytes.try_into().unwrap());
        assert_eq!(parsed_version, 1);
    }

    #[test]
    fn test_seal_different_nonces() {
        let machine_code = b"test";
        let chaos_master = [0x42u8; 32];
        let salt = [0x13u8; 16];

        let sealed1 = seal_muscle_blob(machine_code, &chaos_master, &salt, 1).unwrap();
        let sealed2 = seal_muscle_blob(machine_code, &chaos_master, &salt, 1).unwrap();

        // Different nonces should produce different ciphertexts
        assert_ne!(sealed1, sealed2);
    }

    #[test]
    fn test_seal_size_limit() {
        let oversized = vec![0u8; MAX_MACHINE_CODE + 1];
        let chaos_master = [0x42u8; 32];
        let salt = [0x13u8; 16];

        let result = seal_muscle_blob(&oversized, &chaos_master, &salt, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_output_is_not_plaintext() {
        let machine_code = b"SECRET_MUSCLE_CODE_12345";
        let chaos_master = [0x42u8; 32];
        let salt = [0x13u8; 16];

        let sealed = seal_muscle_blob(machine_code, &chaos_master, &salt, 1).unwrap();

        // The sealed output should NOT contain the plaintext
        let plaintext_in_output = sealed
            .windows(machine_code.len())
            .any(|window| window == machine_code);
        assert!(
            !plaintext_in_output,
            "SECURITY: Sealed output must not contain plaintext!"
        );
    }
}
