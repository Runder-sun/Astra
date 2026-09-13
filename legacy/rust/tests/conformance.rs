#[path = "support/mod.rs"]
mod support;

#[path = "golden/config/config_precedence.rs"]
mod golden_config_precedence;
#[path = "golden/provider/accounting_summary.rs"]
mod golden_provider_accounting_summary;
#[path = "golden/provider/provider_resolution.rs"]
mod golden_provider_resolution;
#[path = "mock_provider/parity_harness.rs"]
mod mock_provider_parity_harness;
#[path = "operator/cli_surfaces.rs"]
mod operator_cli_surfaces;
#[path = "conformance/packaging.rs"]
mod packaging;
#[path = "conformance/parity_demo.rs"]
mod parity_demo;
#[path = "conformance/remote_daemon.rs"]
mod remote_daemon;
#[path = "conformance/runtime/bootstrap.rs"]
mod runtime_bootstrap;
#[path = "conformance/runtime/checkpoint_reducer.rs"]
mod runtime_checkpoint_reducer;
#[path = "conformance/runtime/context_pack.rs"]
mod runtime_context_pack;
#[path = "conformance/runtime/event_log.rs"]
mod runtime_event_log;
#[path = "conformance/runtime/kernel_bundle.rs"]
mod runtime_kernel_bundle;
#[path = "conformance/runtime/orchestration.rs"]
mod runtime_orchestration;
#[path = "conformance/runtime/permission_machine.rs"]
mod runtime_permission_machine;
#[path = "conformance/runtime/run_command.rs"]
mod runtime_run_command;
#[path = "conformance/runtime/schema_registry.rs"]
mod runtime_schema_registry;
#[path = "conformance/runtime/session_store.rs"]
mod runtime_session_store;
#[path = "conformance/runtime/working_memory.rs"]
mod runtime_working_memory;
#[path = "conformance/runtime/workspace_resolution.rs"]
mod runtime_workspace_resolution;
#[path = "conformance/schemas/batch10_enforcement.rs"]
mod schema_batch10_enforcement;
