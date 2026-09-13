#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_BIN="${CARGO_BIN:-cargo}"
MANIFEST_PATH="${PARITY_DEMO_MANIFEST:-$ROOT_DIR/tests/parity_demo/last_run_manifest.json}"

cd "$ROOT_DIR"

echo "[parity-demo] running Claude/Hermes parity demo suite"

scenarios=(
  "runtime_turn_demo|runtime|claude_code_agent_loop|test --test conformance remote_daemon_interrupt_cancels_active_remote_prompt_turn_directly -- --nocapture;test --test conformance remote_daemon_executes_projected_tui_action_ids -- --nocapture;test --test conformance transcript_round_trip_preserves_tool_turn_structure -- --nocapture|remote_daemon_interrupt_cancels_active_remote_prompt_turn_directly,remote_daemon_executes_projected_tui_action_ids,transcript_round_trip_preserves_tool_turn_structure|active_turn_registry,direct_runtime_cancellation,cancelled_turn_outcome,notification_fallback_contract,tool_turn_replay"
  "tui_inline_repl_demo|tui|claw_code_inline_repl|test --lib tui::tests:: -- --nocapture|tui::tests::|inline_repl_default,registry_backed_palette,live_permissions,turn_interrupt,structured_output,narrow_terminal_fit"
  "remote_mobile_handoff_demo|mobile|claude_mobile_continuity|test --test conformance remote_mobile_web_app_assets_expose_theme_language_and_session_controls -- --nocapture;test --test conformance remote_mobile_runtime_bootstrap_probes_before_requiring_control_token -- --nocapture;test --test conformance remote_daemon_reconnect_replays_cursor_and_rejects_expired_lease -- --nocapture;test --test conformance remote_daemon_terminal_bridge_records_governed_input_resize_and_signal -- --nocapture;test --test conformance remote_daemon_exposes_workbench_message_and_permission_response -- --nocapture;test --test conformance remote_daemon_interrupt_cancels_active_remote_prompt_turn_directly -- --nocapture|remote_mobile_web_app_assets_expose_theme_language_and_session_controls,remote_mobile_runtime_bootstrap_probes_before_requiring_control_token,remote_daemon_reconnect_replays_cursor_and_rejects_expired_lease,remote_daemon_terminal_bridge_records_governed_input_resize_and_signal,remote_daemon_exposes_workbench_message_and_permission_response,remote_daemon_interrupt_cancels_active_remote_prompt_turn_directly|shared_session_projection,readiness_probe_before_token,permission_response,cursor_reconnect,governed_terminal_bridge,direct_interrupt"
  "host_contract_demo|host|shared_host_surface_contract|test --test conformance host_surface_and_terminal_payloads_validate_against_m14_schemas -- --nocapture;test --test conformance remote_daemon_exposes_host_surface_and_governed_terminal_projection -- --nocapture;test --test conformance remote_daemon_events_streams_canonical_events_after_cursor -- --nocapture;test --test conformance remote_daemon_exposes_artifact_code_viewer_projection -- --nocapture;test --test conformance remote_daemon_exposes_result_panel_projection_without_new_truth -- --nocapture|host_surface_and_terminal_payloads_validate_against_m14_schemas,remote_daemon_exposes_host_surface_and_governed_terminal_projection,remote_daemon_events_streams_canonical_events_after_cursor,remote_daemon_exposes_artifact_code_viewer_projection,remote_daemon_exposes_result_panel_projection_without_new_truth|schema_valid_host_surface,shared_action_ids,direct_and_fallback_contracts,cursor_event_log,projection_backed_artifacts"
  "routine_background_demo|automation|hermes_routines|test --test conformance operator_cli_surfaces::routines_create_trigger_and_project_background_agent -- --nocapture|operator_cli_surfaces::routines_create_trigger_and_project_background_agent|routine_command_family,routine_definition,routine_trigger,agent_runtime_record,projectops_supervisor_lease"
  "goal_research_loop_demo|automation|hermes_research_loop|test --test conformance operator_cli_surfaces::goal_research_loop_demo_closes_success_and_repair_paths -- --nocapture|operator_cli_surfaces::goal_research_loop_demo_closes_success_and_repair_paths|goal_watch_install,watch_tick,goal_task_dispatch,worker_acceptance,stage_decision,repair_review,followup_dispatch,research_board_projection,event_log"
)

