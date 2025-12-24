//! Ledger core library: envelope signing/verification, append-only log,
//! Merkle segmenter, checkpoint writer, and replay validator.
#![deny(missing_docs)]

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use blake3::Hasher;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

pub use ledger_spec::{envelope_hash, hash_body};
use ledger_spec::{
    Attestation, ChannelRegistry, ChannelState, Envelope, Signature, ValidationError,
};

/// Base application orchestrators (audit terminal, privacy analyzer, agency assistant).
pub mod apps;
/// Brainstem ledger orchestration: append flow, query surfaces, and receipts.
pub mod brainstem;
/// Lifecycle management and enforcement for muscles.
pub mod lifecycle;
/// Pluggable policy enforcement and decision emission.
pub mod policy;

/// Append-only log identifier.
pub type LogId = String;

/// Errors emitted by append-only logs (validation + storage).
#[derive(Debug, thiserror::Error)]
pub enum AppendError {
    /// Envelope failed validation.
    #[error(transparent)]
    Validation(#[from] ValidationError),
    /// Storage or I/O failure.
    #[error("storage error: {0}")]
    Storage(#[from] anyhow::Error),
}

/// Common log operations shared by in-memory and persistent implementations.
pub trait AppendLogStorage: Send + Sync {
    /// Append a validated envelope.
    fn append(&self, env: Envelope, registry: &ChannelRegistry) -> Result<(), AppendError>;
    /// Append a validated envelope and return its index.
    fn append_with_index(
        &self,
        env: Envelope,
        registry: &ChannelRegistry,
    ) -> Result<usize, AppendError>;
    /// Read a slice of envelopes.
    fn read(&self, offset: usize, limit: usize) -> Vec<Envelope>;
    /// Return the length.
    fn len(&self) -> usize;
    /// Compute the Merkle root over current entries.
    fn merkle_root(&self) -> Option<[u8; 32]>;
    /// Produce a Merkle receipt for a specific log entry.
    fn receipt_for(&self, index: usize) -> Option<MerkleReceipt>;
    /// Optional storage usage hint (in bytes) for health reporting.
    fn storage_usage_bytes(&self) -> Option<u64> {
        None
    }
}

/// In-memory append-only log with hash chaining and Merkle checkpoints.
#[derive(Debug, Default, Clone)]
pub struct AppendLog {
    entries: Arc<RwLock<Vec<Envelope>>>,
}

impl AppendLog {
    /// Create a new empty log.
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Append an envelope after validation.
    pub fn append(&self, env: Envelope, registry: &ChannelRegistry) -> Result<(), AppendError> {
        self.append_with_index(env, registry).map(|_| ())
    }

    fn validate_and_append(
        &self,
        mut env: Envelope,
        registry: &ChannelRegistry,
    ) -> Result<usize, AppendError> {
        let mut entries = self.entries.write();
        let prev_hash = entries.last().map(envelope_hash);
        if env.header.prev.is_none() {
            env.header.prev = prev_hash;
        }
        let prev_state = ChannelState {
            last_hash: prev_hash,
            last_timestamp: entries.last().map(|e| e.header.timestamp),
        };
        let _ = ledger_spec::validate_envelope(&env, registry, &prev_state)?;
        let index = entries.len();
        entries.push(env);
        Ok(index)
    }

    /// Append an envelope and return its log index once validated.
    pub fn append_with_index(
        &self,
        env: Envelope,
        registry: &ChannelRegistry,
    ) -> Result<usize, AppendError> {
        let span = tracing::info_span!(
            "append_log",
            channel = %env.header.channel,
            offset = tracing::field::Empty,
            latency_ms = tracing::field::Empty
        );
        let _guard = span.enter();
        let start = std::time::Instant::now();
        let res = self.validate_and_append(env, registry);
        let elapsed = start.elapsed().as_millis() as u64;
        span.record("latency_ms", &elapsed);
        match &res {
            Ok(idx) => {
                span.record("offset", &(*idx as u64));
                tracing::debug!("append committed");
            }
            Err(err) => tracing::error!(error = %err, "append failed"),
        }
        res
    }

    /// Read a slice of envelopes.
    pub fn read(&self, offset: usize, limit: usize) -> Vec<Envelope> {
        let span = tracing::info_span!(
            "read_log",
            offset = offset as u64,
            limit = limit as u64,
            latency_ms = tracing::field::Empty
        );
        let _guard = span.enter();
        let start = std::time::Instant::now();
        let entries = self.entries.read();
        let out: Vec<_> = entries.iter().skip(offset).take(limit).cloned().collect();
        let elapsed = start.elapsed().as_millis() as u64;
        span.record("latency_ms", &elapsed);
        tracing::debug!(result_len = out.len(), "read completed");
        out
    }

    /// Return the length.
    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    /// Compute a Merkle root over current entries.
    pub fn merkle_root(&self) -> Option<[u8; 32]> {
        let entries = self.entries.read();
        if entries.is_empty() {
            return None;
        }
        let leaves: Vec<[u8; 32]> = entries.iter().map(envelope_hash).collect();
        compute_merkle_root(&leaves)
    }

    /// Produce a Merkle receipt for a specific log entry.
    pub fn receipt_for(&self, index: usize) -> Option<MerkleReceipt> {
        let entries = self.entries.read();
        if index >= entries.len() {
            return None;
        }
        let leaves: Vec<[u8; 32]> = entries.iter().map(envelope_hash).collect();
        MerkleReceipt::from_leaves(&leaves, index)
    }

    /// Estimate storage usage in bytes (approximate based on entry count).
    pub fn storage_usage_bytes(&self) -> Option<u64> {
        let entries = self.entries.read();
        if entries.is_empty() {
            return Some(0);
        }
        // Rough estimate: ~500 bytes per envelope on average
        Some((entries.len() as u64) * 500)
    }
}

impl AppendLogStorage for AppendLog {
    fn append(&self, env: Envelope, registry: &ChannelRegistry) -> Result<(), AppendError> {
        AppendLog::append(self, env, registry)
    }

    fn append_with_index(
        &self,
        env: Envelope,
        registry: &ChannelRegistry,
    ) -> Result<usize, AppendError> {
        AppendLog::append_with_index(self, env, registry)
    }

    fn read(&self, offset: usize, limit: usize) -> Vec<Envelope> {
        AppendLog::read(self, offset, limit)
    }

    fn len(&self) -> usize {
        AppendLog::len(self)
    }

    fn merkle_root(&self) -> Option<[u8; 32]> {
        AppendLog::merkle_root(self)
    }

    fn receipt_for(&self, index: usize) -> Option<MerkleReceipt> {
        AppendLog::receipt_for(self, index)
    }

    fn storage_usage_bytes(&self) -> Option<u64> {
        Some(0)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct PersistentMetadata {
    length: usize,
    root: Option<[u8; 32]>,
}

#[derive(Debug)]
struct PersistentState {
    entries: Vec<Envelope>,
    wal_entries: usize,
}

impl PersistentMetadata {
    fn from_state(state: &PersistentState) -> Self {
        Self {
            length: state.entries.len(),
            root: merkle_root_for(&state.entries),
        }
    }
}

/// Disk-backed append log with checksummed WAL and segment compaction.
#[derive(Debug, Clone)]
pub struct PersistentAppendLog {
    state: Arc<RwLock<PersistentState>>,
    wal: Arc<Mutex<File>>,
    segments: Arc<Mutex<File>>,
    dir: PathBuf,
    meta_path: PathBuf,
    wal_path: PathBuf,
    segment_size: usize,
}

const DEFAULT_SEGMENT_SIZE: usize = 1024;
const CHECKSUM_DOMAIN: &[u8] = b"ea-ledger:wal:v1";

fn read_metadata_file(path: &Path) -> Option<PersistentMetadata> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PersistentMetadata>(&bytes).ok())
}

impl PersistentAppendLog {
    /// Open (or create) a persistent log at `dir` with the default segment size.
    pub fn open<P: AsRef<Path>>(dir: P) -> Result<Self, AppendError> {
        Self::open_with_segment_size(dir, DEFAULT_SEGMENT_SIZE)
    }

    /// Open (or create) a persistent log at `dir` with a custom segment size.
    pub fn open_with_segment_size<P: AsRef<Path>>(
        dir: P,
        segment_size: usize,
    ) -> Result<Self, AppendError> {
        let dir = dir.as_ref();
        let segment_size = segment_size.max(1);
        fs::create_dir_all(dir)
            .with_context(|| format!("failed to create log directory {}", dir.display()))?;
        let wal_path = dir.join("append.wal");
        let segments_path = dir.join("segments.bin");
        let meta_path = dir.join("meta.json");
        let wal_entries = read_records(&wal_path)?;
        let mut entries = read_records(&segments_path)?;
        let wal_count = wal_entries.len();
        entries.extend(wal_entries);
        let current_meta = PersistentMetadata {
            length: entries.len(),
            root: merkle_root_for(&entries),
        };
        if let Some(on_disk) = read_metadata_file(&meta_path) {
            if on_disk != current_meta {
                return Err(anyhow::anyhow!("persistent log metadata mismatch during recovery").into());
            }
        }

        let wal = Arc::new(Mutex::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .read(true)
                .open(&wal_path)
                .with_context(|| format!("failed to open WAL {}", wal_path.display()))?,
        ));
        let segments = Arc::new(Mutex::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .read(true)
                .open(&segments_path)
                .with_context(|| format!("failed to open segments {}", segments_path.display()))?,
        ));
        let log = Self {
            state: Arc::new(RwLock::new(PersistentState {
                entries,
                wal_entries: wal_count,
            })),
            wal,
            segments,
            dir: dir.to_path_buf(),
            meta_path,
            wal_path,
            segment_size,
        };
        log.ensure_metadata()?;
        Ok(log)
    }

