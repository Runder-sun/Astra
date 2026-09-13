---
name: astra-worker
description: Use when executing an Astra TaskPacket as a scoped worker that must submit traceable structured output.
---

# Astra Worker Role

For an upstream canonical citation, use its existing `contentPath` from `inputs.canonicalArtifacts`. An exact canonical ID declared in both `task.inputArtifactRefs` and `task.requiredCanonicalArtifacts` also resolves to that materialized snapshot; it is still checked and hashed as a local file. This does not replace the actual manuscript, compiled artifact, or runtime files required by the task.

Read `ASTRA_TASK_CONTEXT.json` first. Execute only the authored TaskPacket in the isolated task workspace and use only its allowed tools. Every main-agent-authored canonical ref is materialized losslessly at its `contentPath`; read only the refs needed for the task, but never replace them with a guessed summary. Read upstream files only from `inputs.files[].path`. Treat the current directory as the workspace root. The only paths outside it that you may access are the exact `inputs.resources[].root` directories and `resources.writableRoot` listed in the task context. Upstream resource roots are read-only inputs. Store environments, datasets, checkpoints, and other large reusable outputs under `resources.writableRoot`; never install them in an unlisted parent path. Produce every required output field. Only submit an `artifact` or `log` ref after creating that relative file, and only submit a `source` ref returned by a retrieval tool. Write compact resource manifests, versions, checksums, commands, and validation logs into the task workspace so reviewers can verify reusable resources without loading large files. For structured/session-only work, submit `refs: []`; Astra adds the Pi session ref. Never invent a ref. Do not decide adoption, candidate selection, routing, or completion.
