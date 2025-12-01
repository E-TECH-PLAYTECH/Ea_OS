#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![deny(missing_docs, clippy::all, clippy::pedantic)]
#![doc = r#"
DendriteWasmMuscle — dendritic integrator for Eä neural compute substrate.

The first true dendritic integrator performing:
• Spatial + temporal summation (neural integration)
• Persistent synaptic weights (long-term potentiation)  
• Hebbian plasticity (cells that fire together, wire together)
• Graded potential emission (post-synaptic response)
"#]

extern crate alloc;

use alloc::{vec::Vec, collections::BTreeMap, format, string::String};
use core::marker::PhantomData;
use muscle_ea_core::{
    biology::*,
    runtime::{Muscle, MuscleContext, MuscleOutput, MuscleSuccessor, SuccessorMetadata},
    error::MuscleError,
    crypto::MuscleSalt,
};
use muscle_ea_axonwasm::AxonPulse;
use sha3::{Digest, Sha3_256};
use zeroize::Zeroizing;
use rand_core::{RngCore, CryptoRng};
use hex;

/// DendriteWasmMuscle v1 "Purkinje Cell" — the first true dendritic integrator
pub struct DendriteWasmMuscle<R: RngCore + CryptoRng = rand_core::OsRng> {
    /// Maximum number of synaptic inputs (dendritic spines)
    max_synapses: usize,
    /// Time window for temporal integration (in integration cycles)
    temporal_window: u64,
    /// Learning rate (α in Hebbian rule)
    learning_rate: f32,
    _phantom: PhantomData<R>,
}

impl<R: RngCore + CryptoRng> Default for DendriteWasmMuscle<R> {
    fn default() -> Self {
        Self {
            max_synapses: 8192,        // Purkinje cells have ~200k spines — we start modest
            temporal_window: 8,        // 8-cycle recent history
            learning_rate: 0.01,       // Classic Hebbian α
            _phantom: PhantomData,
        }
    }
}

impl<R: RngCore + CryptoRng> DendriteWasmMuscle<R> {
    /// Create with custom integration parameters
    pub fn new(max_synapses: usize, temporal_window: u64, learning_rate: f32) -> Self {
        Self {
            max_synapses,
            temporal_window,
            learning_rate,
            _phantom: PhantomData,
        }
    }
}

/// Input: Vector of incoming action potentials from many axons
pub type DendriticInput = Vec<AxonPulse>;

/// Output: Single graded potential + learning trace
#[derive(Debug, Clone)]
pub struct GradedPotential {
    /// Summated membrane potential (post-synaptic response)
    pub voltage: f32,
    /// Raw concatenated payload from contributing pulses
    pub payload: Zeroizing<Vec<u8>>,
    /// Number of contributing synapses
    pub active_inputs: u64,
    /// Cryptographic hash of input pattern (for Hebbian key)
    pub pattern_hash: [u8; 32],
}

impl<R: RngCore + CryptoRng> Muscle for DendriteWasmMuscle<R> {
    type PrivateInput = DendriticInput;
    type PrivateOutput = GradedPotential;

    fn execute(
        &self,
        ctx: &mut MuscleContext<impl RngCore + CryptoRng>,
        inputs: Self::PrivateInput,
    ) -> Result<MuscleOutput<Self::PrivateOutput>, MuscleError> {
        let mut dendrite = Dendrite::new(self, ctx, inputs)?;
        let potential = dendrite.integrate_and_fire()?;
        let successors = dendrite.emit_hebbian_successors(potential.pattern_hash)?;
        Ok(MuscleOutput {
            output: potential,
            successors,
        })
    }
}

/// The living dendrite — contains synaptic weights and integration logic
struct Dendrite<'a, R: RngCore + CryptoRng> {
    muscle: &'a DendriteWasmMuscle<R>,
    ctx: &'a mut MuscleContext<impl RngCore + CryptoRng>,
    inputs: DendriticInput,
    /// Synaptic weights: lineage_tag → weight (f32 stored as u32 via fixed-point)
    weights: BTreeMap<[u8; 8], u32>,
    /// Recent activation history for temporal summation
    recent_activity: BTreeMap<[u8; 8], (u64, f32)>, // (timestamp, contribution)
    current_tick: u64,
    integration_count: u64,
}

impl<'a, R: RngCore + CryptoRng> Dendrite<'a, R> {
    fn new(
        muscle: &'a DendriteWasmMuscle<R>,
        ctx: &'a mut MuscleContext<impl RngCore + CryptoRng>,
        inputs: DendriticInput,
    ) -> Result<Self, MuscleError> {
        let mut dendrite = Self {
            muscle,
            ctx,
            inputs: inputs.into_iter().take(muscle.max_synapses).collect(),
            weights: BTreeMap::new(),
            recent_activity: BTreeMap::new(),
            current_tick: 0, // Start integration cycle
            integration_count: 0,
        };
        dendrite.load_persistent_weights()?;
        Ok(dendrite)
    }