    fn ensure_metadata(&self) -> Result<(), AppendError> {
        let state = self.state.read();
        let expected = PersistentMetadata::from_state(&state);
        match read_metadata_file(&self.meta_path) {
            Some(on_disk) if on_disk == expected => Ok(()),
            _ => self.persist_metadata(&expected),
        }
    }

    fn persist_metadata(&self, meta: &PersistentMetadata) -> Result<(), AppendError> {
        let tmp = self.meta_path.with_extension("tmp");
        let encoded =
            serde_json::to_vec(meta).context("failed to serialize persistent log metadata")?;
        fs::write(&tmp, encoded)
            .with_context(|| format!("failed to write metadata {}", tmp.display()))?;
        fs::rename(&tmp, &self.meta_path).with_context(|| {
            format!(
                "failed to atomically persist metadata {} -> {}",
                tmp.display(),
                self.meta_path.display()
            )
        })?;
        Ok(())
    }

    fn write_wal(&self, env: &Envelope) -> Result<(), AppendError> {
        let mut wal = self.wal.lock();
        // Use JSON instead of bincode because Envelope contains serde_json::Value
        let bytes = serde_json::to_vec(env).context("failed to serialize envelope")?;
        let mut hasher = Hasher::new();
        hasher.update(CHECKSUM_DOMAIN);
        hasher.update(&bytes);
        let digest = hasher.finalize();
        let len = (bytes.len() as u32).to_be_bytes();
        wal.write_all(&len)
            .context("failed to write wal length prefix")?;
        wal.write_all(digest.as_bytes())
            .context("failed to write wal checksum")?;
        wal.write_all(&bytes)
            .context("failed to write wal entry body")?;
        wal.flush().context("failed to flush wal")?;
        wal.sync_all().context("failed to sync wal to disk")?;
        Ok(())
    }

