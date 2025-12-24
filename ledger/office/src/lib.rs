//! Eä Office Suite - Ledger-backed productivity applications.
//!
//! This crate provides a complete office suite with cryptographic versioning:
//! - **Notes/Documents**: Markdown editor with full version history
//! - **Spreadsheet**: Grid-based data with formulas
//! - **File Manager**: CAS-backed file browser
//! - **Calendar**: Event scheduling with audit trails
//!
//! All operations are recorded to the Eä ledger with Merkle proofs.

#![deny(missing_docs)]

pub mod events;
pub mod document;
pub mod spreadsheet;
pub mod files;
pub mod calendar;
pub mod ui;

pub use events::OfficeEvent;
pub use document::DocumentApp;
pub use spreadsheet::SpreadsheetApp;
pub use files::FileManagerApp;
pub use calendar::CalendarApp;
