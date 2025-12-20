# Eä OS / Arda Event & Ledger Protocol

This document defines the canonical event and ledger protocol for the Eä OS / Arda stack. It covers block layout, envelope semantics, transports, validation and attestation evidence, and backward-compatible adapters to keep nodes in lockstep while the platform evolves.

## 1. Goals and design principles

- **Append-only verifiability**: every write is a Merkle-linked block with immutable history and deterministic replay.
- **Typed, versioned envelopes**: explicit domains and event classes (Command, Observation, Result, Alert, Attestation, CapabilityAdvertisement) isolate responsibilities while preserving forward evolution.
- **Transport-agnostic**: QUIC/gRPC, VM mailbox, and enclave bridges share a single envelope contract with capability negotiation.
- **Deterministic verification**: hashes, signatures, and policy hooks are machine-checkable across JSON and CBOR encodings.
- **Backwards safety**: adapters and downgrade policies keep older peers functional without breaking audit trails.

## 2. Ledger block model (append-only, Merkle-linked)

Each channel is an append-only sequence of **blocks**. A block contains a bounded batch of envelopes and links to its predecessor.

```
Block {
  header: {
    protocol_version: u16,
    channel: string,
    height: uint64,
    prev_block_hash: bytes32,   // BLAKE3 over prior block header
    merkle_root: bytes32,       // Merkle root over ordered envelope hashes
    timestamp: uint64,          // nanoseconds since Unix epoch
    origin_identity: bytes32,   // hash of verifier key / domain key
    integrity_proof: bytes?,    // lattice proof or SNARK for the batch
  },
  envelopes: [Envelope...],
  signature: bytes?,            // signer over header + merkle_root
}
```

**Merkle strategy**: each envelope hash is `BLAKE3("ea:envelope:v1" || canonical_envelope_bytes)`. Envelope hashes are ordered as appended; the Merkle root is computed with left-balanced concatenation. Blocks are immutable; forks are resolved by policy (e.g., longest valid chain per channel) at a higher layer.

## 3. Envelope and event surface

### 3.1 Envelope fields

| Field | Type | Purpose |
|-------|------|---------|
| `protocol_version` | `string` (semver) | Envelope schema version; drives adapter selection.
| `domain` | `string` | Logical boundary (e.g., `ea.core`, `arda.ui`, `compliance`).
| `event_class` | `string` | One of `Command`, `Observation`, `Result`, `Alert`, `Attestation`, `CapabilityAdvertisement`.
| `event_type` | `string` | Domain-specific event discriminator (e.g., `muscle.invoke`, `audit.export_ready`).
| `correlation_id` | `string` (UUID/Hash) | Correlates causally related envelopes.
| `causality_chain` | `array<string>` | Ordered ancestry of envelope ids for replay and blame mapping.
| `timestamp` | `string` (RFC3339 or integer nanos) | Authoritative creation time.
| `origin_identity` | `object` | `{ identity_type: "ed25519"|"p256"|"attested-domain", public_key: string, label?: string }`.
| `attestation_evidence` | `object|null` | Structured evidence bundle (see §6).
| `payload_hash` | `string` (hex/base64) | Hash of the canonical payload bytes.
| `payload_ref` | `object|null` | `{ locator: uri, content_type?: string, bytes?: uint64 }` for detached payloads.
| `integrity_proof` | `object|null` | `{ type: "merkle"|"snark"|"lattice", proof: string }`.
| `confidentiality_tag` | `object|null` | `{ level: "public"|"internal"|"confidential"|"restricted", key_ref?: string, enclave_ref?: string }`.
| `policy_tag` | `object|null` | `{ policy_hash: string, policy_version?: string, decision?: "allow"|"audit"|"deny" }`.
| `signature` | `object` | `{ alg: "ed25519"|"p256"|"secp256k1", sig: string, signed_fields: ["protocol_version", ...] }`.
| `payload` | `any` | Event body (JSON or CBOR). Must match declared `event_type` schema.

