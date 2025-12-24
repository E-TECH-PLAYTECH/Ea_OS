//! Runtime traits and types for Eä muscle execution
//!
//! Defines the interface between muscles and the biological runtime environment.

use crate::biology::SealedBlob;
use crate::error::MuscleError;
use core::fmt;
use rand_core::{CryptoRng, RngCore};
use zeroize::Zeroizing;

/// Context provided to muscles during execution
pub struct MuscleContext<R: RngCore + CryptoRng> {
    /// The current muscle's sealed blob
    current_blob: SealedBlob,
    /// Master key for cryptographic operations
    master_key: Zeroizing<[u8; 32]>,
    /// Random number generator for the execution
    rng: R,
}

impl<R: RngCore + CryptoRng> MuscleContext<R> {
    /// Create a new muscle context
    pub fn new(current_blob: SealedBlob, master_key: [u8; 32], rng: R) -> Self {
        Self {
            current_blob,
            master_key: Zeroizing::new(master_key),
            rng,
        }
    }

    /// Get the current muscle's sealed blob
    pub fn current_blob(&self) -> &SealedBlob {
        &self.current_blob
    }

    /// Get the master key
    pub fn master_key(&self) -> &[u8; 32] {
        &self.master_key
    }

    /// Get mutable access to the RNG
    pub fn rng(&mut self) -> &mut R {
        &mut self.rng
    }
}

impl<R: RngCore + CryptoRng> fmt::Debug for MuscleContext<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MuscleContext {{ blob_version: {}, ... }}",
            self.current_blob.version()
        )
    }
}

/// Output from muscle execution
#[derive(Debug, Clone)]
pub struct MuscleOutput<T> {
    /// The private output data
    pub output: T,
    /// Successor muscles produced during execution
    pub successors: alloc::vec::Vec<MuscleSuccessor>,
}

/// A successor muscle produced during execution
#[derive(Debug, Clone)]
pub struct MuscleSuccessor {
    /// The sealed blob for the successor
    pub blob: SealedBlob,
    /// Metadata about the successor
    pub metadata: SuccessorMetadata,
}

/// Metadata about a successor muscle
#[derive(Debug, Clone)]
pub struct SuccessorMetadata {
    /// Version of the successor
    pub version: u32,
    /// Type identifier for the successor
    pub muscle_type: alloc::string::String,
    /// Additional properties
    pub properties: alloc::collections::BTreeMap<alloc::string::String, alloc::string::String>,
}

impl SuccessorMetadata {
    /// Create new successor metadata
    pub fn new(version: u32, muscle_type: alloc::string::String) -> Self {
        Self {
            version,
            muscle_type,
            properties: alloc::collections::BTreeMap::new(),
        }
    }

    /// Add a property to the metadata
    pub fn with_property(
        mut self,
        key: alloc::string::String,
        value: alloc::string::String,
    ) -> Self {
        self.properties.insert(key, value);
        self
    }
}

/// Core trait that all muscles must implement
///
/// Generic over the RNG type `R` to allow different randomness sources
/// while maintaining object safety. Use `OsRng` for production and
/// deterministic RNGs for testing.
pub trait Muscle<R: RngCore + CryptoRng> {
    /// Type of private input data
    type PrivateInput;
    /// Type of private output data
    type PrivateOutput;

    /// Execute the muscle with the given context and input
    fn execute(
        &self,
        ctx: &mut MuscleContext<R>,
        private_input: Self::PrivateInput,
    ) -> Result<MuscleOutput<Self::PrivateOutput>, MuscleError>;
}

/// Blanket implementation for boxed muscles
///
/// Allows `Box<dyn Muscle<R, ...>>` to be used as a `Muscle<R>` directly,
/// enabling dynamic dispatch while preserving the RNG type guarantee.
impl<R: RngCore + CryptoRng, M: Muscle<R> + ?Sized> Muscle<R> for alloc::boxed::Box<M> {
    type PrivateInput = M::PrivateInput;
    type PrivateOutput = M::PrivateOutput;

    fn execute(
        &self,
        ctx: &mut MuscleContext<R>,
        private_input: Self::PrivateInput,
    ) -> Result<MuscleOutput<Self::PrivateOutput>, MuscleError> {
        (**self).execute(ctx, private_input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biology::MuscleSalt;
    use rand_core::OsRng;

    struct TestMuscle;

    impl<R: RngCore + CryptoRng> Muscle<R> for TestMuscle {
        type PrivateInput = alloc::vec::Vec<u8>;
        type PrivateOutput = alloc::vec::Vec<u8>;

        fn execute(
            &self,
            _ctx: &mut MuscleContext<R>,
            input: Self::PrivateInput,
        ) -> Result<MuscleOutput<Self::PrivateOutput>, MuscleError> {
            Ok(MuscleOutput {
                output: input,
                successors: alloc::vec![],
            })
        }
    }

    #[test]
    fn test_muscle_trait_implementation() {
        let muscle = TestMuscle;
        let blob = SealedBlob::new(alloc::vec![], MuscleSalt::new([0; 16]), 1);
        let master_key = [0u8; 32];
        let mut ctx = MuscleContext::new(blob, master_key, OsRng);

        let input = alloc::vec![1, 2, 3];
        let result = muscle.execute(&mut ctx, input.clone()).unwrap();

        assert_eq!(result.output, input);
        assert!(result.successors.is_empty());
    }

    #[test]
    fn test_boxed_muscle() {
        // The Muscle trait is now generic over RNG type, making this object-safe.
        // By specifying OsRng as the RNG type, we can create a trait object.
        let muscle: alloc::boxed::Box<
            dyn Muscle<OsRng, PrivateInput = alloc::vec::Vec<u8>, PrivateOutput = alloc::vec::Vec<u8>>,
        > = alloc::boxed::Box::new(TestMuscle);

        let blob = SealedBlob::new(alloc::vec![], MuscleSalt::new([0; 16]), 1);
        let master_key = [0u8; 32];
        let mut ctx = MuscleContext::new(blob, master_key, OsRng);

        let input = alloc::vec![4, 5, 6];
        let result = muscle.execute(&mut ctx, input.clone()).unwrap();

        assert_eq!(result.output, input);
    }
}