json_array_from_csv() {
  local csv="$1"
  local first=1
  printf '['
  IFS=',' read -ra values <<<"$csv"
  for value in "${values[@]}"; do
    if [[ "$first" -eq 0 ]]; then
      printf ', '
    fi
    first=0
    printf '"%s"' "$value"
  done
  printf ']'
}

json_array_from_args() {
  local first=1
  printf '['
  for value in "$@"; do
    if [[ "$first" -eq 0 ]]; then
      printf ', '
    fi
    first=0
    printf '"%s"' "$value"
  done
  printf ']'
}

run_cargo_commands() {
  local commands="$1"
  local failed=0
  IFS=';' read -ra command_list <<<"$commands"
  for cargo_args in "${command_list[@]}"; do
    echo "[parity-demo] cargo $cargo_args"
    read -ra cargo_argv <<<"$cargo_args"
    if ! "$CARGO_BIN" "${cargo_argv[@]}"; then
      failed=1
    fi
  done
  return "$failed"
}

scenario_statuses=()
passed_count=0
failed_count=0
gap_count=0

for scenario in "${scenarios[@]}"; do
  IFS="|" read -r demo_id category reference cargo_args test_name evidence_csv <<<"$scenario"
  echo "[parity-demo] $demo_id"
  scenario_status="passed"
  if ! run_cargo_commands "$cargo_args"; then
    scenario_status="failed"
  fi
  scenario_statuses+=("$scenario_status")
  if [[ "$scenario_status" == "passed" ]]; then
    passed_count=$((passed_count + 1))
  else
    failed_count=$((failed_count + 1))
  fi
done

overall_status="passed"
if [[ "$failed_count" -gt 0 ]]; then
  overall_status="failed"
elif [[ "$gap_count" -gt 0 ]]; then
  overall_status="partial"
fi

mkdir -p "$(dirname "$MANIFEST_PATH")"
{
  echo "{"
  echo '  "harness": "parity_demo_suite",'
  echo '  "schema_version": "1",'
  printf '  "generated_at": "%s",\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf '  "status": "%s",\n' "$overall_status"
  printf '  "summary": { "passed": %d, "failed": %d, "gap": %d },\n' "$passed_count" "$failed_count" "$gap_count"
  echo '  "demos": ['
  for index in "${!scenarios[@]}"; do
    IFS="|" read -r demo_id category reference cargo_args test_name evidence_csv <<<"${scenarios[$index]}"
    echo '    {'
    printf '      "demo_id": "%s",\n' "$demo_id"
    printf '      "category": "%s",\n' "$category"
    printf '      "reference": "%s",\n' "$reference"
    printf '      "status": "%s",\n' "${scenario_statuses[$index]}"
    echo '      "required_for_parity": true,'
    printf '      "test_names": '
    json_array_from_csv "$test_name"
    echo ','
    printf '      "evidence": '
    json_array_from_csv "$evidence_csv"
    echo ','
    echo '      "gap": null'
    if [[ "$index" == "$((${#scenarios[@]} - 1))" ]]; then
      echo '    }'
    else
      echo '    },'
    fi
  done
  echo '  ]'
  echo "}"
} > "$MANIFEST_PATH"

echo "[parity-demo] parity demo suite status: $overall_status"
echo "[parity-demo] manifest written to $MANIFEST_PATH"

if [[ "$failed_count" -gt 0 ]]; then
  exit 1
fi