    fn compact_segments(&self) -> Result<(), AppendError> {
        let wal_bytes = fs::read(&self.wal_path).unwrap_or_default();
        if wal_bytes.is_empty() {
            return Ok(());
        }
        {
            let mut segments = self.segments.lock();
            segments
                .write_all(&wal_bytes)
                .context("failed to write compacted wal into segments")?;
            segments
                .sync_all()
                .context("failed to sync compacted segments")?;
        }
        {
            let mut wal = self.wal.lock();
            wal.set_len(0).context("failed to truncate wal")?;
            wal.seek(SeekFrom::End(0))
                .context("failed to reset wal cursor")?;
            wal.sync_all().context("failed to sync truncated wal")?;
        }
        let mut state = self.state.write();
        state.wal_entries = 0;
        Ok(())
    }

    fn metadata(&self) -> Option<PersistentMetadata> {
        read_metadata_file(&self.meta_path)
    }
}

impl AppendLogStorage for PersistentAppendLog {
    fn append(&self, env: Envelope, registry: &ChannelRegistry) -> Result<(), AppendError> {
        self.append_with_index(env, registry).map(|_| ())
    }

    fn append_with_index(
        &self,
        mut env: Envelope,
        registry: &ChannelRegistry,
    ) -> Result<usize, AppendError> {
        let span = tracing::info_span!(
            "append_persistent_log",
            channel = %env.header.channel,
            offset = tracing::field::Empty,
            latency_ms = tracing::field::Empty
        );
        let _guard = span.enter();
        let start = std::time::Instant::now();
        let mut state = self.state.write();
        let prev_hash = state.entries.last().map(envelope_hash);
        if env.header.prev.is_none() {
            env.header.prev = prev_hash;
        }
        let prev_state = ChannelState {
            last_hash: prev_hash,
            last_timestamp: state.entries.last().map(|e| e.header.timestamp),
        };
        let _ = ledger_spec::validate_envelope(&env, registry, &prev_state)?;
        let index = state.entries.len();
        self.write_wal(&env)?;
        state.entries.push(env);
        state.wal_entries += 1;
        let meta = PersistentMetadata {
            length: state.entries.len(),
            root: merkle_root_for(&state.entries),
        };
        drop(state);
        self.persist_metadata(&meta)?;
        if meta.length % self.segment_size == 0 {
            self.compact_segments()?;
        }
        let elapsed = start.elapsed().as_millis() as u64;
        span.record("offset", &(index as u64));
        span.record("latency_ms", &elapsed);
        tracing::debug!("append committed to wal");
        Ok(index)
    }

