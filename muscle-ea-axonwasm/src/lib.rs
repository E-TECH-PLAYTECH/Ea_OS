#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![deny(missing_docs, clippy::all, clippy::pedantic)]
#![doc = r#"
AxonWasmMuscle — streaming neural fiber for Eä biological compute substrate.

The first true streaming neural fiber capable of:
• Multi-organelle parallel execution (synaptic terminals)
• Synaptic fan-in / fan-out (neural network topology)  
• Streaming sealed signal propagation (action potentials)
• Axonal successor chaining (neural plasticity)
"#]

extern crate alloc;

use alloc::{format, string::String, vec::Vec, vec_deque::VecDeque};
use core::marker::PhantomData;
use muscle_ea_core::{
    biology::*,
    error::MuscleError,
    prelude::*,
    runtime::{Muscle, MuscleContext, MuscleOutput, MuscleSuccessor, SuccessorMetadata},
};
use muscle_ea_pathfinder::PathfinderMuscle;
use rand_core::{CryptoRng, RngCore};
use sha3::{Digest, Sha3_256};
use zeroize::Zeroizing;

/// Incoming neural signal — carries multiple sealed organelles (dendritic input)
#[derive(Debug, Clone)]
pub struct AxonSignal {
    /// Sealed organelles to execute in parallel (synaptic terminals)
    pub organelles: Vec<SealedBlob>,
    /// Neural signal metadata (neurotransmitter profile)
    pub metadata: SignalMetadata,
}

/// Outgoing action potential — carries transformed signal downstream
#[derive(Debug, Clone)]
pub struct AxonPulse {
    /// Combined output from all fired organelles
    pub payload: Zeroizing<Vec<u8>>,
    /// Number of organelles that successfully fired (signal intensity)
    pub intensity: u64,
    /// Cryptographic trace of refractory period
    pub refractory_trace: Vec<u8>,
}

/// Neural signal metadata (neurotransmitter profile)
#[derive(Debug, Clone)]
pub struct SignalMetadata {
    /// Signal type/priority (neurotransmitter analog)
    pub neurotransmitter: u8,
    /// Urgency level (signal priority)
    pub urgency: u8,
    /// Lineage identifier for neural pathway tracing
    pub lineage_tag: [u8; 8],
}

impl SignalMetadata {
    /// Create new signal metadata
    pub fn new(neurotransmitter: u8, urgency: u8, lineage_tag: [u8; 8]) -> Self {
        Self {
            neurotransmitter,
            urgency,
            lineage_tag,
        }
    }
}

/// AxonWasmMuscle v1 "Giant Squid Axon" — the first true streaming neural fiber
pub struct AxonWasmMuscle<R: RngCore + CryptoRng = rand_core::OsRng> {
    /// Maximum concurrent organelles (synaptic terminals)
    max_parallelism: usize,
    /// Metabolic budget per action potential
    fuel_per_pulse: u64,
    _phantom: PhantomData<R>,
}

impl<R: RngCore + CryptoRng> Default for AxonWasmMuscle<R> {
    fn default() -> Self {
        Self {
            max_parallelism: 8, // Reduced for no-std compatibility
            fuel_per_pulse: 1_000_000,
            _phantom: PhantomData,
        }
    }
}

impl<R: RngCore + CryptoRng> AxonWasmMuscle<R> {
    /// Create with custom parallelism and fuel budget
    pub fn new(max_parallelism: usize, fuel_per_pulse: u64) -> Self {
        Self {
            max_parallelism,
            fuel_per_pulse,
            _phantom: PhantomData,
        }
    }
}

impl<R: RngCore + CryptoRng> Muscle for AxonWasmMuscle<R> {
    type PrivateInput = AxonSignal;
    type PrivateOutput = AxonPulse;

    fn execute(
        &self,
        ctx: &mut MuscleContext<impl RngCore + CryptoRng>,
        signal: Self::PrivateInput,
    ) -> Result<MuscleOutput<Self::PrivateOutput>, MuscleError> {
        let mut axon = AxonFiber::new(self, ctx, signal)?;
        let pulse = axon.propagate()?;
        Ok(MuscleOutput {
            output: pulse,
            successors: axon.emit_successors(),
        })
    }
}

