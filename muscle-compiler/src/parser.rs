// muscle-compiler/src/parser.rs
// Eä Weight Parser v5.0 — Robust Python weight extraction

use regex::Regex;
use once_cell::sync::Lazy;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Weight matrix W1 not found")]
    W1NotFound,
    #[error("Bias vector b1 not found")]
    B1NotFound,
    #[error("Weight vector W2 not found")]
    W2NotFound,
    #[error("Invalid float value: {0}")]
    InvalidFloat(String),
    #[error("Invalid matrix dimensions: expected {expected}, found {found}")]
    InvalidDimensions { expected: usize, found: usize },
}

/// Neural network weights structure
#[derive(Debug, Clone, PartialEq)]
pub struct Weights {
    pub w1: [[f32; 3]; 4],   // 4x3 input→hidden weights
    pub b1: [f32; 3],        // 3 hidden biases
    pub w2: [f32; 3],        // 3x1 hidden→output weights  
    pub b2: f32,             // output bias
}

// Regex patterns for parsing Python numpy arrays
static RE_W1: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"W1\s*=\s*np\.array\s*\(\s*\[\s*\[([^\]]+)\]\s*,\s*\[([^\]]+)\]\s*,\s*\[([^\]]+)\]\s*,\s*\[([^\]]+)\]\s*\]\s*\)").unwrap()
});

static RE_B1: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"b1\s*=\s*np\.array\s*\(\s*\[\s*([^,\]]+)\s*,\s*([^,\]]+)\s*,\s*([^,\]]+)\s*\]\s*\)").unwrap()
});

static RE_W2: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"W2\s*=\s*np\.array\s*\(\s*\[\s*([^,\]]+)\s*,\s*([^,\]]+)\s*,\s*([^,\]]+)\s*\]\s*\)").unwrap()
});

static RE_B2: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"b2\s*=\s*([\d\.\-e]+)").unwrap()
});

/// Parse a single f32 value from string
fn parse_float(s: &str) -> Result<f32, ParseError> {
    s.trim()
        .parse()
        .map_err(|_| ParseError::InvalidFloat(s.to_string()))
}

/// Parse a row of 3 f32 values
fn parse_row(row_str: &str) -> Result<[f32; 3], ParseError> {
    let parts: Vec<&str> = row_str.split(',').map(|s| s.trim()).collect();
    if parts.len() != 3 {
        return Err(ParseError::InvalidDimensions {
            expected: 3,
            found: parts.len(),
        });
    }

    Ok([
        parse_float(parts[0])?,
        parse_float(parts[1])?,
        parse_float(parts[2])?,
    ])
}

/// Extract weights from Python source code
pub fn extract_weights(source: &str) -> Result<Weights, ParseError> {
    // Extract W1 (4x3 matrix)
    let w1_caps = RE_W1.captures(source).ok_or(ParseError::W1NotFound)?;
    let w1 = [
        parse_row(w1_caps.get(1).unwrap().as_str())?,
        parse_row(w1_caps.get(2).unwrap().as_str())?,
        parse_row(w1_caps.get(3).unwrap().as_str())?,
        parse_row(w1_caps.get(4).unwrap().as_str())?,
    ];

    // Extract b1 (3-element vector)
    let b1_caps = RE_B1.captures(source).ok_or(ParseError::B1NotFound)?;
    let b1 = [
        parse_float(b1_caps.get(1).unwrap().as_str())?,
        parse_float(b1_caps.get(2).unwrap().as_str())?,
        parse_float(b1_caps.get(3).unwrap().as_str())?,
    ];

    // Extract W2 (3-element vector)
    let w2_caps = RE_W2.captures(source).ok_or(ParseError::W2NotFound)?;
    let w2 = [
        parse_float(w2_caps.get(1).unwrap().as_str())?,
        parse_float(w2_caps.get(2).unwrap().as_str())?,
        parse_float(w2_caps.get(3).unwrap().as_str())?,
    ];

    // Extract b2 (scalar)
    let b2 = RE_B2.captures(source)
        .and_then(|caps| parse_float(caps.get(1).unwrap().as_str()).ok())
        .unwrap_or(0.0); // Default to 0.0 if not found

    Ok(Weights { w1, b1, w2, b2 })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SOURCE: &str = r#"
import numpy as np

# Neural network weights for family muscle
W1 = np.array([
    [0.1, 0.2, 0.3],
    [0.4, 0.5, 0.6], 
    [0.7, 0.8, 0.9],
    [1.0, 1.1, 1.2]
])

b1 = np.array([0.1, 0.2, 0.3])

W2 = np.array([0.4, 0.5, 0.6])

b2 = 0.7
"#;

    #[test]
    fn test_extract_weights() {
        let weights = extract_weights(TEST_SOURCE).unwrap();
        
        assert_eq!(weights.w1[0], [0.1, 0.2, 0.3]);
        assert_eq!(weights.w1[1], [0.4, 0.5, 0.6]);
        assert_eq!(weights.w1[2], [0.7, 0.8, 0.9]);
        assert_eq!(weights.w1[3], [1.0, 1.1, 1.2]);
        
        assert_eq!(weights.b1, [0.1, 0.2, 0.3]);
        assert_eq!(weights.w2, [0.4, 0.5, 0.6]);
        assert_eq!(weights.b2, 0.7);
    }

    #[test]
    fn test_missing_components() {
        let incomplete_source = "W1 = np.array([[0.1, 0.2, 0.3]])";
        assert!(extract_weights(incomplete_source).is_err());
    }

    #[test]
    fn test_invalid_floats() {
        let bad_source = r#"
W1 = np.array([[0.1, 0.2, invalid]])
b1 = np.array([0.1, 0.2, 0.3])
W2 = np.array([0.4, 0.5, 0.6])
b2 = 0.7
"#;
        assert!(extract_weights(bad_source).is_err());
    }
}