    /// Load synaptic weights from simulated persistent storage
    fn load_persistent_weights(&mut self) -> Result<(), MuscleError> {
        // In a real implementation, this would load from:
        // - Previous execution state
        // - External weight storage  
        // - Inherited from parent muscle
        // For now, initialize with uniform weights
        for pulse in &self.inputs {
            if pulse.refractory_trace.len() >= 8 {
                let mut tag = [0u8; 8];
                tag.copy_from_slice(&pulse.refractory_trace[..8]);
                self.weights.entry(tag).or_insert(1000); // Default weight = 1.0
            }
        }
        Ok(())
    }

    /// Spatial + temporal summation with Hebbian plasticity
    fn integrate_and_fire(&mut self) -> Result<GradedPotential, MuscleError> {
        let mut voltage: f32 = 0.0;
        let mut payload = Zeroizing::new(Vec::new());
        let mut active_inputs: u64 = 0;
        let mut pattern = Sha3_256::new();

        // Process each incoming pulse (synaptic input)
        for pulse in &self.inputs {
            // Extract lineage tag from refractory trace
            let tag = if pulse.refractory_trace.len() >= 8 {
                let mut tag_arr = [0u8; 8];
                tag_arr.copy_from_slice(&pulse.refractory_trace[..8]);
                tag_arr
            } else {
                [0u8; 8] // Default tag for untraceable inputs
            };

            pattern.update(&tag);

            // Spatial weighting with synaptic strength
            let weight = self.weights.get(&tag).map(|&w| w as f32 / 1000.0).unwrap_or(1.0);
            let contribution = pulse.intensity as f32 * weight;
            voltage += contribution;
            active_inputs += pulse.intensity;

            // Temporal summation (boost recent activity - NMDA-like)
            if let Some((last_tick, last_contrib)) = self.recent_activity.get(&tag) {
                if self.current_tick.saturating_sub(*last_tick) <= self.muscle.temporal_window {
                    voltage += last_contrib * 0.3; // NMDA-like temporal boost
                }
            }

            // Accumulate payload and update activity history
            payload.extend_from_slice(&pulse.payload);
            self.recent_activity.insert(tag, (self.current_tick, contribution));
        }

        let pattern_hash = pattern.finalize().into();
        self.integration_count += 1;
        self.current_tick += 1;

        Ok(GradedPotential {
            voltage,
            payload,
            active_inputs,
            pattern_hash,
        })
    }

