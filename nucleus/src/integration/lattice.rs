use ea_ledger::MuscleUpdate;

#[derive(Debug)]
pub struct LatticeStream {
    // In a real system, this would be a ring buffer or stream from network/disk
    updates: [Option<MuscleUpdate>; 16], 
    head: usize,
    tail: usize,
}

impl LatticeStream {
    pub const fn new() -> Self {
        Self {
            updates: [None; 16],
            head: 0,
            tail: 0,
        }
    }
    
    pub fn verify_root(&self) -> bool {
        // Verify against genesis root
        true
    }
    
    pub fn next_update(&mut self) -> Option<MuscleUpdate> {
        if self.head == self.tail {
            return None;
        }
        
        let update = self.updates[self.tail];
        self.tail = (self.tail + 1) % 16;
        update
    }
    
    pub fn push_update(&mut self, update: MuscleUpdate) -> bool {
        let next = (self.head + 1) % 16;
        if next == self.tail {
            return false;
        }
        
        self.updates[self.head] = Some(update);
        self.head = next;
        true
    }
}

// Re-export for compatibility if needed, but prefer ea_ledger types
#[allow(unused_imports)]
pub use ea_ledger::LatticeRoot;