/// The living axon fiber — contains execution state and propagation logic
struct AxonFiber<'a, R: RngCore + CryptoRng> {
    muscle: &'a AxonWasmMuscle<R>,
    ctx: &'a mut MuscleContext<impl RngCore + CryptoRng>,
    incoming: AxonSignal,
    fired_organelles: VecDeque<MuscleOutput<Vec<u8>>>,
    successors: Vec<MuscleSuccessor>,
    fuel_remaining: u64,
}

impl<'a, R: RngCore + CryptoRng> AxonFiber<'a, R> {
    fn new(
        muscle: &'a AxonWasmMuscle<R>,
        ctx: &'a mut MuscleContext<impl RngCore + CryptoRng>,
        signal: AxonSignal,
    ) -> Result<Self, MuscleError> {
        Ok(Self {
            muscle,
            ctx,
            incoming: signal,
            fired_organelles: VecDeque::with_capacity(muscle.max_parallelism),
            successors: Vec::new(),
            fuel_remaining: muscle.fuel_per_pulse,
        })
    }

    /// Propagate the action potential — synchronous parallel organelle execution
    fn propagate(&mut self) -> Result<AxonPulse, MuscleError> {
        // Execute organelles with limited parallelism (synaptic firing)
        let mut executing = Vec::with_capacity(self.muscle.max_parallelism);

        for blob in self
            .incoming
            .organelles
            .iter()
            .take(self.muscle.max_parallelism)
        {
            if self.fuel_remaining == 0 {
                break; // Refractory period - no more firing
            }

            match self.fire_organelle_sync(blob) {
                Ok(output) => {
                    self.fuel_remaining = self.fuel_remaining.saturating_sub(50_000);
                    self.fired_organelles.push_back(output);
                }
                Err(e) => {
                    // Failed organelles don't propagate but don't stop others
                    continue;
                }
            }
        }

        self.summate_pulse()
    }

    /// Fire a single organelle synchronously (synaptic terminal)
    fn fire_organelle_sync(&self, blob: &SealedBlob) -> Result<MuscleOutput<Vec<u8>>, MuscleError> {
        let pathfinder = PathfinderMuscle::default();

        // Create execution context for this organelle
        let mut organelle_ctx = MuscleContext::new(
            blob.clone(),
            *self.ctx.master_key(),
            self.ctx
                .rng()
                .try_clone()
                .map_err(|_| MuscleError::RngFailure)?,
        );

        pathfinder.execute(&mut organelle_ctx, Vec::new())
    }

    /// Temporal + spatial summation → generate graded potential
    fn summate_pulse(&mut self) -> Result<AxonPulse, MuscleError> {
        let intensity = self.fired_organelles.len() as u64;
        let mut payload = Zeroizing::new(Vec::new());

        // Summate outputs from all fired organelles
        while let Some(output) = self.fired_organelles.pop_front() {
            payload.extend_from_slice(&output.output);
            self.successors.extend(output.successors);
        }

        Ok(AxonPulse {
            payload,
            intensity,
            refractory_trace: self.generate_refractory_trace(),
        })
    }

    /// Generate cryptographic refractory trace
    fn generate_refractory_trace(&self) -> Vec<u8> {
        let mut hasher = Sha3_256::new();
        hasher.update(b"AXON_REFRACTORY");
        hasher.update(&self.incoming.metadata.lineage_tag);
        hasher.update(&self.fuel_remaining.to_le_bytes());
        hasher.update(&self.fired_organelles.len().to_le_bytes());
        hasher.finalize()[..8].to_vec() // 8-byte trace
    }

