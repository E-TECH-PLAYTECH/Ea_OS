use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ea_lattice_ledger::*;
use ed25519_dalek::SigningKey;
use ledger_core::{signing, AppendLog};
use ledger_spec::{ChannelRegistry, Envelope, EnvelopeBody, EnvelopeHeader};
use ledger_transport::Loopback;
use rand_core::OsRng;

fn bench_generate_update(c: &mut Criterion) {
    c.bench_function("generate_update", |b| {
        let root = [0u8; 32];
        let id = [0xEAu8; 32];
        let blob = [0x77u8; MAX_BLOB];

        b.iter(|| {
            generate_update(
                black_box(id),
                black_box(1),
                black_box(blob),
                black_box(root),
            )
        });
    });
}

fn bench_verify_update(c: &mut Criterion) {
    c.bench_function("verify_update", |b| {
        let root = [0u8; 32];
        let id = [0xEAu8; 32];
        let blob = [0x77u8; MAX_BLOB];
        let update = generate_update(id, 1, blob, root);

        b.iter(|| verify_update(black_box(root), black_box(&update)));
    });
}

fn bench_square_mod_n(c: &mut Criterion) {
    c.bench_function("square_mod_n", |b| {
        let input = [0x42u8; 32];

        b.iter(|| square_mod_n(black_box(&input)));
    });
}

fn bench_append_latency(c: &mut Criterion) {
    let mut registry = ChannelRegistry::new();
    let signer = SigningKey::generate(&mut OsRng);
    registry.upsert(ledger_spec::ChannelSpec {
        name: "bench".into(),
        policy: ledger_spec::ChannelPolicy {
            min_signers: 1,
            allowed_signers: vec![signer.verifying_key().to_bytes()],
            require_attestations: false,
            enforce_timestamp_ordering: true,
        },
    });
    let mut env = Envelope {
        header: EnvelopeHeader {
            channel: "bench".into(),
            version: 1,
            prev: None,
            body_hash: [0u8; 32],
            timestamp: 1,
        },
        body: EnvelopeBody {
            payload: serde_json::json!({"n": 1}),
            payload_type: Some("bench".into()),
        },
        signatures: Vec::new(),
        attestations: Vec::new(),
    };
    env.header.body_hash = ledger_spec::hash_body(&env.body);
    signing::sign_envelope(&mut env, &signer);
    let log = AppendLog::new();

    c.bench_function("append_log_with_validation", |b| {
        b.iter(|| {
            let mut to_append = env.clone();
            to_append.header.timestamp += 1;
            log.append(black_box(to_append), black_box(&registry))
                .expect("append succeeds");
        });
    });
}

fn bench_receipt_generation(c: &mut Criterion) {
    let mut registry = ChannelRegistry::new();
    let signer = SigningKey::generate(&mut OsRng);
    registry.upsert(ledger_spec::ChannelSpec {
        name: "bench".into(),
        policy: ledger_spec::ChannelPolicy {
            min_signers: 1,
            allowed_signers: vec![signer.verifying_key().to_bytes()],
            require_attestations: false,
            enforce_timestamp_ordering: true,
        },
    });
    let log = AppendLog::new();
    for ts in 0..32u64 {
        let mut env = Envelope {
            header: EnvelopeHeader {
                channel: "bench".into(),
                version: 1,
                prev: None,
                body_hash: [0u8; 32],
                timestamp: ts,
            },
            body: EnvelopeBody {
                payload: serde_json::json!({"ts": ts}),
                payload_type: Some("bench".into()),
            },
            signatures: Vec::new(),
            attestations: Vec::new(),
        };
        env.header.body_hash = ledger_spec::hash_body(&env.body);
        signing::sign_envelope(&mut env, &signer);
        log.append(env, &registry).expect("append");
    }

    c.bench_function("merkle_receipt_generation", |b| {
        b.iter(|| {
            let receipt = log.receipt_for(black_box(16)).expect("receipt");
            black_box(receipt.verify());
        });
    });
}

fn bench_transport_loopback_latency(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let mut registry = ChannelRegistry::new();
    let signer = SigningKey::generate(&mut OsRng);
    registry.upsert(ledger_spec::ChannelSpec {
        name: "bench".into(),
        policy: ledger_spec::ChannelPolicy {
            min_signers: 1,
            allowed_signers: vec![signer.verifying_key().to_bytes()],
            require_attestations: false,
            enforce_timestamp_ordering: true,
        },
    });
    let transport = Loopback::new(registry.clone(), None).expect("loopback");

    c.bench_function("loopback_append_read_latency", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut env = Envelope {
                    header: EnvelopeHeader {
                        channel: "bench".into(),
                        version: 1,
                        prev: None,
                        body_hash: [0u8; 32],
                        timestamp: 1,
                    },
                    body: EnvelopeBody {
                        payload: serde_json::json!({"ts": 1}),
                        payload_type: Some("bench".into()),
                    },
                    signatures: Vec::new(),
                    attestations: Vec::new(),
                };
                env.header.body_hash = ledger_spec::hash_body(&env.body);
                signing::sign_envelope(&mut env, &signer);
                transport.append(env.clone()).await.expect("append");
                let _ = transport.read(0, 1).await.expect("read");
            })
        });
    });
}

criterion_group!(
    benches,
    bench_generate_update,
    bench_verify_update,
    bench_square_mod_n,
    bench_append_latency,
    bench_receipt_generation,
    bench_transport_loopback_latency,
);
criterion_main!(benches);
