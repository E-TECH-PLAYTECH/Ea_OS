use assert_cmd::cargo::cargo_bin;
use std::fs::File;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

use ledger_spec::{ChannelPolicy, ChannelSpec, Envelope, EnvelopeBody, EnvelopeHeader};

#[test]
fn daemon_and_cli_share_log_with_registry_enforcement() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempdir()?;
    let socket_path = temp.path().join("ledger.sock");
    let registry_path = temp.path().join("registry.json");
    let mismatched_registry_path = temp.path().join("registry_mismatch.json");
    let envelope_path = temp.path().join("env.json");

    // Registry with a single channel that uses the default policy.
    let registry = vec![ChannelSpec {
        name: "ipc_demo".into(),
        policy: ChannelPolicy::default(),
    }];
    let mut reg_file = File::create(&registry_path)?;
    reg_file.write_all(serde_json::to_string(&registry)?.as_bytes())?;

    // Registry that omits the channel to trigger client-side validation failure.
    let mismatched_registry: Vec<ChannelSpec> = Vec::new();
    let mut mismatch_file = File::create(&mismatched_registry_path)?;
    mismatch_file.write_all(serde_json::to_string(&mismatched_registry)?.as_bytes())?;

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

    // Start daemon bound to the Unix socket.
    // Use a random port for metrics/status to avoid conflicts with other tests.
    let status_port = portpicker::pick_unused_port().unwrap_or(19091);
    let mut daemon = Command::new(cargo_bin("ledgerd"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .arg("--transport")
        .arg("unix")
        .arg("--unix-path")
        .arg(&socket_path)
        .arg("--status-addr")
        .arg(format!("127.0.0.1:{}", status_port))
        .arg("--registry")
        .arg(&registry_path)
        .arg("daemon")
        .arg("--checkpoint")
        .arg("2")
        .spawn()?;

    // Give the daemon a moment to bind the socket.
    // Wait up to 2 seconds for the socket to be created.
    for _ in 0..20 {
        if socket_path.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    // Extra time for daemon to be fully ready after socket creation.
    thread::sleep(Duration::from_millis(200));

    // Append via CLI using the shared registry.
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

    // Attempt to append with a registry that lacks the channel; should fail client-side.
    assert_cmd::Command::cargo_bin("ledgerd")?
        .arg("--transport")
        .arg("unix")
        .arg("--unix-path")
        .arg(&socket_path)
        .arg("--registry")
        .arg(&mismatched_registry_path)
        .arg("append")
        .arg("--file")
        .arg(&envelope_path)
        .assert()
        .failure();

    // Read back the entry and confirm only the valid append is visible.
    let output = assert_cmd::Command::cargo_bin("ledgerd")?
        .arg("--transport")
        .arg("unix")
        .arg("--unix-path")
        .arg(&socket_path)
        .arg("--registry")
        .arg(&registry_path)
        .arg("read")
        .arg("--offset")
        .arg("0")
        .arg("--limit")
        .arg("2")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&output);
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "expected single log entry after enforcing registry match"
    );
    assert!(
        stdout.contains("channel=ipc_demo") && stdout.contains("\"hello\":\"world\""),
        "unexpected read output: {stdout}"
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
    Ok(())
}
