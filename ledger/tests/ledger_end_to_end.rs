use ed25519_dalek::SigningKey;
use ledger_core::{signing, AppendError, AppendLog, CheckpointWriter, ReplayValidator};
use ledger_spec::{
    Attestation, AttestationKind, ChannelPolicy, ChannelRegistry, ChannelSpec, Envelope,
    EnvelopeBody, EnvelopeHeader, ValidationError,
};
use rand_core::OsRng;

fn base_envelope(channel: &str, ts: u64, prev: Option<[u8; 32]>) -> Envelope {
    let body = EnvelopeBody {
        payload: serde_json::json!({ "ts": ts, "channel": channel }),
        payload_type: Some("e2e".into()),
    };
    let body_hash = ledger_spec::hash_body(&body);
    Envelope {
        header: EnvelopeHeader {
            channel: channel.into(),
            version: 1,
            prev,
            body_hash,
            timestamp: ts,
        },
        body,
        signatures: Vec::new(),
        attestations: Vec::new(),
    }
}

fn attach_attestation(env: &mut Envelope, issuer: &SigningKey) {
    let statement = AttestationKind::Runtime {
        runtime_id: "tee-alpha".into(),
        policy_hash: [0xAAu8; 32],
    };
    let mut att = Attestation {
        issuer: [0u8; 32],
        statement_hash: ledger_spec::hash_attestation_statement(&statement),
        signature: [0u8; 64],
        statement,
    };
    signing::sign_attestation(&mut att, issuer);
    env.attestations.push(att);
}

#[test]
fn append_validate_checkpoint_and_receipts_across_channels(
) -> Result<(), Box<dyn std::error::Error>> {
    let signer_alpha = SigningKey::generate(&mut OsRng);
    let signer_beta_one = SigningKey::generate(&mut OsRng);
    let signer_beta_two = SigningKey::generate(&mut OsRng);
    let attester = SigningKey::generate(&mut OsRng);

    let mut registry = ChannelRegistry::new();
    registry.upsert(ChannelSpec {
        name: "alpha".into(),
        policy: ChannelPolicy {
            min_signers: 1,
            allowed_signers: vec![signer_alpha.verifying_key().to_bytes()],
            require_attestations: true,
            enforce_timestamp_ordering: true,
        },
    });
    registry.upsert(ChannelSpec {
        name: "beta".into(),
        policy: ChannelPolicy {
            min_signers: 2,
            allowed_signers: vec![
                signer_beta_one.verifying_key().to_bytes(),
                signer_beta_two.verifying_key().to_bytes(),
            ],
            require_attestations: false,
            enforce_timestamp_ordering: true,
        },
    });

    let log = AppendLog::new();
    let mut prev: Option<[u8; 32]> = None;

    // Missing attestation should be rejected for channel alpha.
    let mut invalid_env = base_envelope("alpha", 1, prev);
    signing::sign_envelope(&mut invalid_env, &signer_alpha);
    let err = log
        .append(invalid_env, &registry)
        .expect_err("missing attestation must fail");
    assert!(matches!(
        err,
        AppendError::Validation(ValidationError::MissingAttestations)
    ));
    // Note: failed append doesn't add to log, so prev stays None

    // Append a valid attested envelope for alpha.
    let mut alpha_env = base_envelope("alpha", 1, prev);
    attach_attestation(&mut alpha_env, &attester);
    signing::sign_envelope(&mut alpha_env, &signer_alpha);
    prev = Some(ledger_spec::envelope_hash(&alpha_env));
    log.append(alpha_env, &registry)?;

    // Append a multi-signer envelope for beta.
    let mut beta_env = base_envelope("beta", 2, prev);
    signing::sign_envelope(&mut beta_env, &signer_beta_one);
    signing::sign_envelope(&mut beta_env, &signer_beta_two);
    prev = Some(ledger_spec::envelope_hash(&beta_env));
    log.append(beta_env, &registry)?;

    // Append another attested alpha envelope to extend the chain.
    let mut alpha_env_next = base_envelope("alpha", 3, prev);
    attach_attestation(&mut alpha_env_next, &attester);
    signing::sign_envelope(&mut alpha_env_next, &signer_alpha);
    log.append(alpha_env_next, &registry)?;

    assert_eq!(log.len(), 3);
    let mut checkpoint_writer = CheckpointWriter::new();
    let checkpoint = checkpoint_writer
        .maybe_checkpoint(&log, 2)
        .expect("checkpoint emitted");
    assert_eq!(checkpoint.length, log.len());
    assert_eq!(Some(checkpoint.root), log.merkle_root());

    let entries = log.read(0, log.len());
    let validator = ReplayValidator::new(registry.clone());
    validator
        .validate_sequence(&entries)
        .expect("replay validation passes");

    for (idx, env) in entries.iter().enumerate() {
        let receipt = log.receipt_for(idx).expect("receipt exists");
        assert!(receipt.verify(), "receipt should verify for index {idx}");
        assert_eq!(
            receipt.leaf,
            ledger_spec::envelope_hash(env),
            "receipt leaf must match envelope hash"
        );
    }

    Ok(())
}
