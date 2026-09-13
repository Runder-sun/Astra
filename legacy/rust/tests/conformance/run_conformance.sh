#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CARGO_BIN="${CARGO_BIN:-cargo}"

cd "$ROOT_DIR"

echo "[conformance] running full Rust conformance suite"
"$CARGO_BIN" test --test conformance -- --nocapture

echo "[conformance] running Batch 8 mock parity harness"
bash scripts/run_mock_parity_harness.sh

echo "[conformance] all conformance checks passed"
