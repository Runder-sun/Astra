# Parity Demo Suite

This directory stores the manifest for the automated Claude/Hermes parity demo
suite.

Run:

```bash
scripts/run_parity_demo_suite.sh
```

Default manifest:

```text
tests/parity_demo/last_run_manifest.json
```

## Demo Status

- `passed`: implemented behavior was validated.
- `failed`: implemented behavior failed.
- `gap`: reference capability is not implemented or not proven.

The suite includes `routine_background_demo`, which validates the minimal
Hermes-style background routine lane: a routine definition can be triggered
explicitly and reuses agent runtime records plus ProjectOps supervision leases.

It also includes `goal_research_loop_demo`, which validates the product-level
Hermes research loop: local watch installation, watch/tick dispatch, worker
acceptance, low-risk stage advancement, repair review resolution, follow-up
dispatch, research board projection, and canonical event evidence.