    fn read(&self, offset: usize, limit: usize) -> Vec<Envelope> {
        let span = tracing::info_span!(
            "read_persistent_log",
            offset = offset as u64,
            limit = limit as u64,
            latency_ms = tracing::field::Empty
        );
        let _guard = span.enter();
        let start = std::time::Instant::now();
        let state = self.state.read();
        let out: Vec<Envelope> = state
            .entries
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();
        let elapsed = start.elapsed().as_millis() as u64;
        span.record("latency_ms", &elapsed);
        tracing::debug!(result_len = out.len(), "read completed");
        out
    }

    fn len(&self) -> usize {
        self.state.read().entries.len()
    }

    fn merkle_root(&self) -> Option<[u8; 32]> {
        let state = self.state.read();
        merkle_root_for(&state.entries)
    }

    fn receipt_for(&self, index: usize) -> Option<MerkleReceipt> {
        let state = self.state.read();
        if index >= state.entries.len() {
            return None;
        }
        let leaves: Vec<[u8; 32]> = state.entries.iter().map(envelope_hash).collect();
        MerkleReceipt::from_leaves(&leaves, index)
    }

    fn storage_usage_bytes(&self) -> Option<u64> {
        let wal = std::fs::metadata(&self.wal_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let seg = std::fs::metadata(&self.dir.join("segments.bin"))
            .map(|m| m.len())
            .unwrap_or(0);
        let meta = std::fs::metadata(&self.meta_path)
            .map(|m| m.len())
            .unwrap_or(0);
        Some(wal + seg + meta)
    }
}

fn read_records(path: &Path) -> Result<Vec<Envelope>, AppendError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut file =
        File::open(path).with_context(|| format!("failed to open log file {}", path.display()))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .with_context(|| format!("failed to read log file {}", path.display()))?;
    let mut cursor = 0usize;
    let mut items = Vec::new();
    while cursor < buf.len() {
        if cursor + 4 > buf.len() {
            return Err(anyhow::anyhow!("truncated record length in {}", path.display()).into());
        }
        let len = u32::from_be_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        if cursor + 32 + len > buf.len() {
            return Err(anyhow::anyhow!("truncated record body in {}", path.display()).into());
        }
        let checksum: [u8; 32] = buf[cursor..cursor + 32].try_into().unwrap();
        cursor += 32;
        let payload = &buf[cursor..cursor + len];
        cursor += len;
        let mut hasher = Hasher::new();
        hasher.update(CHECKSUM_DOMAIN);
        hasher.update(payload);
        let digest = hasher.finalize();
        if *digest.as_bytes() != checksum {
            return Err(anyhow::anyhow!("checksum mismatch in {}", path.display()).into());
        }
        let env: Envelope =
            serde_json::from_slice(payload).context("failed to decode envelope from wal")?;
        items.push(env);
    }
    Ok(items)
}
/// Checkpoint record capturing merkle root and length.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Checkpoint {
    /// Log length at checkpoint.
    pub length: usize,
    /// Merkle root.
    pub root: [u8; 32],
}

