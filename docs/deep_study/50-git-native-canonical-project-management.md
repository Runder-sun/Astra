# Git-Native Canonical Project Management

This document tightens the M3/M4 project-management definition: the project may
expose exactly one latest, unified, non-ambiguous public surface for docs and
code. History remains traceable, but stale generations must not look active.

## 1. Principle

Git is a first-class evidence source for project management, but it is not a
second runtime authority.

The authority split is:

- git records versioned working-tree and branch state
- `.pmcli/` records runtime/project protocol state
- schemas define machine-readable contracts
- ProjectOps enforces cleanup, review, and promotion gates

A feature worktree can be a candidate. It is not the canonical public project
surface until it passes canonicality audit and is promoted or merged.

## 2. Canonical Surface Requirement

A project may expose exactly one active canonical surface for each public role:

- status document
- architecture document
- implementation plan
- proof packet
- active runtime/code ownership map
- active schema ownership map

Superseded documents and code paths may remain only as archived artifacts or
explicitly superseded records. They must not be included in default context,
README claims, proof packets, promotion targets, or remote/project dashboards.

## 3. Required Runtime Objects

### GitStateSnapshot

`GitStateSnapshot` is a read-only observation of the workspace:

- whether the workspace is a real git repository
- branch, HEAD, base branch, and merge base when available
- dirty, staged, and untracked paths
- known worktrees
- ahead/behind when available

It is evidence for project state, not runtime authority.

### CanonicalSurfaceManifest

`CanonicalSurfaceManifest` declares the one public project surface:

- active docs
- active code roots
- active schema roots
- archive roots
- external surface files
- forbidden public patterns
- superseded paths

If no explicit manifest exists, the runtime may produce a derived default, but a
release/promotion gate should require an explicit manifest.

### CodeOwnerManifest

`CodeOwnerManifest` maps active implementation responsibility:

- runtime root
- command registry
- tool registry
- schema registry
- provider/session/project layers
- legacy/superseded paths
- forbidden duplicate modules

It prevents two implementations from both looking active.

### CanonicalityAuditReport

`CanonicalityAuditReport` is the merge/promotion gate payload. It reports:

- duplicate active docs
- stale docs
- ambiguous code owners
- untracked public files
- orphan generated files
- multiple latest candidates
- stale README claims
- cleanup proposals
- blocking violations

## 4. M3/M4 Gate

`research-cli projects audit --canonical --json` is the first read-only gate. It
must not mutate project files. Later ProjectOps lanes may use the audit output to
create reviewable cleanup plans.

No branch may be promoted or merged while `CanonicalityAuditReport.ok == false`
for blocking violations.

Minimum blocking violations:

- more than one active document for a canonical public role
- untracked public source/schema/doc files in a real git repository
- README or proof packet contradicts the canonical manifest
- active code owner roots overlap or point at superseded paths
- generated schema/source files are public but not tracked or ignored

## 5. Relationship To Artifact Families

Artifact families manage lineage: latest, archive, supersession, review links.
Canonical surface management decides what is public and active now.

The two must work together:

- a canonical doc/code path may point to an artifact family latest
- archive paths remain inspectable but non-authoritative
- cleanup plans move stale public files into archive families or mark them
  superseded
- promotion updates the canonical surface only after review/evaluation gates

## 6. Completion Criteria

This slice is complete when:

- schemas exist for git state, canonical surface, code ownership, and audit report
- `projects audit --canonical --json` emits a schema-valid report
- duplicate active docs become blocking violations
- untracked public files in a real git repo become blocking violations
- M3 cleanup and branch promotion can consume the audit report as a hard gate