    /// Emit successors with Hebbian weight updates
    fn emit_hebbian_successors(&self, pattern_hash: [u8; 32]) -> Result<Vec<MuscleSuccessor>, MuscleError> {
        let mut successors = Vec::new();

        // Apply Hebbian learning: strengthen weights that contributed
        for (tag, &weight_fixed) in &self.weights {
            let weight = weight_fixed as f32 / 1000.0;
            
            // Check if this synapse was recently active
            if let Some((last_tick, contribution)) = self.recent_activity.get(tag) {
                if contribution > &0.0 && self.current_tick.saturating_sub(*last_tick) <= self.muscle.temporal_window {
                    // Classic Hebbian rule: Δw = α * pre * post
                    // Simplified: increase weight for active synapses
                    let new_weight = weight + self.muscle.learning_rate;
                    let clamped = new_weight.min(10.0).max(0.0); // Cap synaptic strength

                    successors.push(MuscleSuccessor {
                        blob: SealedBlob::new(
                            self.ctx.current_blob().payload.clone(),
                            MuscleSalt::random(self.ctx.rng()),
                            5, // Dendrite version
                        ),
                        metadata: SuccessorMetadata::new(5, "synaptic_weight".to_string())
                            .with_property("lineage_tag".to_string(), hex::encode(tag))
                            .with_property("weight".to_string(), clamped.to_string())
                            .with_property("hebbian_delta".to_string(), self.muscle.learning_rate.to_string())
                            .with_property("pattern_hash".to_string(), hex::encode(&pattern_hash[..8]))
                            .with_property("integration_count".to_string(), self.integration_count.to_string()),
                    });
                }
            }
        }

        // Emit the integrated dendrite itself as a successor
        successors.push(MuscleSuccessor {
            blob: SealedBlob::new(
                self.ctx.current_blob().payload.clone(),
                MuscleSalt::random(self.ctx.rng()),
                5,
            ),
            metadata: SuccessorMetadata::new(5, "dendritic_integrator".to_string())
                .with_property("synapse_count".to_string(), self.weights.len().to_string())
                .with_property("learning_rate".to_string(), self.muscle.learning_rate.to_string())
                .with_property("temporal_window".to_string(), self.muscle.temporal_window.to_string()),
        });

        Ok(successors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;
    use muscle_ea_axonwasm::AxonPulse;

    #[test]
    fn test_dendrite_creation() {
        let muscle = DendriteWasmMuscle::<OsRng>::default();
        assert_eq!(muscle.max_synapses, 8192);
        assert_eq!(muscle.temporal_window, 8);
        assert_eq!(muscle.learning_rate, 0.01);
    }

    #[test]
    fn test_dendrite_integration() {
        let muscle = DendriteWasmMuscle::<OsRng>::default();
        let blob = SealedBlob::new(vec![], MuscleSalt::new([0; 16]), 1);
        let mut ctx = MuscleContext::new(blob, [0; 32], OsRng);

        let pulses = vec![
            AxonPulse {
                payload: Zeroizing::new(b"hello".to_vec()),
                intensity: 10,
                refractory_trace: vec![0xAA; 8],
            },
            AxonPulse {
                payload: Zeroizing::new(b"world".to_vec()),
                intensity: 5,
                refractory_trace: vec![0xBB; 8],
            },
        ];

        let result = muscle.execute(&mut ctx, pulses).unwrap();
        
        // Verify integration results
        assert!(result.output.voltage > 0.0, "Voltage should be positive");
        assert!(result.output.active_inputs >= 15, "Should sum intensities");
        assert!(result.output.payload.len() >= 10, "Should contain both payloads");
        
        // Verify Hebbian successors were generated
        assert!(!result.successors.is_empty(), "Should generate weight successors");
        assert!(result.successors.iter().any(|s| s.metadata.muscle_type == "synaptic_weight"));
        assert!(result.successors.iter().any(|s| s.metadata.muscle_type == "dendritic_integrator"));
    }

    #[test]
    fn test_custom_dendrite_parameters() {
        let muscle = DendriteWasmMuscle::<OsRng>::new(1024, 16, 0.05);
        assert_eq!(muscle.max_synapses, 1024);
        assert_eq!(muscle.temporal_window, 16);
        assert_eq!(muscle.learning_rate, 0.05);
    }

    #[test]
    fn test_hebbian_learning() {
        let muscle = DendriteWasmMuscle::<OsRng>::new(100, 4, 0.1);
        let blob = SealedBlob::new(vec![], MuscleSalt::new([0; 16]), 1);
        let mut ctx = MuscleContext::new(blob, [0; 32], OsRng);

        let pulses = vec![
            AxonPulse {
                payload: Zeroizing::new(vec![]),
                intensity: 8,
                refractory_trace: vec![0xCC; 8], // Consistent tag for testing
            },
        ];

        let result = muscle.execute(&mut ctx, pulses).unwrap();
        
        // Find the weight update successor
        let weight_successor = result.successors
            .iter()
            .find(|s| s.metadata.muscle_type == "synaptic_weight")
            .expect("Should have weight successor");

        // Verify Hebbian properties
        assert!(weight_successor.metadata.properties.contains_key("hebbian_delta"));
        assert!(weight_successor.metadata.properties.contains_key("weight"));
        assert!(weight_successor.metadata.properties.contains_key("lineage_tag"));
        
        let delta_str = weight_successor.metadata.properties.get("hebbian_delta").unwrap();
        let delta: f32 = delta_str.parse().unwrap();
        assert_eq!(delta, 0.1, "Hebbian delta should match learning rate");
    }

    #[test]
    fn test_temporal_summation() {
        let muscle = DendriteWasmMuscle::<OsRng>::new(100, 2, 0.01);
        let blob = SealedBlob::new(vec![], MuscleSalt::new([0; 16]), 1);
        let mut ctx = MuscleContext::new(blob, [0; 32], OsRng);

        // First integration
        let pulses1 = vec![
            AxonPulse {
                payload: Zeroizing::new(vec![]),
                intensity: 5,
                refractory_trace: vec![0xDD; 8],
            },
        ];

        let result1 = muscle.execute(&mut ctx, pulses1).unwrap();
        let voltage1 = result1.output.voltage;

        // Second integration shortly after (within temporal window)
        let pulses2 = vec![
            AxonPulse {
                payload: Zeroizing::new(vec![]),
                intensity: 5,
                refractory_trace: vec![0xDD; 8], // Same synapse
            },
        ];

        let result2 = muscle.execute(&mut ctx, pulses2).unwrap();
        let voltage2 = result2.output.voltage;

        // Second voltage should be higher due to temporal summation
        assert!(voltage2 > voltage1, "Temporal summation should boost voltage");
    }
}
