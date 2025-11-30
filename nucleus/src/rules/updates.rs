use crate::integration::SymbioteInterface;

pub struct LatticeUpdateRule;

impl LatticeUpdateRule {
    pub const fn new() -> Self {
        Self
    }
    
    pub fn process(&self, symbiote: &mut SymbioteInterface, update: LatticeUpdate) -> Option<HealingAction> {
        symbiote.process_update(update)
    }
}

pub struct LatticeUpdate {
    pub position: u64,
    pub value: [u8; 32],
    pub proof: [u8; 64],
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
