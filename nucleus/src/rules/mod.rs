mod boot;
mod timer;
pub mod updates; // Make public so we can access HealingAction

pub use boot::BootRule;
pub use timer::TimerRule;
pub use updates::LatticeUpdateRule;

/// Rule identifiers for compile-time verification
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RuleId {
    Boot,
    LatticeUpdate,
    Timer,
}

/// Fixed-size rule engine
#[derive(Debug)]
pub struct RuleEngine {
    current_rule: RuleId,
    rule_flags: u8,
}

impl RuleEngine {
    pub const fn new() -> Self {
        Self {
            current_rule: RuleId::Boot,
            rule_flags: 0b111, // All rules enabled
        }
    }

    pub const fn is_rule_enabled(&self, rule: RuleId) -> bool {
        match rule {
            RuleId::Boot => (self.rule_flags & 0b001) != 0,
            RuleId::LatticeUpdate => (self.rule_flags & 0b010) != 0,
            RuleId::Timer => (self.rule_flags & 0b100) != 0,
        }
    }

    pub fn set_current_rule(&mut self, rule: RuleId) {
        self.current_rule = rule;
    }

    pub fn current_rule(&self) -> RuleId {
        self.current_rule
    }
}