/// Checkpoint writer produces periodic checkpoints.
#[derive(Debug, Default)]
pub struct CheckpointWriter {
    last_len: usize,
}

impl CheckpointWriter {
    /// Create new writer.
    pub fn new() -> Self {
        Self { last_len: 0 }
    }

    /// Emit a checkpoint if log advanced by at least `interval`.
    pub fn maybe_checkpoint(&mut self, log: &AppendLog, interval: usize) -> Option<Checkpoint> {
        let len = log.len();
        if len >= self.last_len + interval {
            let root = log.merkle_root()?;
            self.last_len = len;
            return Some(Checkpoint { length: len, root });
        }
        None
    }
}

/// Replay validator detects tampering or reordering.
#[derive(Clone)]
pub struct ReplayValidator {
    registry: ChannelRegistry,
}

impl ReplayValidator {
    /// Create new validator.
    pub fn new(registry: ChannelRegistry) -> Self {
        Self { registry }
    }

    /// Validate a sequence of envelopes starting from empty state.
    pub fn validate_sequence(&self, seq: &[Envelope]) -> Result<(), ValidationError> {
        let mut state = ChannelState::default();
        for env in seq {
            state = ledger_spec::validate_envelope(env, &self.registry, &state)?;
        }
        Ok(())
    }
}

/// Envelope signer and verifier helpers.
pub mod signing {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// Sign an envelope (header/body) with the provided key.
    pub fn sign_envelope(env: &mut Envelope, signer: &SigningKey) {
        let env_hash = envelope_hash(env);
        let sig = signer.sign(&env_hash);
        env.signatures.push(Signature {
            signer: signer.verifying_key().to_bytes(),
            signature: sig.to_bytes(),
        });
    }

    /// Attach an attestation signature over its statement hash.
    pub fn sign_attestation(att: &mut Attestation, signer: &SigningKey) {
        let sig = signer.sign(&att.statement_hash);
        att.signature = sig.to_bytes();
        att.issuer = signer.verifying_key().to_bytes();
    }
}

