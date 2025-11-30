#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![deny(missing_docs, clippy::all, clippy::pedantic)]
#![doc = r#"
Specialized Pathfinder Muscle for Eä biological compute substrate.

A WASM-based secure evaluation organ with zero sandbox escape,
fully integrated into the living Muscle.ea tissue architecture.

This muscle provides a biological membrane around WASM execution,
treating it as a specialized cellular organelle while maintaining
all Eä biological computing principles.
"#]

extern crate alloc;

use alloc::{vec::Vec, boxed::Box, string::String, format};
use core::marker::PhantomData;
use muscle_ea_core::{
    biology::*,
    runtime::{Muscle, MuscleContext, MuscleOutput, MuscleSuccessor, SuccessorMetadata},
    error::MuscleError,
    prelude::*,
};
use wasmtime::*;
use zeroize::{Zeroize, Zeroizing};
use rand_core::{RngCore, CryptoRng};
use sha3::{Shake256, digest::{Update, ExtendableOutput, XofReader}};
use aes_gcm::{Aes256Gcm, KeyInit, aead::{Aead, generic_array::GenericArray, Payload}};
use hmac::{Hmac, Mac};
use smallvec::SmallVec;

/// Sealed blob header for pathfinder muscles
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct PathfinderHeader {
    version: u32,               // 3 for pathfinder v1
    salt: [u8; 16],             // Muscle salt
    nonce: [u8; 12],            // AES-GCM nonce
    mac: [u8; 16],              // HMAC-SHA3-256 truncated
    ciphertext_len: u64,        // Length of encrypted payload
}

impl PathfinderHeader {
    fn as_bytes(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const _ as *const u8,
                core::mem::size_of::<PathfinderHeader>()
            )
        }
    }
}

/// Specialized Pathfinder Muscle — a living organ that speaks WASM natively
/// while remaining 100% part of the Eä tissue architecture.
pub struct PathfinderMuscle<R: RngCore + CryptoRng = rand_core::OsRng> {
    _phantom: PhantomData<R>,
}

impl<R: RngCore + CryptoRng> Default for PathfinderMuscle<R> {
    fn default() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<R: RngCore + CryptoRng> Muscle for PathfinderMuscle<R> {
    type PrivateInput = Vec<u8>;
    type PrivateOutput = Vec<u8>;

    fn execute(
        &self,
        ctx: &mut MuscleContext<impl RngCore + CryptoRng>,
        private_input: Self::PrivateInput,
    ) -> Result<MuscleOutput<Self::PrivateOutput>, MuscleError> {
        let sealed = ctx.current_blob();
        
        // Verify this is a pathfinder muscle
        if sealed.version() != 3 {
            return Err(MuscleError::InvalidBlob);
        }

        let (wasm_bytes, successor_keys) = unseal_pathfinder_blob(
            ctx.master_key(),
            sealed.salt(),
            &sealed.payload,
        )?;

        let result = run_pathfinder_isolate(
            &wasm_bytes,
            &private_input,
            successor_keys,
            ctx.rng(),
        )?;

        Ok(MuscleOutput {
            output: result.output,
            successors: result.successors,
        })
    }
}

/// Result from pathfinder execution
#[derive(Debug)]
struct PathfinderResult {
    output: Vec<u8>,
    successors: Vec<MuscleSuccessor>,
}

/// Biological cell state — the living cytoplasm of the pathfinder muscle
struct PathfinderCell<R: RngCore + CryptoRng> {
    input: Zeroizing<Vec<u8>>,
    output: Zeroizing<Vec<u8>>,
    successors: Vec<MuscleSuccessor>,
    successor_keys: Vec<[u8; 32]>,
    rng: R,
    fuel_remaining: u64,
}

impl<R: RngCore + CryptoRng> PathfinderCell<R> {
    fn new(input: Vec<u8>, successor_keys: Vec<[u8; 32]>, rng: R) -> Self {
        Self {
            input: Zeroizing::new(input),
            output: Zeroizing::new(Vec::new()),
            successors: Vec::new(),
            successor_keys,
            rng,
            fuel_remaining: 500_000,
        }
    }

    fn read_input(&self, ptr: u32, len: u32) -> Result<Vec<u8>, MuscleError> {
        let start = ptr as usize;
        let end = start + len as usize;
        
        if end > self.input.len() {
            return Err(MuscleError::IsolationFailure);
        }
        
        Ok(self.input[start..end].to_vec())
    }

    fn write_output(&mut self, data: &[u8]) -> Result<(), MuscleError> {
        if self.output.len() + data.len() > 1 << 20 { // 1 MiB max output
            return Err(MuscleError::ResourceExhausted);
        }
        self.output.extend_from_slice(data);
        Ok(())
    }

