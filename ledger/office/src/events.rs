//! Office-specific event types for the ledger.
//!
//! These events extend the core ledger event system with document,
//! spreadsheet, file, and calendar operations.

use ledger_spec::events::ContentRef;
use ledger_spec::{Hash, Timestamp};
use serde::{Deserialize, Serialize};

/// Reference to a spreadsheet cell.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CellRef {
    /// Column index (0-based).
    pub col: u32,
    /// Row index (0-based).
    pub row: u32,
}

impl CellRef {
    /// Create a new cell reference.
    pub fn new(col: u32, row: u32) -> Self {
        Self { col, row }
    }

    /// Convert to A1 notation (e.g., "B3").
    pub fn to_a1(&self) -> String {
        let col_name = Self::col_to_letter(self.col);
        format!("{}{}", col_name, self.row + 1)
    }

    fn col_to_letter(col: u32) -> String {
        let mut result = String::new();
        let mut n = col;
        loop {
            result.insert(0, (b'A' + (n % 26) as u8) as char);
            if n < 26 {
                break;
            }
            n = n / 26 - 1;
        }
        result
    }
}

/// Cell value types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CellValue {
    /// Empty cell.
    Empty,
    /// Text content.
    Text(String),
    /// Numeric value.
    Number(f64),
    /// Boolean value.
    Boolean(bool),
    /// Error value.
    Error(String),
}

impl Default for CellValue {
    fn default() -> Self {
        Self::Empty
    }
}

/// File metadata for the file manager.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileMetadata {
    /// File size in bytes.
    pub size: u64,
    /// MIME type.
    pub mime_type: Option<String>,
    /// Creation timestamp.
    pub created_at: Timestamp,
    /// Last modified timestamp.
    pub modified_at: Timestamp,
    /// Whether the file is a directory.
    pub is_directory: bool,
}

/// Changes to a calendar event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventChanges {
    /// New title (if changed).
    pub title: Option<String>,
    /// New start time (if changed).
    pub start: Option<Timestamp>,
    /// New end time (if changed).
    pub end: Option<Timestamp>,
    /// New description (if changed).
    pub description: Option<String>,
    /// New location (if changed).
    pub location: Option<String>,
}

/// Office application events recorded to the ledger.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum OfficeEvent {
    // ─────────────────────────────────────────────────────────────
    // Document Events
    // ─────────────────────────────────────────────────────────────
    /// A new document was created.
    DocumentCreated {
        /// Unique document identifier.
        id: Hash,
        /// Document title.
        title: String,
        /// Reference to content in CAS.
        content: ContentRef,
    },

    /// An existing document was updated.
    DocumentUpdated {
        /// Document identifier.
        id: Hash,
        /// New version number.
        version: u64,
        /// Reference to new content in CAS.
        content: ContentRef,
        /// Optional diff from previous version.
        diff: Option<ContentRef>,
    },

    /// A document was deleted.
    DocumentDeleted {
        /// Document identifier.
        id: Hash,
        /// Reason for deletion.
        reason: String,
    },

    // ─────────────────────────────────────────────────────────────
    // Spreadsheet Events
    // ─────────────────────────────────────────────────────────────
    /// A new spreadsheet was created.
    SheetCreated {
        /// Unique sheet identifier.
        id: Hash,
        /// Sheet name.
        name: String,
        /// Initial column count.
        columns: u32,
        /// Initial row count.
        rows: u32,
    },

    /// A cell was updated.
    CellUpdated {
        /// Sheet identifier.
        sheet_id: Hash,
        /// Cell reference.
        cell: CellRef,
        /// New cell value.
        value: CellValue,
        /// Formula (if any).
        formula: Option<String>,
    },

    /// Multiple cells were updated in a batch.
    CellBatchUpdated {
        /// Sheet identifier.
        sheet_id: Hash,
        /// List of cell updates.
        updates: Vec<(CellRef, CellValue, Option<String>)>,
    },

    /// A spreadsheet was deleted.
    SheetDeleted {
        /// Sheet identifier.
        id: Hash,
        /// Reason for deletion.
        reason: String,
    },

    // ─────────────────────────────────────────────────────────────
    // File Manager Events
    // ─────────────────────────────────────────────────────────────
    /// A file was stored.
    FileStored {
        /// Virtual path.
        path: String,
        /// Reference to content in CAS.
        content: ContentRef,
        /// File metadata.
        metadata: FileMetadata,
    },

    /// A file was deleted.
    FileDeleted {
        /// Virtual path.
        path: String,
        /// Reason for deletion.
        reason: String,
    },

    /// A directory was created.
    DirectoryCreated {
        /// Virtual path.
        path: String,
    },

    /// A file or directory was moved.
    FileMoved {
        /// Original path.
        from: String,
        /// New path.
        to: String,
    },

    // ─────────────────────────────────────────────────────────────
    // Calendar Events
    // ─────────────────────────────────────────────────────────────
    /// A calendar event was scheduled.
    EventScheduled {
        /// Unique event identifier.
        id: Hash,
        /// Event title.
        title: String,
        /// Start timestamp.
        start: Timestamp,
        /// End timestamp.
        end: Timestamp,
        /// Optional description.
        description: Option<String>,
        /// Optional location.
        location: Option<String>,
        /// Recurrence rule (if any).
        recurrence: Option<String>,
    },

    /// A calendar event was modified.
    EventModified {
        /// Event identifier.
        id: Hash,
        /// Changes applied.
        changes: EventChanges,
    },

    /// A calendar event was cancelled.
    EventCancelled {
        /// Event identifier.
        id: Hash,
        /// Reason for cancellation.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_ref_to_a1() {
        assert_eq!(CellRef::new(0, 0).to_a1(), "A1");
        assert_eq!(CellRef::new(1, 2).to_a1(), "B3");
        assert_eq!(CellRef::new(25, 0).to_a1(), "Z1");
        assert_eq!(CellRef::new(26, 0).to_a1(), "AA1");
        assert_eq!(CellRef::new(27, 0).to_a1(), "AB1");
        assert_eq!(CellRef::new(701, 99).to_a1(), "ZZ100");
    }

    #[test]
    fn office_event_serialization() {
        let event = OfficeEvent::DocumentCreated {
            id: [0xAB; 32],
            title: "Test Doc".into(),
            content: ContentRef {
                locator: "cas:abc123".into(),
                hash: [0xCD; 32],
                media_type: Some("text/markdown".into()),
                bytes: Some(1024),
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let restored: OfficeEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, restored);
    }
}
