use assert_cmd::cargo::cargo_bin;
use std::fs::File;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

use ledger_spec::{ChannelPolicy, ChannelSpec, Envelope, EnvelopeBody, EnvelopeHeader};

#[test]
fn env_configured_daemon_serves_shared_log() -> Result<(), Box<dyn std::error::Error>> {
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

    // Start daemon bound to the Unix socket using environment-driven config.
    let mut daemon = Command::new(cargo_bin("ledgerd"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("LEDGER_TRANSPORT", "unix")
        .env("LEDGER_UNIX_PATH", &socket_path)
        .env("LEDGER_REGISTRY", &registry_path)
        .arg("daemon")
        .arg("--checkpoint")
        .arg("2")
        .spawn()?;

    // Give the daemon a moment to bind the socket.
    thread::sleep(Duration::from_millis(500));

    // Append via CLI (same transport + registry via env).
    assert_cmd::Command::cargo_bin("ledgerd")?
        .env("LEDGER_TRANSPORT", "unix")
        .env("LEDGER_UNIX_PATH", &socket_path)
        .env("LEDGER_REGISTRY", &registry_path)
        .arg("append")
        .arg("--file")
        .arg(&envelope_path)
        .assert()
        .success();

    // Read back the entry and confirm the payload shows up.
    let output = assert_cmd::Command::cargo_bin("ledgerd")?
        .env("LEDGER_TRANSPORT", "unix")
        .env("LEDGER_UNIX_PATH", &socket_path)
        .env("LEDGER_REGISTRY", &registry_path)
        .arg("read")
        .arg("--offset")
        .arg("0")
        .arg("--limit")
        .arg("1")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&output);
    assert!(
        stdout.contains("channel=ipc_demo") && stdout.contains("\"hello\":\"world\""),
        "unexpected read output: {stdout}"
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
    Ok(())
}