### 3.2 Event class guidelines

- **Command**: initiates actions; must carry `correlation_id` and optional `causality_chain` seed.
- **Observation**: telemetry/measurements; generally broadcast; may omit `correlation_id` if standalone.
- **Result**: response to a Command; must include originating `correlation_id` and append to `causality_chain`.
- **Alert**: time-sensitive notification; policy may enforce multi-channel fan-out.
- **Attestation**: publishes build/runtime attestation evidence; `attestation_evidence` required.
- **CapabilityAdvertisement**: negotiates transport and feature flags; see §5.

## 4. JSON and CBOR schemas

### 4.1 JSON Schema (Draft 2020-12)

```json
{
  "$id": "https://ea.foundation/schemas/envelope.v1.json",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "required": [
    "protocol_version",
    "domain",
    "event_class",
    "event_type",
    "timestamp",
    "origin_identity",
    "payload_hash",
    "signature",
    "payload"
  ],
  "properties": {
    "protocol_version": { "type": "string", "pattern": "^[0-9]+\.[0-9]+\.[0-9]+$" },
    "domain": { "type": "string", "minLength": 1 },
    "event_class": { "enum": ["Command", "Observation", "Result", "Alert", "Attestation", "CapabilityAdvertisement"] },
    "event_type": { "type": "string", "minLength": 1 },
    "correlation_id": { "type": "string" },
    "causality_chain": { "type": "array", "items": {"type": "string"}, "uniqueItems": false },
    "timestamp": { "oneOf": [ {"type": "string", "format": "date-time"}, {"type": "integer", "minimum": 0} ] },
    "origin_identity": {
      "type": "object",
      "required": ["identity_type", "public_key"],
      "properties": {
        "identity_type": { "enum": ["ed25519", "p256", "secp256k1", "attested-domain"] },
        "public_key": { "type": "string" },
        "label": { "type": "string" }
      },
      "additionalProperties": false
    },
    "attestation_evidence": { "$ref": "#/$defs/attestation_evidence" },
    "payload_hash": { "type": "string", "pattern": "^[A-Fa-f0-9+/=:-]{16,}$" },
    "payload_ref": {
      "type": ["object", "null"],
      "required": ["locator"],
      "properties": {
        "locator": { "type": "string", "format": "uri" },
        "content_type": { "type": "string" },
        "bytes": { "type": "integer", "minimum": 0 }
      },
      "additionalProperties": false
    },
    "integrity_proof": {
      "type": ["object", "null"],
      "required": ["type", "proof"],
      "properties": {
        "type": { "enum": ["merkle", "snark", "lattice"] },
        "proof": { "type": "string" }
      },
      "additionalProperties": false
    },
    "confidentiality_tag": {
      "type": ["object", "null"],
      "required": ["level"],
      "properties": {
        "level": { "enum": ["public", "internal", "confidential", "restricted"] },
        "key_ref": { "type": "string" },
        "enclave_ref": { "type": "string" }
      },
      "additionalProperties": false
    },
    "policy_tag": {
      "type": ["object", "null"],
      "required": ["policy_hash"],
      "properties": {
        "policy_hash": { "type": "string" },
        "policy_version": { "type": "string" },
        "decision": { "enum": ["allow", "audit", "deny"] }
      },
      "additionalProperties": false
    },
    "signature": {
      "type": "object",
      "required": ["alg", "sig", "signed_fields"],
      "properties": {
        "alg": { "enum": ["ed25519", "p256", "secp256k1"] },
        "sig": { "type": "string" },
        "signed_fields": { "type": "array", "items": {"type": "string"}, "minItems": 1 }
      },
      "additionalProperties": false
    },
    "payload": {}
  },
  "$defs": {
    "attestation_evidence": {
      "type": ["object", "null"],
      "required": ["format", "evidence_hash"],
      "properties": {
        "format": { "enum": ["tee.report", "slsa.v1", "snark.v1"] },
        "evidence_hash": { "type": "string" },
        "report": { "type": "object" },
        "cert_chain": { "type": "array", "items": {"type": "string"} },
        "nonce": { "type": "string" },
        "verification_service": { "type": "string", "format": "uri" }
      },
      "additionalProperties": false
    }
  },
  "additionalProperties": false
}
```