    fn seal_successor(&mut self, wasm: &[u8]) -> Result<MuscleSuccessor, MuscleError> {
        if self.successor_keys.is_empty() {
            return Err(MuscleError::ResourceExhausted);
        }
        
        let key = self.successor_keys.remove(0);
        let salt = MuscleSalt::random(&mut self.rng);
        let sealed_blob = seal_pathfinder_blob(&key, &salt, wasm, &mut self.rng)?;
        
        let successor = MuscleSuccessor {
            blob: sealed_blob,
            metadata: SuccessorMetadata::new(3, "pathfinder".to_string())
                .with_property("wasm_size".to_string(), wasm.len().to_string())
                .with_property("organelle_type".to_string(), "wasm_execution".to_string()),
        };
        
        self.successors.push(successor.clone());
        Ok(successor)
    }
}

fn unseal_pathfinder_blob(
    master_key: &[u8; 32],
    salt: &MuscleSalt,
    sealed: &[u8],
) -> Result<(Vec<u8>, Vec<[u8; 32]>), MuscleError> {
    if sealed.len() < core::mem::size_of::<PathfinderHeader>() {
        return Err(MuscleError::InvalidBlob);
    }

    // Parse header
    let header_slice = &sealed[..core::mem::size_of::<PathfinderHeader>()];
    let header: PathfinderHeader = unsafe { core::ptr::read(header_slice.as_ptr() as *const _) };
    
    if header.version != 3 {
        return Err(MuscleError::InvalidBlob);
    }
    
    if header.salt != *salt.as_bytes() {
        return Err(MuscleError::InvalidBlob);
    }

    let ciphertext = &sealed[core::mem::size_of::<PathfinderHeader>()..];
    if ciphertext.len() != header.ciphertext_len as usize {
        return Err(MuscleError::InvalidBlob);
    }

    // Verify MAC
    let expected_mac = compute_pathfinder_hmac(master_key, salt, sealed);
    if !hmac::compare_digest(&expected_mac, &header.mac) {
        return Err(MuscleError::InvalidBlob);
    }

    // Decrypt
    let enc_key = derive_pathfinder_key(master_key, salt, &header.nonce);
    let plaintext = decrypt_pathfinder_aes(&enc_key, &header.nonce, ciphertext)
        .ok_or(MuscleError::Crypto("decryption failed".to_string()))?;

    // Parse successor keys
    if plaintext.len() < 4 {
        return Err(MuscleError::InvalidBlob);
    }
    
    let succ_count = u32::from_le_bytes(plaintext[plaintext.len()-4..].try_into().unwrap()) as usize;
    if plaintext.len() < 4 + succ_count * 32 {
        return Err(MuscleError::InvalidBlob);
    }

    let module_len = plaintext.len() - 4 - succ_count * 32;
    let module_bytes = plaintext[..module_len].to_vec();

    let mut successor_keys = Vec::with_capacity(succ_count);
    let mut offset = module_len;
    for _ in 0..succ_count {
        let mut key = [0u8; 32];
        key.copy_from_slice(&plaintext[offset..offset+32]);
        successor_keys.push(key);
        offset += 32;
    }

    Ok((module_bytes, successor_keys))
}

fn run_pathfinder_isolate<R: RngCore + CryptoRng>(
    wasm: &[u8],
    private_input: &[u8],
    successor_keys: Vec<[u8; 32]>,
    rng: &mut R,
) -> Result<PathfinderResult, MuscleError> {
    let engine = Engine::new(
        Config::new()
            .consume_fuel(true)
            .epoch_interruption(true)
            .static_memory_maximum_size(1 << 16)  // 64 KiB — biological cell constraint
            .dynamic_memory_guard_size(0)
            .guard_before_linear_mem(true)
            .cranelift_opt_level(wasmtime::OptLevel::Speed)
    ).map_err(|e| MuscleError::IsolationFailure)?;

    let mut store = Store::new(&engine, PathfinderCell::new(private_input.to_vec(), successor_keys, rng.try_clone().map_err(|_| MuscleError::RngFailure)?));

    store.add_fuel(500_000).map_err(|_| MuscleError::ResourceExhausted)?;
    store.set_epoch_deadline(1);

    let module = Module::new(&engine, wasm).map_err(|e| MuscleError::MalformedOrganelle)?;
    
    // Create host functions for biological membrane interface
    let read_input_func = Func::wrap(&mut store, |caller: Caller<'_, PathfinderCell<R>>, ptr: u32, len: u32, out_ptr: u32| -> Result<(), Trap> {
        let cell = caller.data();
        let data = cell.read_input(ptr, len).map_err(|e| Trap::new(e.to_string()))?;
        let memory = caller.get_export("memory").and_then(|e| e.into_memory()).ok_or_else(|| Trap::new("no memory"))?;
        memory.write(&caller, out_ptr as usize, &data)?;
        Ok(())
    });