/// Append-only log segmenter that emits Merkle checkpoints.
#[derive(Debug)]
pub struct MerkleSegmenter {
    window: usize,
    queue: VecDeque<[u8; 32]>,
}

impl MerkleSegmenter {
    /// Create a new segmenter with a fixed window size.
    pub fn new(window: usize) -> Self {
        Self {
            window,
            queue: VecDeque::new(),
        }
    }

    /// Push a new envelope hash and emit a segment root if window filled.
    pub fn push(&mut self, env_hash: [u8; 32]) -> Option<[u8; 32]> {
        self.queue.push_back(env_hash);
        if self.queue.len() == self.window {
            let root = compute_merkle(&self.queue.make_contiguous());
            self.queue.clear();
            Some(root)
        } else {
            None
        }
    }
}

fn compute_merkle(items: &[[u8; 32]]) -> [u8; 32] {
    compute_merkle_root(items).unwrap_or([0u8; 32])
}

fn merkle_root_for(entries: &[Envelope]) -> Option<[u8; 32]> {
    if entries.is_empty() {
        return None;
    }
    let leaves: Vec<[u8; 32]> = entries.iter().map(envelope_hash).collect();
    compute_merkle_root(&leaves)
}

fn merkle_parent(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"ea-ledger:merkle");
    hasher.update(left);
    hasher.update(right);
    *hasher.finalize().as_bytes()
}

fn compute_merkle_root(items: &[[u8; 32]]) -> Option<[u8; 32]> {
    let mut leaves = items.to_vec();
    if leaves.is_empty() {
        return None;
    }
    while leaves.len() > 1 {
        leaves = leaves
            .chunks(2)
            .map(|chunk| match chunk {
                [left, right] => merkle_parent(left, right),
                [solo] => merkle_parent(solo, solo),
                _ => unreachable!(),
            })
            .collect();
    }
    leaves.into_iter().next()
}

/// Merkle path position for a sibling hash.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProofPosition {
    /// Sibling sits to the left of the node being proven.
    Left,
    /// Sibling sits to the right of the node being proven.
    Right,
}

/// A node along a Merkle proof path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofNode {
    /// The sibling hash at this level.
    pub sibling: [u8; 32],
    /// Whether the sibling is left or right of the path node.
    pub position: ProofPosition,
}

/// Receipt proving inclusion of a log entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MerkleReceipt {
    /// Index of the leaf in the log.
    pub index: usize,
    /// Total leaf count at time of receipt generation.
    pub leaf_count: usize,
    /// Hash of the leaf being proven (envelope hash).
    pub leaf: [u8; 32],
    /// Merkle root over the log at generation time.
    pub root: [u8; 32],
    /// Proof path from leaf to root.
    pub path: Vec<ProofNode>,
}

impl MerkleReceipt {
    /// Build a receipt from a set of leaves and a target index.
    pub fn from_leaves(leaves: &[[u8; 32]], index: usize) -> Option<Self> {
        if leaves.is_empty() || index >= leaves.len() {
            return None;
        }

        let mut path = Vec::new();
        let mut current_index = index;
        let mut level = leaves.to_vec();

        while level.len() > 1 {
            let sibling_index = if current_index % 2 == 0 {
                current_index + 1
            } else {
                current_index - 1
            };
            let sibling = if sibling_index < level.len() {
                level[sibling_index]
            } else {
                level[current_index]
            };
            let position = if current_index % 2 == 0 {
                ProofPosition::Right
            } else {
                ProofPosition::Left
            };
            path.push(ProofNode { sibling, position });

            let mut next_level = Vec::with_capacity((level.len() + 1) / 2);
            for chunk in level.chunks(2) {
                match chunk {
                    [left, right] => next_level.push(merkle_parent(left, right)),
                    [solo] => next_level.push(merkle_parent(solo, solo)),
                    _ => unreachable!(),
                }
            }
            current_index /= 2;
            level = next_level;
        }

        Some(MerkleReceipt {
            index,
            leaf_count: leaves.len(),
            leaf: leaves[index],
            root: level[0],
            path,
        })
    }

