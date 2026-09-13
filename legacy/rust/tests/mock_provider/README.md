# Mock Provider Parity

This directory defines the deterministic Batch 8 mock-provider baseline.

Current scope:

- provider/config precedence is exercised through conformance tests
- provider live readiness is exercised through a local plain-http probe
- blocked credential lanes are exercised without contacting a vendor

The parity harness entrypoint is:

- `scripts/run_mock_parity_harness.sh`
- `cargo test --test conformance mock_provider_parity_harness_executes_selected_batch8_checks_sequentially`
- manifest artifact: `tests/mock_provider/last_run_manifest.json`

The harness intentionally avoids live vendor dependency and runs the frozen
Batch 8 provider/auth/session routing checks from the Rust conformance suite.