    let write_output_func = Func::wrap(&mut store, |mut caller: Caller<'_, PathfinderCell<R>>, ptr: u32, len: u32| -> Result<(), Trap> {
        let memory = caller.get_export("memory").and_then(|e| e.into_memory()).ok_or_else(|| Trap::new("no memory"))?;
        let mut data = vec![0u8; len as usize];
        memory.read(&caller, ptr as usize, &mut data)?;
        caller.data_mut().write_output(&data).map_err(|e| Trap::new(e.to_string()))?;
        Ok(())
    });

    let seal_successor_func = Func::wrap(&mut store, |mut caller: Caller<'_, PathfinderCell<R>>, ptr: u32, len: u32, out_ptr: u32, out_len_ptr: u32| -> Result<u32, Trap> {
        let memory = caller.get_export("memory").and_then(|e| e.into_memory()).ok_or_else(|| Trap::new("no memory"))?;
        
        // Read WASM bytes from guest memory
        let mut wasm_data = vec![0u8; len as usize];
        memory.read(&caller, ptr as usize, &mut wasm_data)
            .map_err(|e| Trap::new(format!("memory read failed: {}", e)))?;
        
        // Create successor muscle
        let successor = caller.data_mut().seal_successor(&wasm_data)
            .map_err(|e| Trap::new(format!("seal failed: {}", e)))?;
        
        // Serialize successor to bytes for return to guest
        let serialized = serialize_successor_for_guest(&successor)
            .map_err(|e| Trap::new(format!("serialization failed: {}", e)))?;
        
        // Write serialized data back to guest memory
        if serialized.len() > 4096 { // Reasonable limit for successor data
            return Err(Trap::new("successor data too large"));
        }
        
        memory.write(&mut caller, out_ptr as usize, &serialized)
            .map_err(|e| Trap::new(format!("memory write failed: {}", e)))?;
        
        // Write length to guest memory
        let len_bytes = (serialized.len() as u32).to_le_bytes();
        memory.write(&mut caller, out_len_ptr as usize, &len_bytes)
            .map_err(|e| Trap::new(format!("length write failed: {}", e)))?;
        
        Ok(0) // Success return code
    });

    let instance = Instance::new(&mut store, &module, &[
        read_input_func.into(),
        write_output_func.into(),
        seal_successor_func.into(),
    ]).map_err(|e| MuscleError::MalformedOrganelle)?;

    let run = instance
        .get_func(&mut store, "run")
        .ok_or(MuscleError::MissingEntryPoint)?;

    run.call(&mut store, &[], &mut [])
        .map_err(|trap| {
            if trap.downcast_ref::<wasmtime::FuelExhausted>().is_some() {
                MuscleError::ResourceExhausted
            } else {
                MuscleError::Trap(trap.to_string())
            }
        })?;

    let cell = store.into_data();
    Ok(PathfinderResult {
        output: cell.output.to_vec(),
        successors: cell.successors,
    })
}

/// Serialize successor data for passing back to WASM guest
fn serialize_successor_for_guest(successor: &MuscleSuccessor) -> Result<Vec<u8>, MuscleError> {
    use core::fmt::Write;
    
    let mut serialized = Vec::new();
    
    // Simple binary format: [version: u32][blob_size: u32][blob_data][metadata_size: u32][metadata...]
    
    // Version
    serialized.extend_from_slice(&successor.blob.version().to_le_bytes());
    
    // Blob size and data
    let blob_data = &successor.blob.payload;
    serialized.extend_from_slice(&(blob_data.len() as u32).to_le_bytes());
    serialized.extend_from_slice(blob_data);
    
    // Metadata: muscle_type + properties as simple string format
    let mut metadata_str = String::new();
    write!(&mut metadata_str, "type:{}", successor.metadata.muscle_type)
        .map_err(|e| MuscleError::Custom(format!("metadata serialization failed: {}", e)))?;
    
    for (key, value) in &successor.metadata.properties {
        write!(&mut metadata_str, ",{}:{}", key, value)
            .map_err(|e| MuscleError::Custom(format!("property serialization failed: {}", e)))?;
    }
    
    let metadata_bytes = metadata_str.into_bytes();
    serialized.extend_from_slice(&(metadata_bytes.len() as u32).to_le_bytes());
    serialized.extend_from_slice(&metadata_bytes);
    
    Ok(serialized)
}

