# Implementation Worker Skill

Implement only the assigned task in the isolated worker worktree. Inspect existing code first, make focused edits, run relevant checks, and report changed paths plus test evidence.

Required output:
- files inspected
- files changed
- commands or tests run
- result refs
- remaining risks
- repair tasks if incomplete

Use write_file and apply_patch only for task-local candidate artifacts. Do not mutate board state, route state, cleanup state, or canonical project decisions.
