//! File manager application orchestrator.
//!
//! Provides CAS-backed virtual file system with ledger audit trail.

use std::collections::HashMap;
use std::sync::Arc;

use ed25519_dalek::SigningKey;
use ledger_core::brainstem::{AppendReceipt, Ledger};
use ledger_spec::events::ContentRef;
use ledger_spec::Timestamp;

use crate::events::{FileMetadata, OfficeEvent};

/// Errors from file operations.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    /// File not found.
    #[error("file not found: {0}")]
    NotFound(String),
    /// Path already exists.
    #[error("path already exists: {0}")]
    AlreadyExists(String),
    /// Invalid path.
    #[error("invalid path: {0}")]
    InvalidPath(String),
    /// Parent directory not found.
    #[error("parent directory not found: {0}")]
    ParentNotFound(String),
    /// Ledger operation failed.
    #[error("ledger error: {0}")]
    Ledger(#[from] ledger_core::apps::AppError),
    /// Serialization failed.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Virtual file entry.
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Virtual path.
    pub path: String,
    /// Content reference (None for directories).
    pub content_ref: Option<ContentRef>,
    /// File metadata.
    pub metadata: FileMetadata,
}

impl FileEntry {
    /// Check if this is a directory.
    pub fn is_directory(&self) -> bool {
        self.metadata.is_directory
    }

    /// Get the file name (last path component).
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// File manager application orchestrator.
pub struct FileManagerApp {
    ledger: Ledger,
    signer: Arc<SigningKey>,
    channel: String,
    schema_version: u16,
    /// Virtual file system.
    files: HashMap<String, FileEntry>,
}

impl FileManagerApp {
    /// Create a new file manager application.
    pub fn new(
        ledger: Ledger,
        signer: SigningKey,
        channel: impl Into<String>,
        schema_version: u16,
    ) -> Self {
        let mut app = Self {
            ledger,
            signer: Arc::new(signer),
            channel: channel.into(),
            schema_version,
            files: HashMap::new(),
        };
        // Create root directory
        app.files.insert("/".to_string(), FileEntry {
            path: "/".to_string(),
            content_ref: None,
            metadata: FileMetadata {
                size: 0,
                mime_type: None,
                created_at: now_millis(),
                modified_at: now_millis(),
                is_directory: true,
            },
        });
        app
    }

