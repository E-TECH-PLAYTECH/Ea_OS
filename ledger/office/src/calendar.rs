//! Calendar application orchestrator.
//!
//! Provides ledger-backed event scheduling with full audit trail.

use std::collections::HashMap;
use std::sync::Arc;

use blake3::Hasher;
use ed25519_dalek::SigningKey;
use ledger_core::brainstem::{AppendReceipt, Ledger};
use ledger_spec::{Hash, Timestamp};

use crate::events::{EventChanges, OfficeEvent};

/// Errors from calendar operations.
#[derive(Debug, thiserror::Error)]
pub enum CalendarError {
    /// Event not found.
    #[error("event not found: {0}")]
    NotFound(String),
    /// Invalid time range.
    #[error("invalid time range: start must be before end")]
    InvalidTimeRange,
    /// Ledger operation failed.
    #[error("ledger error: {0}")]
    Ledger(#[from] ledger_core::apps::AppError),
    /// Serialization failed.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// A calendar event.
#[derive(Debug, Clone)]
pub struct CalendarEvent {
    /// Unique event identifier.
    pub id: Hash,
    /// Event title.
    pub title: String,
    /// Start timestamp (ms since epoch).
    pub start: Timestamp,
    /// End timestamp (ms since epoch).
    pub end: Timestamp,
    /// Optional description.
    pub description: Option<String>,
    /// Optional location.
    pub location: Option<String>,
    /// Recurrence rule (iCal RRULE format).
    pub recurrence: Option<String>,
    /// Whether the event is cancelled.
    pub cancelled: bool,
    /// Last modified timestamp.
    pub modified_at: Timestamp,
}

impl CalendarEvent {
    /// Create a new calendar event.
    pub fn new(
        title: impl Into<String>,
        start: Timestamp,
        end: Timestamp,
    ) -> Result<Self, CalendarError> {
        if start >= end {
            return Err(CalendarError::InvalidTimeRange);
        }

        let title = title.into();
        let mut hasher = Hasher::new();
        hasher.update(b"ea-office:calendar:");
        hasher.update(title.as_bytes());
        hasher.update(&start.to_le_bytes());
        hasher.update(&now_millis().to_le_bytes());
        let id = *hasher.finalize().as_bytes();

        Ok(Self {
            id,
            title,
            start,
            end,
            description: None,
            location: None,
            recurrence: None,
            cancelled: false,
            modified_at: now_millis(),
        })
    }

    /// Get duration in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// Check if this event overlaps with a time range.
    pub fn overlaps(&self, range_start: Timestamp, range_end: Timestamp) -> bool {
        self.start < range_end && self.end > range_start
    }
}

/// Calendar application orchestrator.
pub struct CalendarApp {
    ledger: Ledger,
    signer: Arc<SigningKey>,
    channel: String,
    schema_version: u16,
    /// In-memory event index.
    events: HashMap<Hash, CalendarEvent>,
}

impl CalendarApp {
    /// Create a new calendar application.
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
            events: HashMap::new(),
        }
    }

    /// Append an office event to the ledger.
    fn append_office_event(&self, event: OfficeEvent) -> Result<AppendReceipt, CalendarError> {
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
        self.ledger.append(env).map_err(|e| CalendarError::Ledger(
            ledger_core::apps::AppError::Ledger(e)
        ))
    }

    /// Schedule a new event.
    pub fn schedule_event(
        &mut self,
        title: impl Into<String>,
        start: Timestamp,
        end: Timestamp,
    ) -> Result<(CalendarEvent, AppendReceipt), CalendarError> {
        let event = CalendarEvent::new(title, start, end)?;

        let office_event = OfficeEvent::EventScheduled {
            id: event.id,
            title: event.title.clone(),
            start: event.start,
            end: event.end,
            description: event.description.clone(),
            location: event.location.clone(),
            recurrence: event.recurrence.clone(),
        };

        let receipt = self.append_office_event(office_event)?;
        self.events.insert(event.id, event.clone());
        Ok((event, receipt))
    }

    /// Schedule an event with additional details.
    pub fn schedule_event_full(
        &mut self,
        title: impl Into<String>,
        start: Timestamp,
        end: Timestamp,
        description: Option<String>,
        location: Option<String>,
        recurrence: Option<String>,
    ) -> Result<(CalendarEvent, AppendReceipt), CalendarError> {
        let mut event = CalendarEvent::new(title, start, end)?;
        event.description = description;
        event.location = location;
        event.recurrence = recurrence;

        let office_event = OfficeEvent::EventScheduled {
            id: event.id,
            title: event.title.clone(),
            start: event.start,
            end: event.end,
            description: event.description.clone(),
            location: event.location.clone(),
            recurrence: event.recurrence.clone(),
        };

        let receipt = self.append_office_event(office_event)?;
        self.events.insert(event.id, event.clone());
        Ok((event, receipt))
    }

    /// Modify an existing event.
    pub fn modify_event(
        &mut self,
        id: Hash,
        changes: EventChanges,
    ) -> Result<(CalendarEvent, AppendReceipt), CalendarError> {
        if !self.events.contains_key(&id) {
            return Err(CalendarError::NotFound(hex::encode(id)));
        }

        let event = self.events.get_mut(&id).unwrap();

        // Apply changes
        if let Some(ref title) = changes.title {
            event.title = title.clone();
        }
        if let Some(start) = changes.start {
            event.start = start;
        }
        if let Some(end) = changes.end {
            event.end = end;
        }
        if let Some(ref desc) = changes.description {
            event.description = Some(desc.clone());
        }
        if let Some(ref loc) = changes.location {
            event.location = Some(loc.clone());
        }
        event.modified_at = now_millis();

        // Validate time range
        if event.start >= event.end {
            return Err(CalendarError::InvalidTimeRange);
        }

        let event = event.clone();

        let office_event = OfficeEvent::EventModified {
            id,
            changes,
        };

        let receipt = self.append_office_event(office_event)?;
        Ok((event, receipt))
    }

