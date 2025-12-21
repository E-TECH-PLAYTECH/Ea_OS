use assert_cmd::cargo::CommandCargoExt;
use std::fs::File;
use std::io::Write;
use std::process::Stdio;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

use ledger_spec::{ChannelPolicy, ChannelSpec, Envelope, EnvelopeBody, EnvelopeHeader};

fn available_status_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map(|addr| addr.port())
        .unwrap_or(9099)
}

#[test]
fn metrics_and_health_endpoints_respond() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempdir()?;
    let socket_path = temp.path().join("ledger.sock");
    let registry_path = temp.path().join("registry.json");
    let envelope_path = temp.path().join("env.json");

    // Registry with a single channel that uses the default policy.
    let registry = vec![ChannelSpec {
        name: "ipc_demo".into(),
        policy: ChannelPolicy::default(),
    }];
    let mut reg_file = File::create(&registry_path)?;
    reg_file.write_all(serde_json::to_string(&registry)?.as_bytes())?;

    // Seed envelope that will be signed by the CLI when appended.
    let env = Envelope {
        header: EnvelopeHeader {
            channel: "ipc_demo".into(),
            version: 1,
            prev: None,
            body_hash: ledger_spec::hash_body(&EnvelopeBody {
                payload: serde_json::json!({"hello": "world"}),
                payload_type: Some("test".into()),
            }),
            timestamp: 1,
        },
        body: EnvelopeBody {
            payload: serde_json::json!({"hello": "world"}),
            payload_type: Some("test".into()),
        },
        signatures: Vec::new(),
        attestations: Vec::new(),
    };
    let mut env_file = File::create(&envelope_path)?;
    serde_json::to_writer(&mut env_file, &env)?;

    let status_port = available_status_port();

    // Start daemon bound to the Unix socket.
    let mut daemon = assert_cmd::Command::cargo_bin("ledgerd")?
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .arg("--transport")
        .arg("unix")
        .arg("--unix-path")
        .arg(&socket_path)
        .arg("--registry")
        .arg(&registry_path)
        .arg("--status-addr")
        .arg(format!("127.0.0.1:{status_port}"))
        .arg("daemon")
        .arg("--checkpoint")
        .arg("2")
        .spawn()?;

    // Give the daemon a moment to bind the socket and status endpoint.
    thread::sleep(Duration::from_millis(750));

    // Append via CLI.
    assert_cmd::Command::cargo_bin("ledgerd")?
        .arg("--transport")
        .arg("unix")
        .arg("--unix-path")
        .arg(&socket_path)
        .arg("--registry")
        .arg(&registry_path)
        .arg("append")
        .arg("--file")
        .arg(&envelope_path)
        .assert()
        .success();

    // Poll metrics endpoint.
    let metrics_body =
        reqwest::blocking::get(format!("http://127.0.0.1:{status_port}/metrics"))?.text()?;
    assert!(
        metrics_body.contains("ledgerd_appends_total"),
        "metrics body missing expected counter"
    );

    // Poll health endpoint.
    let health: serde_json::Value =
        reqwest::blocking::get(format!("http://127.0.0.1:{status_port}/healthz"))?.json()?;
    assert_eq!(health["status"], "ok");
    assert!(
        health["log_length"].as_u64().unwrap_or(0) >= 1,
        "expected log length to be at least 1"
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
    Ok(())
}