    /// Normalize a path (remove trailing slashes, handle ../).
    fn normalize_path(path: &str) -> String {
        let mut parts: Vec<&str> = Vec::new();
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => { parts.pop(); }
                p => parts.push(p),
            }
        }
        if parts.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", parts.join("/"))
        }
    }

    /// Get parent path.
    fn parent_path(path: &str) -> Option<String> {
        if path == "/" {
            return None;
        }
        let normalized = Self::normalize_path(path);
        if let Some(pos) = normalized.rfind('/') {
            if pos == 0 {
                Some("/".to_string())
            } else {
                Some(normalized[..pos].to_string())
            }
        } else {
            Some("/".to_string())
        }
    }

    /// Append an office event to the ledger.
    fn append_office_event(&self, event: OfficeEvent) -> Result<AppendReceipt, FileError> {
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
        self.ledger.append(env).map_err(|e| FileError::Ledger(
            ledger_core::apps::AppError::Ledger(e)
        ))
    }

    /// Create a directory.
    pub fn create_directory(&mut self, path: impl Into<String>) -> Result<AppendReceipt, FileError> {
        let path = Self::normalize_path(&path.into());

        if self.files.contains_key(&path) {
            return Err(FileError::AlreadyExists(path));
        }

        // Check parent exists
        if let Some(parent) = Self::parent_path(&path) {
            if !self.files.contains_key(&parent) {
                return Err(FileError::ParentNotFound(parent));
            }
            if !self.files[&parent].is_directory() {
                return Err(FileError::InvalidPath(format!("{} is not a directory", parent)));
            }
        }

        let event = OfficeEvent::DirectoryCreated { path: path.clone() };
        let receipt = self.append_office_event(event)?;

        self.files.insert(path.clone(), FileEntry {
            path,
            content_ref: None,
            metadata: FileMetadata {
                size: 0,
                mime_type: None,
                created_at: now_millis(),
                modified_at: now_millis(),
                is_directory: true,
            },
        });

        Ok(receipt)
    }

    /// Store a file.
    pub fn store_file(
        &mut self,
        path: impl Into<String>,
        content: Vec<u8>,
        mime_type: Option<String>,
    ) -> Result<AppendReceipt, FileError> {
        let path = Self::normalize_path(&path.into());

        // Check parent exists
        if let Some(parent) = Self::parent_path(&path) {
            if !self.files.contains_key(&parent) {
                return Err(FileError::ParentNotFound(parent));
            }
            if !self.files[&parent].is_directory() {
                return Err(FileError::InvalidPath(format!("{} is not a directory", parent)));
            }
        }

        let size = content.len() as u64;
        let digest = self.ledger.content_store().put(content);
        let content_ref = ContentRef {
            locator: format!("cas:{}", hex::encode(digest)),
            hash: digest,
            media_type: mime_type.clone(),
            bytes: Some(size),
        };

        let metadata = FileMetadata {
            size,
            mime_type,
            created_at: now_millis(),
            modified_at: now_millis(),
            is_directory: false,
        };

        let event = OfficeEvent::FileStored {
            path: path.clone(),
            content: content_ref.clone(),
            metadata: metadata.clone(),
        };
        let receipt = self.append_office_event(event)?;

        self.files.insert(path.clone(), FileEntry {
            path,
            content_ref: Some(content_ref),
            metadata,
        });

        Ok(receipt)
    }

    /// Delete a file or directory.
    pub fn delete(&mut self, path: impl Into<String>, reason: impl Into<String>) -> Result<AppendReceipt, FileError> {
        let path = Self::normalize_path(&path.into());

        if path == "/" {
            return Err(FileError::InvalidPath("cannot delete root".into()));
        }

        if !self.files.contains_key(&path) {
            return Err(FileError::NotFound(path));
        }

        // Check if directory is empty
        let entry = &self.files[&path];
        if entry.is_directory() {
            let has_children = self.files.keys()
                .any(|p| p != &path && p.starts_with(&format!("{}/", path)));
            if has_children {
                return Err(FileError::InvalidPath("directory not empty".into()));
            }
        }

        let event = OfficeEvent::FileDeleted {
            path: path.clone(),
            reason: reason.into(),
        };
        let receipt = self.append_office_event(event)?;

        self.files.remove(&path);
        Ok(receipt)
    }

    /// Move a file or directory.
    pub fn move_file(
        &mut self,
        from: impl Into<String>,
        to: impl Into<String>,
    ) -> Result<AppendReceipt, FileError> {
        let from = Self::normalize_path(&from.into());
        let to = Self::normalize_path(&to.into());

        if !self.files.contains_key(&from) {
            return Err(FileError::NotFound(from));
        }

        if self.files.contains_key(&to) {
            return Err(FileError::AlreadyExists(to));
        }

        // Check destination parent exists
        if let Some(parent) = Self::parent_path(&to) {
            if !self.files.contains_key(&parent) {
                return Err(FileError::ParentNotFound(parent));
            }
        }

        let event = OfficeEvent::FileMoved {
            from: from.clone(),
            to: to.clone(),
        };
        let receipt = self.append_office_event(event)?;

        // Move the entry
        if let Some(mut entry) = self.files.remove(&from) {
            entry.path = to.clone();
            entry.metadata.modified_at = now_millis();
            self.files.insert(to, entry);
        }

        Ok(receipt)
    }

    /// Get a file entry.
    pub fn get(&self, path: &str) -> Option<&FileEntry> {
        let path = Self::normalize_path(path);
        self.files.get(&path)
    }

    /// List directory contents.
    pub fn list_directory(&self, path: &str) -> Result<Vec<&FileEntry>, FileError> {
        let path = Self::normalize_path(path);

        let entry = self.files.get(&path)
            .ok_or_else(|| FileError::NotFound(path.clone()))?;

        if !entry.is_directory() {
            return Err(FileError::InvalidPath(format!("{} is not a directory", path)));
        }

        let prefix = if path == "/" { "/".to_string() } else { format!("{}/", path) };
        let mut entries: Vec<&FileEntry> = self.files.values()
            .filter(|e| {
                if e.path == path { return false; }
                if !e.path.starts_with(&prefix) { return false; }
                // Only immediate children
                let rest = &e.path[prefix.len()..];
                !rest.contains('/')
            })
            .collect();

        entries.sort_by(|a, b| {
            // Directories first, then by name
            match (a.is_directory(), b.is_directory()) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name().cmp(b.name()),
            }
        });

        Ok(entries)
    }

    /// Read file content from CAS.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, FileError> {
        let path = Self::normalize_path(path);
        let entry = self.files.get(&path)
            .ok_or_else(|| FileError::NotFound(path.clone()))?;

        if entry.is_directory() {
            return Err(FileError::InvalidPath(format!("{} is a directory", path)));
        }

        let content_ref = entry.content_ref.as_ref()
            .ok_or_else(|| FileError::NotFound(path.clone()))?;

        self.ledger.content_store().get(&content_ref.hash)
            .ok_or_else(|| FileError::NotFound(path))
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

    fn test_app() -> FileManagerApp {
        let signer = SigningKey::generate(&mut OsRng);
        let mut registry = ChannelRegistry::new();
        registry.upsert(ChannelSpec {
            name: "office.files".into(),
            policy: ledger_spec::ChannelPolicy {
                min_signers: 1,
                allowed_signers: vec![signer.verifying_key().to_bytes()],
                require_attestations: false,
                enforce_timestamp_ordering: true,
            },
        });
        let ledger = Ledger::new(registry);
        FileManagerApp::new(ledger, signer, "office.files", 1)
    }

    #[test]
    fn create_directory_and_file() {
        let mut app = test_app();

        let receipt = app.create_directory("/docs").unwrap();
        assert!(receipt.merkle.verify());

        let receipt2 = app.store_file(
            "/docs/readme.txt",
            b"Hello, world!".to_vec(),
            Some("text/plain".into()),
        ).unwrap();
        assert!(receipt2.merkle.verify());

        let entries = app.list_directory("/docs").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name(), "readme.txt");
    }

    #[test]
    fn read_file_content() {
        let mut app = test_app();

        app.create_directory("/data").unwrap();
        app.store_file("/data/test.bin", vec![1, 2, 3, 4, 5], None).unwrap();

        let content = app.read_file("/data/test.bin").unwrap();
        assert_eq!(content, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn move_file() {
        let mut app = test_app();

        app.create_directory("/src").unwrap();
        app.create_directory("/dst").unwrap();
        app.store_file("/src/file.txt", b"content".to_vec(), None).unwrap();

        let receipt = app.move_file("/src/file.txt", "/dst/moved.txt").unwrap();
        assert!(receipt.merkle.verify());

        assert!(app.get("/src/file.txt").is_none());
        assert!(app.get("/dst/moved.txt").is_some());
    }

    #[test]
    fn delete_file() {
        let mut app = test_app();

        app.store_file("/temp.txt", b"temp".to_vec(), None).unwrap();
        assert!(app.get("/temp.txt").is_some());

        let receipt = app.delete("/temp.txt", "cleanup").unwrap();
        assert!(receipt.merkle.verify());
        assert!(app.get("/temp.txt").is_none());
    }

    #[test]
    fn path_normalization() {
        assert_eq!(FileManagerApp::normalize_path("/a/b/c"), "/a/b/c");
        assert_eq!(FileManagerApp::normalize_path("/a/b/c/"), "/a/b/c");
        assert_eq!(FileManagerApp::normalize_path("/a/../b"), "/b");
        assert_eq!(FileManagerApp::normalize_path("/a/./b"), "/a/b");
        assert_eq!(FileManagerApp::normalize_path(""), "/");
    }
}
