---
name: astra-implement-solution
description: Use during Astra implementation stages to create a runnable minimal implementation and behavioral tests inside the isolated task workspace.
---

# Astra Implementation Stage

Implement the core method in the task workspace. Environment creation, dependency installation, simulator setup, dataset retrieval, policy/checkpoint download, and checksum verification are implementation work, not external blockers. Put these large reusable resources under `resources.writableRoot` and make setup idempotent. Reuse only the exact upstream roots listed in `inputs.resources`; never guess a host path. Setup may use the full TaskPacket runtime budget, but after setup run only focused behavioral smoke checks rather than the full experiment matrix. Preserve implementation files, a compact resource manifest, exact setup commands, versions, checksums, and smoke-test logs as local refs. Do not execute large-scale sweeps, crossover analysis, or research conclusions; those belong to later `run` and `result-to-claim` stages. Report real limitations and failed setup attempts instead of replacing execution evidence with prose or marking installable dependencies as externally blocked.

When a runtime error identifies a missing system executable, shared library, display service, or driver-side utility, inspect the exact error and install or configure that prerequisite from the worker when current permissions allow it. Record the command and retry the real smoke path. A message such as `install vulkan-tools` or `start Xorg` is an implementation action, not a reason to submit early. Stop only for a genuinely unavailable licensed asset, credential, hardware capability, or permission boundary after recording the attempted setup and exact failure.

The worker shell intentionally blocks global host mutation. A blocked `sudo` command is not a final setup blocker. Before reporting a permission boundary, attempt task-owned rootless provisioning: download or extract system packages under `$ASTRA_RESOURCE_ROOT/system`, install language packages and environments under `$ASTRA_RESOURCE_ROOT`, and invoke their executables or libraries with task-local `PATH` or `LD_LIBRARY_PATH` values. Record the exact rootless commands and retry the real runtime. Do not modify global package state or ask an external operator to prepare the environment.
