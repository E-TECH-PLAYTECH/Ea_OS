//! Biological primitives and types for the Eä ecosystem
//!
//! Defines the fundamental biological structures that make up
//! the cellular architecture of Eä muscles.

use core::fmt;
use zeroize::Zeroize;

/// Salt for muscle derivation - ensures unique encryption per muscle
#[derive(Clone, PartialEq, Eq, Hash, Zeroize)]
#[zeroize(drop)]
pub struct MuscleSalt([u8; 16]);

impl MuscleSalt {
    /// Create a new muscle salt from bytes
    pub fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Generate a random muscle salt
    pub fn random<R: rand_core::RngCore + rand_core::CryptoRng>(rng: &mut R) -> Self {
        let mut bytes = [0u8; 16];
        rng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// Get the salt as bytes
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl AsRef<[u8]> for MuscleSalt {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for MuscleSalt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MuscleSalt({})", hex::encode(self.0))
    }
}

/// Sealed blob containing an encrypted muscle
#[derive(Clone, Zeroize)]
#[zeroize(drop)]
pub struct SealedBlob {
    /// The encrypted payload
    pub payload: alloc::vec::Vec<u8>,
    /// The salt used for this specific muscle
    pub salt: MuscleSalt,
    /// Version information
    pub version: u32,
}

impl SealedBlob {
    /// Create a new sealed blob
    pub fn new(payload: alloc::vec::Vec<u8>, salt: MuscleSalt, version: u32) -> Self {
        Self {
            payload,
            salt,
            version,
        }
    }

    /// Get the salt for this blob
    pub fn salt(&self) -> &MuscleSalt {
        &self.salt
    }

    /// Get the version
    pub fn version(&self) -> u32 {
        self.version
    }
}

impl fmt::Debug for SealedBlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SealedBlob {{ version: {}, salt: {}, payload: {} bytes }}",
            self.version,
            hex::encode(self.salt.0),
            self.payload.len()
        )
    }
}

/// Key for deriving successor muscles
#[derive(Clone, PartialEq, Eq, Zeroize)]
#[zeroize(drop)]
pub struct SuccessorKey([u8; 32]);

impl SuccessorKey {
    /// Create a new successor key from bytes
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Generate a random successor key
    pub fn random<R: rand_core::RngCore + rand_core::CryptoRng>(rng: &mut R) -> Self {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// Get the key as bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl AsRef<[u8]> for SuccessorKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SuccessorKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SuccessorKey({}...)", hex::encode(&self.0[..8]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    #[test]
    fn test_muscle_salt_operations() {
        let salt1 = MuscleSalt::random(&mut OsRng);
        let salt2 = MuscleSalt::random(&mut OsRng);

        assert_ne!(salt1.as_bytes(), salt2.as_bytes());
        assert_eq!(salt1.as_bytes().len(), 16);
    }

    #[test]
    fn test_successor_key_operations() {
        let key1 = SuccessorKey::random(&mut OsRng);
        let key2 = SuccessorKey::random(&mut OsRng);

        assert_ne!(key1.as_bytes(), key2.as_bytes());
        assert_eq!(key1.as_bytes().len(), 32);
    }

    #[test]
    fn test_sealed_blob_creation() {
        let salt = MuscleSalt::random(&mut OsRng);
        let payload = alloc::vec![1, 2, 3, 4, 5];
        let blob = SealedBlob::new(payload.clone(), salt.clone(), 1);

        assert_eq!(blob.version(), 1);
        assert_eq!(blob.salt(), &salt);
        assert_eq!(blob.payload, payload);
    }
}