### 4.2 CBOR CDDL

```cddl
; Envelope v1
Envelope = {
  "protocol_version": tstr,
  "domain": tstr,
  "event_class": event-class,
  "event_type": tstr,
  ?"correlation_id": tstr,
  ?"causality_chain": [* tstr],
  "timestamp": int / tstr,
  "origin_identity": OriginIdentity,
  ?"attestation_evidence": AttestationEvidence,
  "payload_hash": bstr / tstr,
  ?"payload_ref": PayloadRef,
  ?"integrity_proof": IntegrityProof,
  ?"confidentiality_tag": ConfidentialityTag,
  ?"policy_tag": PolicyTag,
  "signature": Signature,
  "payload": any
}

event-class = "Command" / "Observation" / "Result" / "Alert" / "Attestation" / "CapabilityAdvertisement"
OriginIdentity = {
  "identity_type": "ed25519" / "p256" / "secp256k1" / "attested-domain",
  "public_key": bstr / tstr,
  ?"label": tstr
}
PayloadRef = {"locator": tstr, ?"content_type": tstr, ?"bytes": uint}
IntegrityProof = {"type": "merkle" / "snark" / "lattice", "proof": bstr / tstr}
ConfidentialityTag = {"level": "public" / "internal" / "confidential" / "restricted", ?"key_ref": tstr, ?"enclave_ref": tstr}
PolicyTag = {"policy_hash": bstr / tstr, ?"policy_version": tstr, ?"decision": "allow" / "audit" / "deny"}
Signature = {"alg": "ed25519" / "p256" / "secp256k1", "sig": bstr / tstr, "signed_fields": [1* tstr]}
AttestationEvidence = {
  "format": "tee.report" / "slsa.v1" / "snark.v1",
  "evidence_hash": bstr / tstr,
  ?"report": any,
  ?"cert_chain": [* (bstr / tstr)],
  ?"nonce": tstr,
  ?"verification_service": tstr
}
```

## 5. Transport abstraction and capability negotiation

- **CapabilityAdvertisement flow**:
  1. Initiator sends envelope (`event_class=CapabilityAdvertisement`, `event_type=transport.capability`) on the chosen channel.
  2. Payload advertises supported transports (e.g., `quic`, `grpc`, `vm_mailbox`, `enclave_bridge`), protocol versions, max message size, compression, and attestation formats.
  3. Responder replies with intersected capabilities and selected parameters, signed and optionally sealed in a TEE report.
  4. Channel policy pins the agreed tuple (`transport`, `protocol_version`, `compression`, `attestation_format`).

- **QUIC/gRPC**: use mTLS with ALPN `ea-ledger/1`. Envelope framing is length-prefixed; flow control uses QUIC streams per correlation id. Reliability and ordering provided by QUIC; backpressure signaled with HTTP/3 error codes.
- **VM mailbox**: envelopes serialized to CBOR, stored in shared memory ring; integrity via `payload_hash` + mailbox root hash signed by the hypervisor or host agent. Capability negotiation limits burst rate and maximum bytes per slot.
- **Enclave bridge**: envelopes encrypted to enclave public key; `confidentiality_tag.enclave_ref` identifies the target. Attestation evidence from the enclave accompanies CapabilityAdvertisement to bind encryption keys to measurements. A bridge process batches envelopes into blocks, anchoring Merkle roots back to the public ledger.

## 6. Attestation evidence schema

Structured envelope payload for `event_class=Attestation` or `attestation_evidence` references:

```json
{
  "format": "tee.report",               // or slsa.v1, snark.v1
  "evidence_hash": "B64(blake3(report))",
  "report": {
    "tee_type": "sgx" | "sev" | "tdx" | "aws_nitro",
    "measurement": "hex",              // MRENCLAVE or equivalent
    "report_data": "hex",              // binds to payload_hash
    "timestamp": "RFC3339",
    "issuer": "uri",
    "certificate": "pem",
    "freshness": { "nonce": "hex", "expires_at": "RFC3339" }
  },
  "cert_chain": ["pem..."],
  "verification_service": "https://attest.ea.foundation/verify",
  "nonce": "caller-provided for replay defense"
}
```

**Rules**:
- `report_data` MUST include `payload_hash || correlation_id` to bind evidence to the envelope.
- `evidence_hash` is the canonical hash stored on-ledger; `report` MAY be off-ledger via `payload_ref`.
- For `slsa.v1`, include provenance, builder id, and buildType; for `snark.v1`, include proving system id and public inputs.

## 7. Validation rules (applicable to JSON and CBOR)

1. **Canonical hashing**: compute `payload_hash` over canonical encoding (JSON RFC8785 or deterministic CBOR). Reject envelopes where provided hash mismatches.
2. **Signature scope**: verify `signature` across the ordered `signed_fields`; default ordering is `[protocol_version, domain, event_class, event_type, correlation_id, causality_chain, timestamp, origin_identity, payload_hash, integrity_proof, confidentiality_tag, policy_tag]`.
3. **Causality**: if `causality_chain` is present, its tail must equal `correlation_id`; append the envelope id when producing children. Blocks must preserve envelope order relative to causality.
4. **Protocol version gating**: nodes accept envelopes whose `protocol_version` is within their supported range or for which an adapter exists (see §8).
5. **Attestation binding**: when `attestation_evidence` exists, ensure `payload_hash` (or `payload_ref.hash`) appears in the attestation `report_data` or provenance statement.
6. **Confidentiality enforcement**: `confidentiality_tag.restricted` envelopes require policy validation before fan-out; transports without encryption MUST reject them.
7. **Integrity proof**: when present, verify proof type against the block or content referenced; Merkle proofs must resolve against the containing block root.
8. **Replay protection**: reject duplicate `correlation_id` per channel for Command and Result; Alerts may be replayed only if `policy_tag.decision` allows.

## 8. Backward-compatible adapter rules

- **Version negotiation**: during CapabilityAdvertisement, include `supported_versions: ["1.0.x", "1.1.x"]`. The responder selects the highest mutually supported minor version.
- **Field shimming**:
  - Older envelopes lacking `causality_chain` may be wrapped by adapters that synthesize it from `correlation_id` and block position.
  - If `confidentiality_tag` is absent, adapters mark it as `"public"` for hashing; downstream policy can override only by denial, not escalation.
  - Unknown `event_class` values are mapped to `Observation` for logging, with policy-driven filtering.
- **Signature bridging**: if a peer only supports subset algorithms, adapters may re-sign envelopes with a local key while preserving the original signature in `attestation_evidence.report.legacy_signature`.
- **Transport downgrade**: when QUIC is unavailable, peers fall back to gRPC over TCP; block hashes remain identical because envelope bytes are canonicalized before transport framing.
- **Schema evolution**: new optional fields must default to `null`/omitted; required field additions trigger a `protocol_version` bump and MUST be negotiated.

## 9. Example CapabilityAdvertisement payload

```json
{
  "protocol_version": "1.0.0",
  "domain": "ea.core",
  "event_class": "CapabilityAdvertisement",
  "event_type": "transport.capability",
  "correlation_id": "9f1c...",
  "timestamp": "2024-11-01T12:00:00Z",
  "origin_identity": {"identity_type": "ed25519", "public_key": "base64..."},
  "payload_hash": "b64(blake3(payload))",
  "signature": {"alg": "ed25519", "sig": "base64...", "signed_fields": ["protocol_version", "domain", "event_class", "event_type", "timestamp", "payload_hash"]},
  "payload": {
    "transports": ["quic", "grpc", "vm_mailbox", "enclave_bridge"],
    "supported_versions": ["1.0.x", "1.1.x"],
    "max_message_bytes": 1048576,
    "compression": ["none", "zstd"],
    "attestation_formats": ["tee.report", "slsa.v1"],
    "features": {"streaming": true, "retry": true}
  }
}
```