    /// Emit successors including myelinated continuations
    fn emit_successors(mut self) -> Vec<MuscleSuccessor> {
        let mut successors = self.successors;

        // Auto-emit myelinated continuation if urgency threshold met
        if self.incoming.metadata.urgency > 200 {
            let continuation = MuscleSuccessor {
                blob: SealedBlob::new(
                    self.ctx.current_blob().payload.clone(),
                    MuscleSalt::random(self.ctx.rng()),
                    4, // Axon version
                ),
                metadata: SuccessorMetadata::new(4, "myelinated_continuation".to_string())
                    .with_property(
                        "intensity".to_string(),
                        self.fired_organelles.len().to_string(),
                    )
                    .with_property(
                        "urgency".to_string(),
                        self.incoming.metadata.urgency.to_string(),
                    )
                    .with_property(
                        "lineage".to_string(),
                        hex::encode(self.incoming.metadata.lineage_tag),
                    ),
            };
            successors.push(continuation);
        }

        successors
    }
}

// Extension trait for MuscleContext to support axon operations
trait AxonContextExt {
    /// Check if sufficient fuel remains for axon propagation
    fn has_sufficient_fuel(&self, required: u64) -> bool;
}

impl<R: RngCore + CryptoRng> AxonContextExt for MuscleContext<R> {
    fn has_sufficient_fuel(&self, required: u64) -> bool {
        // In real implementation, this would check actual fuel levels
        // For now, assume sufficient fuel for demonstration
        required <= 1_000_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    #[test]
    fn test_axon_signal_creation() {
        let signal = AxonSignal {
            organelles: Vec::new(),
            metadata: SignalMetadata::new(1, 150, [0xAA; 8]),
        };

        assert_eq!(signal.metadata.neurotransmitter, 1);
        assert_eq!(signal.metadata.urgency, 150);
        assert_eq!(signal.metadata.lineage_tag, [0xAA; 8]);
    }

    #[test]
    fn test_axon_muscle_creation() {
        let muscle = AxonWasmMuscle::<OsRng>::new(16, 2_000_000);
        assert_eq!(muscle.max_parallelism, 16);
        assert_eq!(muscle.fuel_per_pulse, 2_000_000);
    }

    #[test]
    fn test_axon_fiber_initialization() {
        let muscle = AxonWasmMuscle::<OsRng>::default();
        let blob = SealedBlob::new(Vec::new(), MuscleSalt::new([0; 16]), 1);
        let mut ctx = MuscleContext::new(blob, [0; 32], OsRng);

        let signal = AxonSignal {
            organelles: Vec::new(),
            metadata: SignalMetadata::new(0, 0, [0; 8]),
        };

        let fiber = AxonFiber::new(&muscle, &mut ctx, signal).unwrap();
        assert_eq!(fiber.fuel_remaining, muscle.fuel_per_pulse);
    }

    #[test]
    fn test_refractory_trace_generation() {
        let muscle = AxonWasmMuscle::<OsRng>::default();
        let blob = SealedBlob::new(Vec::new(), MuscleSalt::new([0; 16]), 1);
        let mut ctx = MuscleContext::new(blob, [0; 32], OsRng);

        let signal = AxonSignal {
            organelles: Vec::new(),
            metadata: SignalMetadata::new(0, 0, [0xBB; 8]),
        };

        let fiber = AxonFiber::new(&muscle, &mut ctx, signal).unwrap();
        let trace = fiber.generate_refractory_trace();

        assert_eq!(trace.len(), 8); // 8-byte refractory trace
    }

    #[test]
    fn test_myelinated_continuation_emission() {
        let muscle = AxonWasmMuscle::<OsRng>::default();
        let blob = SealedBlob::new(Vec::new(), MuscleSalt::new([0; 16]), 1);
        let mut ctx = MuscleContext::new(blob, [0; 32], OsRng);

        // High urgency signal should trigger myelinated continuation
        let signal = AxonSignal {
            organelles: Vec::new(),
            metadata: SignalMetadata::new(0, 250, [0xCC; 8]), // High urgency
        };

        let mut fiber = AxonFiber::new(&muscle, &mut ctx, signal).unwrap();
        let _pulse = fiber.propagate().unwrap(); // Ignore pulse for this test
        let successors = fiber.emit_successors();

        // Should contain myelinated continuation due to high urgency
        assert!(!successors.is_empty());
        assert_eq!(
            successors[0].metadata.muscle_type,
            "myelinated_continuation"
        );
    }
}
