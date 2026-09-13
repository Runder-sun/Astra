#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_BIN="${CARGO_BIN:-cargo}"
MANIFEST_PATH="${MOCK_PARITY_MANIFEST:-$ROOT_DIR/tests/mock_provider/last_run_manifest.json}"

cd "$ROOT_DIR"

echo "[mock-parity] running Batch 8 provider/auth/session parity checks"
scenarios=(
  "provider_config|prompt_uses_config_and_env_precedence_for_default_model_then_cli_overrides"
  "provider_live|providers_test_reports_live_lane_readiness_machine_readably"
  "provider_auth|providers_test_missing_credentials_returns_blocked_typed_failure"
  "provider_config|config_effective_applies_environment_overrides_machine_readably"
  "provider_config|config_get_reports_environment_override_source_machine_readably"
  "provider_config|config_sources_reports_cross_gap_override_without_middle_layer"
  "provider_catalog|providers_refresh_catalog_file_source_reports_not_graduated_typed_failure"
  "session_routing|prompt_with_session_appends_to_existing_session_transcript"
  "session_routing|resume_returns_selected_session_with_recap_and_lineage"
  "session_routing|sessions_create_list_and_resume_latest_are_project_scoped"
)

for scenario in "${scenarios[@]}"; do
  IFS="|" read -r category test_name <<<"$scenario"
  "$CARGO_BIN" test --test conformance "$test_name" -- --nocapture
done

mkdir -p "$(dirname "$MANIFEST_PATH")"
{
  echo "{"
  echo '  "harness": "mock_provider_parity",'
  echo '  "status": "passed",'
  echo '  "scenarios": ['
  for index in "${!scenarios[@]}"; do
    IFS="|" read -r category test_name <<<"${scenarios[$index]}"
    printf '    {"category":"%s","test_name":"%s","status":"passed"}' "$category" "$test_name"
    if [[ "$index" -lt "$((${#scenarios[@]} - 1))" ]]; then
      echo ","
    else
      echo
    fi
  done
  echo "  ]"
  echo "}"
} > "$MANIFEST_PATH"

echo "[mock-parity] parity checks passed"
echo "[mock-parity] manifest written to $MANIFEST_PATH"