## 10. Operational guidance

- Persist canonical JSON and CBOR encodings to allow cross-language verification.
- Keep block sizes bounded (e.g., 1–4 MiB) to limit Merkle fan-out and checkpoint latency.
- Anchor block Merkle roots to an external transparency log or L2 periodically for third-party auditability.
- Enforce policy tags at ingress; reject envelopes lacking mandatory attestation formats for restricted domains.

This protocol specification is intended to be production-ready: deterministic hashing, strict validation, explicit negotiation, and backward compatibility ensure that Arda companions, muscles, and external auditors share a coherent, verifiable event stream.

## 11. Deterministic replay and forensic pipeline

- **Replay inputs**: deterministic replay runs from a trusted block store snapshot plus a frozen policy bundle (`policy_hash`, evaluator version, and capability negotiation matrix). The replay coordinator rejects input where `policy_hash` differs from the policy tag present in the block headers.
- **Execution model**: the replay engine rehydrates envelopes in canonical order, verifying Merkle proofs and signatures before emitting events to the state machine. All side effects are redirected to a sandboxed ledger state (no network I/O) to guarantee repeatability.
- **Determinism guards**: runtime must expose a deterministic clock (derived from block timestamps), stable hashing (RFC8785 JSON, deterministic CBOR), and pure reducers. Any nondeterministic hook (randomness, wall clock, external service calls) must be modeled as deterministic inputs derived from envelope payloads.
- **Checkpointing**: every N blocks (configurable, e.g., 1k) the replay engine emits a signed checkpoint `{height, state_root, policy_hash, attestation_digest}`. Checkpoints enable fast-forward replay and are themselves envelope-addressable artifacts.
- **Forensics mode**: when replay diverges, the engine emits a structured `Alert` with the divergence height, mismatching state roots, and the minimal repro bundle (block range + policy bundle + replay inputs) to allow off-cluster investigation.

## 12. Merkle-proof export bundles

- **Purpose**: allow auditors to verify any envelope or block range without access to the full ledger. Bundles are detached artifacts that chain to the canonical ledger via Merkle proofs and optional L2 anchors.
- **Format**: a `bundle.manifest.json` (or CBOR) containing `{channel, start_height, end_height, block_hashes, merkle_proofs[], payload_refs[], policy_hash, created_at, signer}` plus the referenced envelope payloads and proofs. Bundles are signed and compressed (e.g., `tar+zstd`).
- **Proof contents**: for each envelope, include the envelope bytes, its hash, sibling hashes up to the block Merkle root, and the block header signature. For range proofs, include block headers and an outer range Merkle root to prove continuity.
- **Validation workflow**: (1) verify bundle signature; (2) recompute envelope hashes; (3) verify each Merkle path to the block root; (4) verify block header signatures; (5) reconcile `policy_hash` with local policy to detect drift.
- **Streaming export**: exporters operate off follower nodes or cold replicas to avoid impacting primaries. Export jobs are idempotent, chunked by height ranges, and resumable via the manifest metadata.

## 13. Retention and rotation policies

