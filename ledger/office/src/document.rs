//! Document application orchestrator.
//!
//! Provides ledger-backed document management with full version history
//! and Merkle proofs for every save operation.

use std::collections::HashMap;
use std::sync::Arc;

use blake3::Hasher;
use ed25519_dalek::SigningKey;
use ledger_core::brainstem::{AppendReceipt, Ledger};
use ledger_spec::events::ContentRef;
use ledger_spec::{ChannelRegistry, Hash, SchemaVersion, Timestamp};

use crate::events::OfficeEvent;

/// Errors from document operations.
#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    /// Document not found.
    #[error("document not found: {0}")]
    NotFound(String),
    /// Ledger operation failed.
    #[error("ledger error: {0}")]
    Ledger(#[from] ledger_core::apps::AppError),
    /// Serialization failed.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// In-memory document state.
#[derive(Debug, Clone)]
pub struct Document {
    /// Unique document identifier.
    pub id: Hash,
    /// Document title.
    pub title: String,
    /// Current content (plaintext/markdown).
    pub content: String,
    /// Current version number.
    pub version: u64,
    /// Last modified timestamp.
    pub modified_at: Timestamp,
    /// Content reference in CAS.
    pub content_ref: Option<ContentRef>,
}

impl Document {
    /// Create a new document with the given title.
    pub fn new(title: impl Into<String>) -> Self {
        let title = title.into();
        let mut hasher = Hasher::new();
        hasher.update(b"ea-office:document:");
        hasher.update(title.as_bytes());
        hasher.update(&now_millis().to_le_bytes());
        let id = *hasher.finalize().as_bytes();

        Self {
            id,
            title,
            content: String::new(),
            version: 0,
            modified_at: now_millis(),
            content_ref: None,
        }
    }
}

/// Document application orchestrator.
pub struct DocumentApp {
    ledger: Ledger,
    signer: Arc<SigningKey>,
    channel: String,
    schema_version: SchemaVersion,
    /// In-memory document index.
    documents: HashMap<Hash, Document>,
}

impl DocumentApp {
    /// Create a new document application.
    pub fn new(
        ledger: Ledger,
        signer: SigningKey,
        channel: impl Into<String>,
        schema_version: SchemaVersion,
    ) -> Self {
        Self {
            ledger,
            signer: Arc::new(signer),
            channel: channel.into(),
            schema_version,
            documents: HashMap::new(),
        }
    }

    /// Store content in CAS and return a reference.
    fn store_content(&self, content: &str) -> ContentRef {
        let bytes = content.as_bytes().to_vec();
        let digest = self.ledger.content_store().put(bytes.clone());
        ContentRef {
            locator: format!("cas:{}", hex::encode(digest)),
            hash: digest,
            media_type: Some("text/markdown".into()),
            bytes: Some(bytes.len() as u64),
        }
    }

    /// Append an office event to the ledger.
    fn append_office_event(&self, event: OfficeEvent) -> Result<AppendReceipt, DocumentError> {
        // Wrap office event in JSON and use a custom payload type
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
        self.ledger.append(env).map_err(|e| DocumentError::Ledger(
            ledger_core::apps::AppError::Ledger(e)
        ))
    }

    /// Create a new document.
    pub fn create_document(&mut self, title: impl Into<String>) -> Result<(Document, AppendReceipt), DocumentError> {
        let mut doc = Document::new(title);
        let content_ref = self.store_content(&doc.content);
        doc.content_ref = Some(content_ref.clone());
        doc.version = 1;

        let event = OfficeEvent::DocumentCreated {
            id: doc.id,
            title: doc.title.clone(),
            content: content_ref,
        };

        let receipt = self.append_office_event(event)?;
        self.documents.insert(doc.id, doc.clone());
        Ok((doc, receipt))
    }

    /// Update a document's content.
    pub fn update_document(
        &mut self,
        id: Hash,
        new_content: impl Into<String>,
    ) -> Result<(Document, AppendReceipt), DocumentError> {
        if !self.documents.contains_key(&id) {
            return Err(DocumentError::NotFound(hex::encode(id)));
        }

        let new_content = new_content.into();
        let content_ref = self.store_content(&new_content);

        let doc = self.documents.get_mut(&id).unwrap();
        doc.content = new_content;
        doc.version += 1;
        doc.modified_at = now_millis();
        doc.content_ref = Some(content_ref.clone());

        let event = OfficeEvent::DocumentUpdated {
            id: doc.id,
            version: doc.version,
            content: content_ref,
            diff: None, // Full content stored in CAS; diff computation optional for future optimization
        };

        let doc = doc.clone();
        let receipt = self.append_office_event(event)?;
        Ok((doc, receipt))
    }

    /// Delete a document.
    pub fn delete_document(
        &mut self,
        id: Hash,
        reason: impl Into<String>,
    ) -> Result<AppendReceipt, DocumentError> {
        if !self.documents.contains_key(&id) {
            return Err(DocumentError::NotFound(hex::encode(id)));
        }

        let event = OfficeEvent::DocumentDeleted {
            id,
            reason: reason.into(),
        };

        let receipt = self.append_office_event(event)?;
        self.documents.remove(&id);
        Ok(receipt)
    }

    /// Get a document by ID.
    pub fn get_document(&self, id: &Hash) -> Option<&Document> {
        self.documents.get(id)
    }

    /// List all documents.
    pub fn list_documents(&self) -> Vec<&Document> {
        self.documents.values().collect()
    }

    /// Get the ledger for direct queries.
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
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
    use ledger_spec::ChannelSpec;
    use rand_core::OsRng;

    fn test_app() -> DocumentApp {
        let signer = SigningKey::generate(&mut OsRng);
        let mut registry = ChannelRegistry::new();
        registry.upsert(ChannelSpec {
            name: "office.documents".into(),
            policy: ledger_spec::ChannelPolicy {
                min_signers: 1,
                allowed_signers: vec![signer.verifying_key().to_bytes()],
                require_attestations: false,
                enforce_timestamp_ordering: true,
            },
        });
        let ledger = Ledger::new(registry);
        DocumentApp::new(ledger, signer, "office.documents", 1)
    }

    #[test]
    fn create_and_update_document() {
        let mut app = test_app();

        let (doc, receipt) = app.create_document("My Notes").unwrap();
        assert_eq!(doc.title, "My Notes");
        assert_eq!(doc.version, 1);
        assert!(receipt.merkle.verify());

        let (updated, receipt2) = app.update_document(doc.id, "Hello, world!").unwrap();
        assert_eq!(updated.content, "Hello, world!");
        assert_eq!(updated.version, 2);
        assert!(receipt2.merkle.verify());
    }

    #[test]
    fn delete_document() {
        let mut app = test_app();

        let (doc, _) = app.create_document("To Delete").unwrap();
        assert!(app.get_document(&doc.id).is_some());

        let receipt = app.delete_document(doc.id, "No longer needed").unwrap();
        assert!(receipt.merkle.verify());
        assert!(app.get_document(&doc.id).is_none());
    }

    #[test]
    fn list_documents() {
        let mut app = test_app();

        app.create_document("Doc 1").unwrap();
        app.create_document("Doc 2").unwrap();
        app.create_document("Doc 3").unwrap();

        let docs = app.list_documents();
        assert_eq!(docs.len(), 3);
    }
}