    /// Verify this receipt against the embedded root.
    pub fn verify(&self) -> bool {
        if self.path.is_empty() && self.leaf_count != 1 {
            return false;
        }
        let mut hash = self.leaf;
        for node in &self.path {
            hash = match node.position {
                ProofPosition::Left => merkle_parent(&node.sibling, &hash),
                ProofPosition::Right => merkle_parent(&hash, &node.sibling),
            };
        }
        hash == self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use ledger_spec::{EnvelopeBody, EnvelopeHeader};
    use rand_core::OsRng;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sample_env(prev: Option<[u8; 32]>, ts: u64, sk: &SigningKey) -> Envelope {
        let body = EnvelopeBody {
            payload: serde_json::json!({"n": ts}),
            payload_type: Some("test".into()),
        };
        let body_hash = hash_body(&body);
        let header = EnvelopeHeader {
            channel: "muscle_io".into(),
            version: 1,
            prev,
            body_hash,
            timestamp: ts,
        };
        let mut env = Envelope {
            header,
            body,
            signatures: Vec::new(),
            attestations: Vec::new(),
        };
        signing::sign_envelope(&mut env, sk);
        env
    }

    fn registry(sk: &SigningKey) -> ChannelRegistry {
        let mut registry = ChannelRegistry::new();
        registry.upsert(ledger_spec::ChannelSpec {
            name: "muscle_io".into(),
            policy: ledger_spec::ChannelPolicy {
                min_signers: 1,
                allowed_signers: vec![sk.verifying_key().to_bytes()],
                require_attestations: false,
                enforce_timestamp_ordering: true,
            },
        });
        registry
    }

    #[test]
    fn append_and_checkpoint() {
        let sk = SigningKey::generate(&mut OsRng);
        let reg = registry(&sk);
        let log = AppendLog::new();
        let mut prev = None;
        for ts in 1..=3 {
            let env = sample_env(prev, ts, &sk);
            prev = Some(envelope_hash(&env));
            log.append(env, &reg).unwrap();
        }
        assert_eq!(log.len(), 3);
        let mut writer = CheckpointWriter::new();
        let cp = writer.maybe_checkpoint(&log, 2).unwrap();
        assert_eq!(cp.length, 3);
        assert!(cp.root.iter().any(|b| *b != 0));
    }

    #[test]
    fn merkle_segmenter_emits_root() {
        let sk = SigningKey::generate(&mut OsRng);
        let mut segmenter = MerkleSegmenter::new(2);
        let mut prev = None;
        for ts in 1..=2 {
            let env = sample_env(prev, ts, &sk);
            prev = Some(envelope_hash(&env));
            let root = segmenter.push(envelope_hash(&env));
            if ts == 2 {
                assert!(root.is_some());
            }
        }
    }

    #[test]
    fn replay_validator_detects_tamper() {
        let sk = SigningKey::generate(&mut OsRng);
        let reg = registry(&sk);
        let validator = ReplayValidator::new(reg);
        let env1 = sample_env(None, 1, &sk);
        let mut env2 = sample_env(Some(envelope_hash(&env1)), 2, &sk);
        // Tamper body without updating hash
        env2.body.payload = serde_json::json!({"n": 99});
        let seq = vec![env1, env2];
        let err = validator.validate_sequence(&seq).unwrap_err();
        assert_eq!(err, ValidationError::BodyHashMismatch);
    }

    #[test]
    fn merkle_receipt_roundtrip() {
        let sk = SigningKey::generate(&mut OsRng);
        let reg = registry(&sk);
        let log = AppendLog::new();
        let mut prev = None;
        for ts in 1..=4 {
            let env = sample_env(prev, ts, &sk);
            prev = Some(envelope_hash(&env));
            log.append(env, &reg).unwrap();
        }
        let receipt = log.receipt_for(2).expect("receipt exists");
        assert!(receipt.verify());
        assert_eq!(receipt.index, 2);
    }

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("ledger-core-{prefix}-{nanos}"));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn persistent_log_recovers_merkle_root() {
        let sk = SigningKey::generate(&mut OsRng);
        let reg = registry(&sk);
        let dir = temp_dir("recover");
        let log = PersistentAppendLog::open(&dir).expect("create persistent log");
        let mut prev = None;
        for ts in 1..=3 {
            let env = sample_env(prev, ts, &sk);
            prev = Some(envelope_hash(&env));
            log.append(env, &reg).expect("append to persistent log");
        }
        let expected_root = log.merkle_root().expect("root exists");
        let expected_len = log.len();
        drop(log);
        let recovered = PersistentAppendLog::open(&dir).expect("reopen persistent log");
        assert_eq!(recovered.len(), expected_len);
        assert_eq!(recovered.merkle_root().unwrap(), expected_root);
        assert!(recovered.receipt_for(1).unwrap().verify());
    }

