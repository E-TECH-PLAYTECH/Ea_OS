//! Spreadsheet application orchestrator.
//!
//! Provides ledger-backed spreadsheet with cell-level versioning
//! and formula support.

use std::collections::HashMap;
use std::sync::Arc;

use blake3::Hasher;
use ed25519_dalek::SigningKey;
use ledger_core::brainstem::{AppendReceipt, Ledger};
use ledger_spec::{Hash, Timestamp};

use crate::events::{CellRef, CellValue, OfficeEvent};

/// Errors from spreadsheet operations.
#[derive(Debug, thiserror::Error)]
pub enum SpreadsheetError {
    /// Sheet not found.
    #[error("sheet not found: {0}")]
    NotFound(String),
    /// Invalid cell reference.
    #[error("invalid cell reference: {0}")]
    InvalidCell(String),
    /// Formula evaluation error.
    #[error("formula error: {0}")]
    FormulaError(String),
    /// Ledger operation failed.
    #[error("ledger error: {0}")]
    Ledger(#[from] ledger_core::apps::AppError),
    /// Serialization failed.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// A single cell in the spreadsheet.
#[derive(Debug, Clone, Default)]
pub struct Cell {
    /// Cell value.
    pub value: CellValue,
    /// Formula (if any).
    pub formula: Option<String>,
}

/// In-memory spreadsheet state.
#[derive(Debug, Clone)]
pub struct Sheet {
    /// Unique sheet identifier.
    pub id: Hash,
    /// Sheet name.
    pub name: String,
    /// Number of columns.
    pub columns: u32,
    /// Number of rows.
    pub rows: u32,
    /// Cell data (row-major storage).
    pub cells: HashMap<(u32, u32), Cell>,
    /// Last modified timestamp.
    pub modified_at: Timestamp,
}

impl Sheet {
    /// Create a new sheet.
    pub fn new(name: impl Into<String>, columns: u32, rows: u32) -> Self {
        let name = name.into();
        let mut hasher = Hasher::new();
        hasher.update(b"ea-office:sheet:");
        hasher.update(name.as_bytes());
        hasher.update(&now_millis().to_le_bytes());
        let id = *hasher.finalize().as_bytes();

        Self {
            id,
            name,
            columns,
            rows,
            cells: HashMap::new(),
            modified_at: now_millis(),
        }
    }

    /// Get a cell value.
    pub fn get_cell(&self, col: u32, row: u32) -> &Cell {
        static EMPTY: Cell = Cell {
            value: CellValue::Empty,
            formula: None,
        };
        self.cells.get(&(col, row)).unwrap_or(&EMPTY)
    }

    /// Set a cell value.
    pub fn set_cell(&mut self, col: u32, row: u32, value: CellValue, formula: Option<String>) {
        self.cells.insert((col, row), Cell { value, formula });
        self.modified_at = now_millis();
    }
}

/// Spreadsheet application orchestrator.
pub struct SpreadsheetApp {
    ledger: Ledger,
    signer: Arc<SigningKey>,
    channel: String,
    schema_version: u16,
    /// In-memory sheet index.
    sheets: HashMap<Hash, Sheet>,
}

impl SpreadsheetApp {
    /// Create a new spreadsheet application.
    pub fn new(
        ledger: Ledger,
        signer: SigningKey,
        channel: impl Into<String>,
        schema_version: u16,
    ) -> Self {
        Self {
            ledger,
            signer: Arc::new(signer),
            channel: channel.into(),
            schema_version,
            sheets: HashMap::new(),
        }
    }

    /// Append an office event to the ledger.
    fn append_office_event(&self, event: OfficeEvent) -> Result<AppendReceipt, SpreadsheetError> {
        let payload = serde_json::to_value(&event)?;
        let body = ledger_spec::EnvelopeBody {
            payload,
            payload_type: Some("ea.office.v1".into()),
        };
        let body_hash = ledger_spec::hash_body(&body);
        let mut env = ledger_spec::Envelope {
            header: ledger_spec::EnvelopeHeader {
                channel: self.channel.clone(),
                version: self.schema_version,
                prev: self.ledger.tail_hash(),
                body_hash,
                timestamp: now_millis(),
            },
            body,
            signatures: Vec::new(),
            attestations: Vec::new(),
        };
        ledger_core::signing::sign_envelope(&mut env, &self.signer);
        self.ledger.append(env).map_err(|e| SpreadsheetError::Ledger(
            ledger_core::apps::AppError::Ledger(e)
        ))
    }

    /// Create a new spreadsheet.
    pub fn create_sheet(
        &mut self,
        name: impl Into<String>,
        columns: u32,
        rows: u32,
    ) -> Result<(Sheet, AppendReceipt), SpreadsheetError> {
        let sheet = Sheet::new(name, columns.max(1), rows.max(1));

        let event = OfficeEvent::SheetCreated {
            id: sheet.id,
            name: sheet.name.clone(),
            columns: sheet.columns,
            rows: sheet.rows,
        };

        let receipt = self.append_office_event(event)?;
        self.sheets.insert(sheet.id, sheet.clone());
        Ok((sheet, receipt))
    }

    /// Update a single cell.
    pub fn update_cell(
        &mut self,
        sheet_id: Hash,
        col: u32,
        row: u32,
        value: CellValue,
        formula: Option<String>,
    ) -> Result<AppendReceipt, SpreadsheetError> {
        if !self.sheets.contains_key(&sheet_id) {
            return Err(SpreadsheetError::NotFound(hex::encode(sheet_id)));
        }

        // Evaluate formula if present
        let final_value = if let Some(ref formula_str) = formula {
            Self::parse_simple_formula(formula_str).unwrap_or(value.clone())
        } else {
            value.clone()
        };

        let sheet = self.sheets.get_mut(&sheet_id).unwrap();
        sheet.set_cell(col, row, final_value.clone(), formula.clone());

        let event = OfficeEvent::CellUpdated {
            sheet_id,
            cell: CellRef::new(col, row),
            value: final_value,
            formula,
        };

        self.append_office_event(event)
    }

    /// Update multiple cells in a batch.
    pub fn update_cells_batch(
        &mut self,
        sheet_id: Hash,
        updates: Vec<(u32, u32, CellValue, Option<String>)>,
    ) -> Result<AppendReceipt, SpreadsheetError> {
        let sheet = self.sheets.get_mut(&sheet_id)
            .ok_or_else(|| SpreadsheetError::NotFound(hex::encode(sheet_id)))?;

        let mut batch = Vec::new();
        for (col, row, value, formula) in updates {
            let final_value = if let Some(ref formula_str) = formula {
                // Simple formula evaluation
                match Self::parse_simple_formula(formula_str) {
                    Some(v) => v,
                    None => value.clone(),
                }
            } else {
                value.clone()
            };

            sheet.set_cell(col, row, final_value.clone(), formula.clone());
            batch.push((CellRef::new(col, row), final_value, formula));
        }

        let event = OfficeEvent::CellBatchUpdated {
            sheet_id,
            updates: batch,
        };

        self.append_office_event(event)
    }

    /// Simple formula evaluation (for demo).
    /// Will be used when cell reference formulas (=A1+B2) are implemented.
    #[allow(dead_code)]
    fn evaluate_formula(&self, _sheet: &Sheet, formula: &str) -> Result<CellValue, SpreadsheetError> {
        if let Some(value) = Self::parse_simple_formula(formula) {
            Ok(value)
        } else {
            Err(SpreadsheetError::FormulaError(format!(
                "unsupported formula: {}",
                formula
            )))
        }
    }

    /// Parse simple formulas like "=1+2" or "=SUM(1,2,3)".
    fn parse_simple_formula(formula: &str) -> Option<CellValue> {
        let formula = formula.trim();
        if !formula.starts_with('=') {
            return None;
        }
        let expr = &formula[1..];

        // Try simple number
        if let Ok(n) = expr.parse::<f64>() {
            return Some(CellValue::Number(n));
        }

        // Try simple addition: "1+2"
        if let Some((a, b)) = expr.split_once('+') {
            if let (Ok(a), Ok(b)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
                return Some(CellValue::Number(a + b));
            }
        }

        // Try SUM(1,2,3)
        if expr.starts_with("SUM(") && expr.ends_with(')') {
            let inner = &expr[4..expr.len() - 1];
            let sum: f64 = inner
                .split(',')
                .filter_map(|s| s.trim().parse::<f64>().ok())
                .sum();
            return Some(CellValue::Number(sum));
        }

        // Try AVG(1,2,3)
        if expr.starts_with("AVG(") && expr.ends_with(')') {
            let inner = &expr[4..expr.len() - 1];
            let nums: Vec<f64> = inner
                .split(',')
                .filter_map(|s| s.trim().parse::<f64>().ok())
                .collect();
            if !nums.is_empty() {
                let avg = nums.iter().sum::<f64>() / nums.len() as f64;
                return Some(CellValue::Number(avg));
            }
        }

        None
    }

    /// Delete a spreadsheet.
    pub fn delete_sheet(
        &mut self,
        id: Hash,
        reason: impl Into<String>,
    ) -> Result<AppendReceipt, SpreadsheetError> {
        if !self.sheets.contains_key(&id) {
            return Err(SpreadsheetError::NotFound(hex::encode(id)));
        }

        let event = OfficeEvent::SheetDeleted {
            id,
            reason: reason.into(),
        };

        let receipt = self.append_office_event(event)?;
        self.sheets.remove(&id);
        Ok(receipt)
    }

    /// Get a sheet by ID.
    pub fn get_sheet(&self, id: &Hash) -> Option<&Sheet> {
        self.sheets.get(id)
    }

    /// List all sheets.
    pub fn list_sheets(&self) -> Vec<&Sheet> {
        self.sheets.values().collect()
    }
}

fn now_millis() -> Timestamp {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_spec::{ChannelRegistry, ChannelSpec};
    use rand_core::OsRng;

    fn test_app() -> SpreadsheetApp {
        let signer = SigningKey::generate(&mut OsRng);
        let mut registry = ChannelRegistry::new();
        registry.upsert(ChannelSpec {
            name: "office.spreadsheets".into(),
            policy: ledger_spec::ChannelPolicy {
                min_signers: 1,
                allowed_signers: vec![signer.verifying_key().to_bytes()],
                require_attestations: false,
                enforce_timestamp_ordering: true,
            },
        });
        let ledger = Ledger::new(registry);
        SpreadsheetApp::new(ledger, signer, "office.spreadsheets", 1)
    }

    #[test]
    fn create_and_update_sheet() {
        let mut app = test_app();

        let (sheet, receipt) = app.create_sheet("Budget", 10, 20).unwrap();
        assert_eq!(sheet.name, "Budget");
        assert_eq!(sheet.columns, 10);
        assert_eq!(sheet.rows, 20);
        assert!(receipt.merkle.verify());

        let receipt2 = app.update_cell(
            sheet.id,
            0, 0,
            CellValue::Text("Header".into()),
            None,
        ).unwrap();
        assert!(receipt2.merkle.verify());

        let updated_sheet = app.get_sheet(&sheet.id).unwrap();
        assert!(matches!(
            &updated_sheet.get_cell(0, 0).value,
            CellValue::Text(s) if s == "Header"
        ));
    }

    #[test]
    fn formula_evaluation() {
        let mut app = test_app();

        let (sheet, _) = app.create_sheet("Calc", 5, 5).unwrap();

        // Test simple formula
        app.update_cell(
            sheet.id,
            0, 0,
            CellValue::Empty,
            Some("=1+2".into()),
        ).unwrap();

        let updated = app.get_sheet(&sheet.id).unwrap();
        assert!(matches!(
            updated.get_cell(0, 0).value,
            CellValue::Number(n) if (n - 3.0).abs() < 0.001
        ));

        // Test SUM
        app.update_cell(
            sheet.id,
            1, 0,
            CellValue::Empty,
            Some("=SUM(1,2,3,4)".into()),
        ).unwrap();

        let updated = app.get_sheet(&sheet.id).unwrap();
        assert!(matches!(
            updated.get_cell(1, 0).value,
            CellValue::Number(n) if (n - 10.0).abs() < 0.001
        ));
    }

    #[test]
    fn batch_update() {
        let mut app = test_app();

        let (sheet, _) = app.create_sheet("Batch", 10, 10).unwrap();

        let updates = vec![
            (0, 0, CellValue::Number(1.0), None),
            (1, 0, CellValue::Number(2.0), None),
            (2, 0, CellValue::Number(3.0), None),
        ];

        let receipt = app.update_cells_batch(sheet.id, updates).unwrap();
        assert!(receipt.merkle.verify());

        let updated = app.get_sheet(&sheet.id).unwrap();
        assert!(matches!(updated.get_cell(0, 0).value, CellValue::Number(n) if (n - 1.0).abs() < 0.001));
        assert!(matches!(updated.get_cell(1, 0).value, CellValue::Number(n) if (n - 2.0).abs() < 0.001));
        assert!(matches!(updated.get_cell(2, 0).value, CellValue::Number(n) if (n - 3.0).abs() < 0.001));
    }
}
