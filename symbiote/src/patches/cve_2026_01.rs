//! CVE-2026-01 specific patch implementation

use super::{SecurityPatch, PatchError};
use alloc::string::String;

/// Detailed implementation for CVE-2026-01
impl super::Cve202601Patch {
    /// Check if source is vulnerable to CVE-2026-01
    pub fn is_vulnerable(&self, source: &str) -> bool {
        // Detect vulnerable pattern: unbounded array loading without validation
        source.contains("np.array([") && 
        !source.contains("input_validation") &&
        source.contains("W1 =")
    }

    /// Get CVE details
    pub fn cve_details(&self) -> CveDetails {
        CveDetails {
            id: "CVE-2026-01",
            description: "Buffer overflow in neural network weight matrix loading",
            severity: "HIGH",
            affected_versions: "muscle versions 40-42",
            fixed_version: "version 43",
        }
    }
}

/// CVE details structure
#[derive(Debug, Clone)]
pub struct CveDetails {
    /// CVE identifier
    pub id: &'static str,
    /// Vulnerability description
    pub description: &'static str,
    /// Severity level
    pub severity: &'static str,
    /// Affected versions
    pub affected_versions: &'static str,
    /// Fixed version
    pub fixed_version: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vulnerability_detection() {
        let patch = super::Cve202601Patch;
        
        let vulnerable_source = r#"
            import numpy as np
            W1 = np.array([
                [0.1, 0.2, 0.3],
                [0.4, 0.5, 0.6]
            ])
        "#;
        
        let safe_source = r#"
            import numpy as np
            # input_validation added
            W1 = np.array([
                [0.1, 0.2, 0.3]
            ])
        "#;
        
        assert!(patch.is_vulnerable(vulnerable_source));
        assert!(!patch.is_vulnerable(safe_source));
    }

    #[test]
    fn test_cve_details() {
        let patch = super::Cve202601Patch;
        let details = patch.cve_details();
        
        assert_eq!(details.id, "CVE-2026-01");
        assert_eq!(details.severity, "HIGH");
    }
}
