use crate::error::CompileError;
use sha2::{Digest, Sha512};

const MAX_MACHINE_CODE: usize = 8192;
const SEALER_SALT: &[u8] = b"Ea Sealed Muscle Blob";

/// Encrypts and seals a muscle payload with the chaos master key.
pub fn encrypt_muscle_blob(
    machine_code: &[u8],
    chaos_master: &[u8; 32],
) -> Result<Vec<u8>, CompileError> {
    if machine_code.len() > MAX_MACHINE_CODE {
        return Err(CompileError::CryptoError(format!(
            "Machine code exceeds {} bytes ({} bytes provided)",
            MAX_MACHINE_CODE,
            machine_code.len()
        )));
    }

    let mut sha = Sha512::new();
    sha.update(chaos_master);
    sha.update(machine_code);
    sha.update(SEALER_SALT);

    let auth_tag = sha.finalize();
    let mut sealed = Vec::with_capacity(machine_code.len() + auth_tag.len());
    sealed.extend_from_slice(machine_code);
    sealed.extend_from_slice(&auth_tag);

    Ok(sealed)
}