    /// Cancel an event.
    pub fn cancel_event(
        &mut self,
        id: Hash,
        reason: impl Into<String>,
    ) -> Result<AppendReceipt, CalendarError> {
        let event = self.events.get_mut(&id)
            .ok_or_else(|| CalendarError::NotFound(hex::encode(id)))?;

        event.cancelled = true;
        event.modified_at = now_millis();

        let office_event = OfficeEvent::EventCancelled {
            id,
            reason: reason.into(),
        };

        self.append_office_event(office_event)
    }

    /// Get an event by ID.
    pub fn get_event(&self, id: &Hash) -> Option<&CalendarEvent> {
        self.events.get(id)
    }

    /// List all events (including cancelled).
    pub fn list_events(&self) -> Vec<&CalendarEvent> {
        self.events.values().collect()
    }

    /// List active (non-cancelled) events.
    pub fn list_active_events(&self) -> Vec<&CalendarEvent> {
        self.events.values()
            .filter(|e| !e.cancelled)
            .collect()
    }

    /// Query events in a time range.
    pub fn query_events(
        &self,
        range_start: Timestamp,
        range_end: Timestamp,
    ) -> Vec<&CalendarEvent> {
        self.events.values()
            .filter(|e| !e.cancelled && e.overlaps(range_start, range_end))
            .collect()
    }

    /// Get events for a specific day.
    pub fn events_for_day(&self, year: i32, month: u32, day: u32) -> Vec<&CalendarEvent> {
        use chrono::{NaiveDate, TimeZone, Utc};

        let date = match NaiveDate::from_ymd_opt(year, month, day) {
            Some(d) => d,
            None => return Vec::new(),
        };

        let start_of_day = Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap());
        let end_of_day = Utc.from_utc_datetime(&date.and_hms_opt(23, 59, 59).unwrap());

        let start_ms = start_of_day.timestamp_millis() as u64;
        let end_ms = end_of_day.timestamp_millis() as u64;

        self.query_events(start_ms, end_ms)
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

    fn test_app() -> CalendarApp {
        let signer = SigningKey::generate(&mut OsRng);
        let mut registry = ChannelRegistry::new();
        registry.upsert(ChannelSpec {
            name: "office.calendar".into(),
            policy: ledger_spec::ChannelPolicy {
                min_signers: 1,
                allowed_signers: vec![signer.verifying_key().to_bytes()],
                require_attestations: false,
                enforce_timestamp_ordering: true,
            },
        });
        let ledger = Ledger::new(registry);
        CalendarApp::new(ledger, signer, "office.calendar", 1)
    }

    #[test]
    fn schedule_and_modify_event() {
        let mut app = test_app();

        // Schedule an event
        let start = now_millis();
        let end = start + 3600_000; // 1 hour later
        let (event, receipt) = app.schedule_event("Team Meeting", start, end).unwrap();
        assert_eq!(event.title, "Team Meeting");
        assert!(receipt.merkle.verify());

        // Modify the event
        let changes = EventChanges {
            title: Some("Updated Meeting".into()),
            start: None,
            end: None,
            description: Some("Weekly sync".into()),
            location: Some("Room 101".into()),
        };
        let (updated, receipt2) = app.modify_event(event.id, changes).unwrap();
        assert_eq!(updated.title, "Updated Meeting");
        assert_eq!(updated.description, Some("Weekly sync".into()));
        assert!(receipt2.merkle.verify());
    }

    #[test]
    fn cancel_event() {
        let mut app = test_app();

        let start = now_millis();
        let end = start + 3600_000;
        let (event, _) = app.schedule_event("To Cancel", start, end).unwrap();

        let receipt = app.cancel_event(event.id, "Conflict").unwrap();
        assert!(receipt.merkle.verify());

        let updated = app.get_event(&event.id).unwrap();
        assert!(updated.cancelled);
    }

    #[test]
    fn query_events_in_range() {
        let mut app = test_app();

        let base = now_millis();
        app.schedule_event("Event 1", base, base + 1000).unwrap();
        app.schedule_event("Event 2", base + 2000, base + 3000).unwrap();
        app.schedule_event("Event 3", base + 5000, base + 6000).unwrap();

        // Query range that includes events 1 and 2
        let results = app.query_events(base, base + 4000);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn invalid_time_range() {
        let mut app = test_app();

        let start = now_millis();
        let end = start; // Same as start - invalid

        let result = app.schedule_event("Bad Event", start, end);
        assert!(matches!(result, Err(CalendarError::InvalidTimeRange)));
    }

    #[test]
    fn event_overlap_detection() {
        let event = CalendarEvent::new("Test", 1000, 2000).unwrap();

        assert!(event.overlaps(500, 1500));   // Overlaps start
        assert!(event.overlaps(1500, 2500));  // Overlaps end
        assert!(event.overlaps(1200, 1800));  // Fully inside
        assert!(event.overlaps(500, 2500));   // Fully contains
        assert!(!event.overlaps(2000, 3000)); // After (touching)
        assert!(!event.overlaps(0, 1000));    // Before (touching)
    }
}
