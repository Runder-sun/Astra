#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CARGO_BIN="${CARGO_BIN:-cargo}"

cd "$ROOT_DIR"

echo "[golden] running curated golden fixtures"
"$CARGO_BIN" test --test conformance golden_ -- --nocapture

echo "[golden] golden fixtures passed"