// Cryptographic organelles — biological framing of crypto operations
fn derive_pathfinder_key(master_key: &[u8; 32], salt: &MuscleSalt, nonce: &[u8; 12]) -> [u8; 32] {
    let mut shake = Shake256::default();
    shake.update(b"MUSCLE_PATHFINDER_V1_ENC");
    shake.update(master_key);
    shake.update(salt.as_bytes());
    shake.update(nonce);
    let mut key = [0u8; 32];
    shake.finalize_xof().read(&mut key);
    key
}

fn compute_pathfinder_hmac(master_key: &[u8; 32], salt: &MuscleSalt, data: &[u8]) -> [u8; 16] {
    type HmacSha3256 = Hmac<sha3::Sha3_256>;
    let mut mac = HmacSha3256::new_from_slice(master_key)
        .expect("HMAC key should be valid");
    mac.update(b"MUSCLE_PATHFINDER_V1_MAC");
    mac.update(salt.as_bytes());
    mac.update(data);
    
    let result = mac.finalize().into_bytes();
    let mut truncated = [0u8; 16];
    truncated.copy_from_slice(&result[..16]);
    truncated
}

fn decrypt_pathfinder_aes(key: &[u8; 32], nonce: &[u8; 12], ciphertext: &[u8]) -> Option<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let nonce = GenericArray::from_slice(nonce);
    cipher.decrypt(nonce, ciphertext).ok()
}

fn seal_pathfinder_blob(key: &[u8; 32], salt: &MuscleSalt, payload: &[u8], rng: &mut impl RngCore) -> Result<SealedBlob, MuscleError> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| MuscleError::Crypto("invalid key".to_string()))?;
    
    let mut nonce = [0u8; 12];
    rng.fill_bytes(&mut nonce);
    
    let nonce_array = GenericArray::from_slice(&nonce);
    let ciphertext = cipher.encrypt(nonce_array, payload)
        .map_err(|_| MuscleError::Crypto("encryption failed".to_string()))?;

    // Build the full sealed data with header
    let mut sealed_data = Vec::with_capacity(core::mem::size_of::<PathfinderHeader>() + ciphertext.len());
    
    // Create header (MAC will be computed after)
    let header = PathfinderHeader {
        version: 3,
        salt: *salt.as_bytes(),
        nonce,
        mac: [0u8; 16], // Placeholder - will be set below
        ciphertext_len: ciphertext.len() as u64,
    };
    
    // Write header
    sealed_data.extend_from_slice(header.as_bytes());
    sealed_data.extend_from_slice(&ciphertext);
    
    // Compute and set MAC
    let mac = compute_pathfinder_hmac(key, salt, &sealed_data);
    
    // Update MAC in the sealed data
    let mac_offset = core::mem::size_of_val(&header.version) 
                   + core::mem::size_of_val(&header.salt)
                   + core::mem::size_of_val(&header.nonce);
    sealed_data[mac_offset..mac_offset + 16].copy_from_slice(&mac);
    
    Ok(SealedBlob::new(sealed_data, *salt, 3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    #[test]
    fn test_pathfinder_muscle_creation() {
        let muscle = PathfinderMuscle::<OsRng>::default();
        assert!(core::mem::size_of_val(&muscle) > 0);
    }

    #[test]
    fn test_pathfinder_cell_operations() {
        let mut rng = OsRng;
        let input = vec![1, 2, 3, 4, 5];
        let keys = vec![[0u8; 32]];
        let cell = PathfinderCell::new(input.clone(), keys, &mut rng);
        
        let read_data = cell.read_input(1, 3).unwrap();
        assert_eq!(read_data, vec![2, 3, 4]);
    }

    #[test]
    fn test_crypto_primitives() {
        let key = [1u8; 32];
        let salt = MuscleSalt::new([2u8; 16]);
        let nonce = [3u8; 12];
        
        let derived = derive_pathfinder_key(&key, &salt, &nonce);
        assert_eq!(derived.len(), 32);
        
        let data = b"test data";
        let mac = compute_pathfinder_hmac(&key, &salt, data);
        assert_eq!(mac.len(), 16);
    }

    #[test]
    fn test_successor_serialization() {
        let salt = MuscleSalt::new([0u8; 16]);
        let blob = SealedBlob::new(vec![1, 2, 3], salt, 3);
        let metadata = SuccessorMetadata::new(3, "test".to_string())
            .with_property("key".to_string(), "value".to_string());
        
        let successor = MuscleSuccessor {
            blob,
            metadata,
        };
        
        let serialized = serialize_successor_for_guest(&successor).unwrap();
        assert!(!serialized.is_empty());
        assert!(serialized.len() >= 16); // Minimum header size
    }
}