- **Tiered retention**: hot storage keeps the last M days or N blocks for low-latency reads; warm storage retains compressed blocks and Merkle manifests; cold storage (WORM/S3 Glacier) holds immutable archives and periodic checkpoints. All tiers store cryptographic proofs to allow reconstitution.
- **Rotation cadence**: policy bundles, signing keys, and attestation roots have explicit lifetimes (e.g., 90-day rotation with 30-day overlap). Rotation events are recorded as `CapabilityAdvertisement` or `Alert` envelopes carrying the new fingerprints and effective timestamps.
- **Deletion rules**: blocks are immutable; retention expiry triggers archival, not rewrite. If legal/compliance requires removal of payloads, use `payload_ref` with tombstones while keeping hashes to preserve auditability.
- **Compaction**: optional compaction emits summary blocks `{start_height, end_height, summary_root}` with deterministic derivation from original blocks to keep replay integrity intact. Compaction outputs are themselves signed and anchored.
- **Key escrow and recovery**: policy requires dual control for key destruction and rotation; recovery drills periodically verify that archived checkpoints plus keys can rebuild a follower node in a sterile environment.

## 14. Alerting on attestation staleness and policy drift

- **Signals**:
  - `attestation_age_seconds`: derived from `timestamp` vs. current wall clock and attestation freshness window.
  - `policy_hash_mismatch`: binary metric when envelope policy tags differ from the active policy bundle hash.
  - `attestation_chain_valid`: boolean for certificate/quote validity.
- **Thresholds**: default alerts at 80% of freshness budget (e.g., TEE report older than 20 hours in a 24-hour SLA) and immediate paging for `policy_hash_mismatch` or revoked certificates.
- **Instrumentation**: emit `Observation` envelopes for health signals, mirrored into Prometheus/OpenTelemetry metrics with labels `{channel, domain, protocol_version}`. Alertmanager rules fan out to paging and audit channels.
- **Automated response**: policy drift or stale attestation triggers capability renegotiation; peers quarantine drifted nodes by refusing new sessions and backpressure replication until fresh evidence arrives.

## 15. Load and chaos testing plans

- **Load baselines**: benchmark per-transport throughput (envelopes/sec, block finalize latency, tail p99) across realistic payload sizes (1 KiB, 64 KiB, 1 MiB). Include sustained load and burst (10×) tests.
- **Replay stress**: continuously run deterministic replay against the live feed plus synthetic corruptions to validate divergence detection and checkpoint recovery time.
- **Chaos scenarios**: induce packet loss/jitter, disk full, Merkle root corruption, delayed attestation fetching, and key rotation mid-stream. Verify that alerts fire, replay rejects bad data, and healthy peers continue.
- **Scalability drills**: scale followers horizontally under fan-out, validating catch-up time and export bundle generation under load. Measure how policy evaluation latency impacts ingestion TPS.
- **Tooling**: prefer deterministic workload generators (e.g., reproducible seeds for envelope generation), tc/netem for network faults, and infra-as-code scripts to schedule chaos events. Every scenario must produce a machine-readable report with success criteria and regression baselines.

## 16. Upgrade and deprecation process

- **Version lifecycle**: each protocol version moves through `preview → active → maintenance → deprecated → disabled`. Active and maintenance versions are supported concurrently with explicit EOL dates. Capability negotiation advertises `supported_versions` and `retiring_versions` to peers.
- **Rollout**: introduce new versions behind feature flags; upgrade followers first, then primaries, validating deterministic replay on both versions with identical checkpoints. Downgrade paths are tested by replaying the latest blocks with the older adapter to confirm read-compatibility.
- **Deprecation gates**: refuse new sessions on deprecated versions after a configurable sunset; continue read-only support for a grace window to allow export bundle extraction. Alerts fire 30/14/7 days before sunset via `Alert` envelopes and ops tooling.
- **Muscle artifacts**: muscles declare the minimum and maximum supported protocol versions in their manifests. Publishing a new muscle version requires fresh attestation evidence and a compatibility test against the target protocol version set. Deprecating a protocol version triggers validation that no active muscle declares it as exclusive.
- **Documentation and change control**: every version change ships with a migration guide, replay validation transcript, and updated policy hashes. Change approvals require dual sign-off from ledger owners and security, with audit trails stored on-ledger as `Result` or `Attestation` envelopes.
