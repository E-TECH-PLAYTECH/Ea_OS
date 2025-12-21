# ledgerd operational runbook

This runbook summarizes day-2 operations for `ledgerd` with the new telemetry endpoints.

## Configuration

- **Registry**: `--registry PATH` or `LEDGER_REGISTRY` points to a JSON array of `ChannelSpec` entries.
- **Transport**: `--transport`, `--unix-path`, and `--quic-endpoint` or the corresponding environment variables (`LEDGER_TRANSPORT`, `LEDGER_UNIX_PATH`, `LEDGER_QUIC_ENDPOINT`).
- **Logging**: use `--log-level` (or `LEDGER_LOG_LEVEL`) to override verbosity; `-v/--verbose` still works for quick toggles.
- **Metrics/health server**: bind address via `--status-addr` or `LEDGER_STATUS_ADDR` (default `127.0.0.1:9090`). The server exposes:
  - `/metrics`: Prometheus format
  - `/healthz`: liveness-style JSON
  - `/readyz`: readiness-style JSON

## Observability fields

- Tracing spans on append/read include `channel`, `offset`, `latency_ms`, and in the daemon loop the receive `backlog`.
- Prometheus metrics:
  - `ledgerd_appends_total{channel}`
  - `ledgerd_append_errors_total{channel}`
  - `ledgerd_append_latency_ms_bucket|sum|count{channel}`
  - `ledgerd_read_latency_ms_bucket|sum|count{channel}`
  - `ledgerd_backlog`
  - `ledgerd_disk_usage_bytes`
  - `ledgerd_attestation_configured` (1 if configured)

## Health and readiness

- `/healthz` and `/readyz` return:
  - `status`: `"ok"` or `"ready"` label
  - `backlog`: current receive buffer depth
  - `log_length`: current append log length
  - `disk_usage_bytes`: storage estimation (0 for in-memory)
  - `attestation_configured`: boolean derived from transport configuration

## Smoke checks

1. Start the daemon:
   ```bash
   ledgerd --transport unix --unix-path /tmp/ledgerd.sock \
     --registry /path/registry.json \
     --status-addr 127.0.0.1:9090 daemon --checkpoint 5
   ```
2. Append a test envelope:
   ```bash
   ledgerd --transport unix --unix-path /tmp/ledgerd.sock \
     --registry /path/registry.json append --file env.json
   ```
3. Verify endpoints:
   ```bash
   curl -sf http://127.0.0.1:9090/healthz
   curl -sf http://127.0.0.1:9090/readyz
   curl -sf http://127.0.0.1:9090/metrics | head
   ```
