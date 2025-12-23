use crate::integration::{LatticeUpdate, SealedBlob, SymbioteInterface};

pub struct LatticeUpdateRule;

impl LatticeUpdateRule {
    pub const fn new() -> Self {
        Self
    }

    pub fn process(
        &self,
        symbiote: &mut SymbioteInterface,
        update: LatticeUpdate,
    ) -> Option<HealingAction> {
        symbiote.process_update(update)
    }
}

pub struct HealingAction {
    pub is_healing: bool,
    pub blob: SealedBlob,
}

impl HealingAction {
    pub fn is_healing(&self) -> bool {
        self.is_healing
    }

    pub fn generate_sealed_blob(self) -> Option<SealedBlob> {
        if self.is_healing {
            Some(self.blob)
        } else {
            None
        }
    }
}