    #[test]
    fn persistent_log_compacts_segments() {
        let sk = SigningKey::generate(&mut OsRng);
        let reg = registry(&sk);
        let dir = temp_dir("compact");
        let log = PersistentAppendLog::open_with_segment_size(&dir, 2).unwrap();
        let mut prev = None;
        for ts in 1..=4 {
            let env = sample_env(prev, ts, &sk);
            prev = Some(envelope_hash(&env));
            log.append(env, &reg).unwrap();
        }
        // Wal should have compacted after hitting the segment size twice.
        let wal_path = dir.join("append.wal");
        let wal_bytes = std::fs::read(&wal_path).unwrap_or_default();
        assert!(
            wal_bytes.is_empty(),
            "wal should be empty after compaction, got {} bytes",
            wal_bytes.len()
        );
        assert_eq!(log.len(), 4);
    }

    #[test]
    fn persistent_log_persists_metadata_across_restart() {
        let sk = SigningKey::generate(&mut OsRng);
        let reg = registry(&sk);
        let dir = temp_dir("meta");
        let log = PersistentAppendLog::open_with_segment_size(&dir, 2).unwrap();
        let mut prev = None;
        for ts in 1..=3 {
            let env = sample_env(prev, ts, &sk);
            prev = Some(envelope_hash(&env));
            log.append(env, &reg).unwrap();
        }
        let meta = log.metadata().expect("metadata persisted");
        assert_eq!(meta.length, log.len());
        assert_eq!(meta.root, log.merkle_root());
        drop(log);

        let reopened = PersistentAppendLog::open_with_segment_size(&dir, 2).unwrap();
        let idx = reopened
            .append_with_index(sample_env(prev, 4, &sk), &reg)
            .unwrap();
        assert_eq!(idx, meta.length);
        let reopened_meta = reopened.metadata().unwrap();
        assert_eq!(reopened_meta.length, reopened.len());
        assert_eq!(reopened_meta.root, reopened.merkle_root());
    }

    #[test]
    fn persistent_log_rejects_corrupt_metadata() {
        let sk = SigningKey::generate(&mut OsRng);
        let reg = registry(&sk);
        let dir = temp_dir("meta-mismatch");
        let log = PersistentAppendLog::open(&dir).unwrap();
        log.append(sample_env(None, 1, &sk), &reg).unwrap();
        let meta_path = dir.join("meta.json");
        let mut meta: PersistentMetadata =
            serde_json::from_slice(&std::fs::read(&meta_path).unwrap()).unwrap();
        meta.length += 1;
        std::fs::write(&meta_path, serde_json::to_vec(&meta).unwrap()).unwrap();
        let err = PersistentAppendLog::open(&dir).unwrap_err();
        assert!(err.to_string().contains("metadata mismatch"));
    }
}
