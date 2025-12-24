// referee/src/muscle_loader.rs
// Eä Muscle Loader v5.0 — Compatible with v5.0 blob format

use crate::crypto::{self, MuscleSalt};
use alloc::string::{String, ToString};
use blake3::Hasher;

#[cfg(feature = "uefi-runtime")]
use uefi::table::boot::{AllocateType, BootServices, MemoryType};

/// Parsed muscle blob information
pub struct LoadedMuscle {
    pub entry_point: u64,
    pub memory_pages: u64,
    pub name: String,
    pub arch: String,
}

/// Error types for muscle loading
#[derive(Debug)]
pub enum LoadError {
    InvalidFormat,
    IntegrityCheckFailed,
    MemoryAllocationFailed,
    ArchitectureMismatch,
    DecryptionFailed,
}

/// Load and validate a muscle blob from memory (UEFI runtime only)
#[cfg(feature = "uefi-runtime")]
pub fn load_muscle(
    boot_services: &BootServices,
    master_key: &[u8; 32],
    blob_data: &[u8],
    muscle_index: usize,
) -> Result<LoadedMuscle, LoadError> {
    // Parse blob header
    let (name, arch, sealed_payload) =
        parse_blob_header(blob_data).map_err(|_| LoadError::InvalidFormat)?;

    // Verify architecture compatibility
    if !is_architecture_supported(&arch) {
        return Err(LoadError::ArchitectureMismatch);
    }

    // Generate salt from muscle index and name
    let salt = generate_salt(muscle_index, &name);

    // Decrypt muscle payload
    let (decrypted_data, _version) =
        crypto::open(master_key, &salt, sealed_payload).map_err(|_| LoadError::DecryptionFailed)?;

    // Allocate executable memory for muscle
    let memory_pages = calculate_required_pages(decrypted_data.len());
    let memory_ptr = boot_services
        .allocate_pages(
            AllocateType::AnyPages,
            MemoryType::LOADER_CODE,
            memory_pages,
        )
        .map_err(|_| LoadError::MemoryAllocationFailed)?;

    // Copy decrypted code to executable memory
    unsafe {
        core::ptr::copy_nonoverlapping(
            decrypted_data.as_ptr(),
            memory_ptr as *mut u8,
            decrypted_data.len(),
        );
    }

    Ok(LoadedMuscle {
        entry_point: memory_ptr,
        memory_pages: memory_pages as u64,
        name,
        arch,
    })
}

/// Parse v5.0 blob header
fn parse_blob_header(blob: &[u8]) -> Result<(String, String, &[u8]), &'static str> {
    if blob.len() < 48 {
        return Err("blob too small");
    }

    // Verify magic
    if &blob[0..4] != b"EaM5" {
        return Err("invalid magic");
    }

    let format_version = blob[4];
    if format_version != 5 {
        return Err("unsupported format version");
    }

    let arch_code = blob[5];
    let name_len = blob[6] as usize;
    let _reserved = blob[7];

    if blob.len() < 40 + name_len {
        return Err("invalid name length");
    }

    let name =
        String::from_utf8(blob[8..8 + name_len].to_vec()).map_err(|_| "invalid utf8 name")?;

    let arch = match arch_code {
        1 => "aarch64",
        2 => "x86_64",
        _ => return Err("unknown architecture"),
    };

    // Payload starts after 40-byte header, ends before 8-byte integrity hash
    let payload_end = blob.len() - 8;
    if 40 > payload_end {
        return Err("invalid blob structure");
    }

    let payload = &blob[40..payload_end];

    // Verify integrity
    let mut hasher = Hasher::new();
    hasher.update(&blob[..payload_end]);
    let computed_hash = hasher.finalize();
    let stored_hash = &blob[payload_end..];

    if computed_hash.as_bytes()[..8] != *stored_hash {
        return Err("integrity check failed");
    }

    Ok((name, arch.to_string(), payload))
}

/// Generate salt for key derivation
pub fn generate_salt(muscle_index: usize, muscle_name: &str) -> MuscleSalt {
    let mut hasher = Hasher::new();
    hasher.update(&muscle_index.to_le_bytes());
    hasher.update(muscle_name.as_bytes());
    let hash = hasher.finalize();

    let mut salt = [0u8; 16];
    salt.copy_from_slice(&hash.as_bytes()[..16]);
    salt
}

/// Check if architecture is supported
fn is_architecture_supported(arch: &str) -> bool {
    // For now, support both - in production this would check current platform
    arch == "aarch64" || arch == "x86_64"
}

/// Calculate required pages for muscle
pub fn calculate_required_pages(size: usize) -> usize {
    (size + 4095) / 4096
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_salt_generation() {
        let salt1 = generate_salt(0, "test_muscle");
        let salt2 = generate_salt(0, "test_muscle");
        let salt3 = generate_salt(1, "test_muscle");

        // Same index and name should produce same salt
        assert_eq!(salt1, salt2);
        // Different index should produce different salt
        assert_ne!(salt1, salt3);
    }

    #[test]
    fn test_page_calculation() {
        assert_eq!(calculate_required_pages(0), 0);
        assert_eq!(calculate_required_pages(1), 1);
        assert_eq!(calculate_required_pages(4096), 1);
        assert_eq!(calculate_required_pages(4097), 2);
    }
}
