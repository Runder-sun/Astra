# Dr. Claw Productization Review

This document studies `reference_repos/requested/dr-claw` as a productization
reference, not as a kernel/runtime authority reference.

That distinction matters.

`research-cli` already has its kernel truth, event vocabulary, remote contract,
memory policy, and implementation sequence frozen in `11` through `34`.

So the right question is no longer "should we become like Dr. Claw?"

The right question is:

- which product ideas from Dr. Claw make our system more operable
- which patterns would weaken our single-runtime-truth architecture
- how to absorb the useful parts without regressing into an app-layer aggregator

## 1. What Dr. Claw Actually Is

From the inspected code and docs, Dr. Claw is best understood as a
full-surface research workspace:

- browser-first UI with desktop packaging and terminal entrypoints
- multi-backend execution adapters for external agent systems
- project discovery/indexing over provider-native session stores
- stage-oriented research workspace bootstrapping
- skill-library productization for large research workflow catalogs

Grounding files:

- `reference_repos/requested/dr-claw/README.md`
- `reference_repos/requested/dr-claw/server/projects.js`
- `reference_repos/requested/dr-claw/server/routes/projects.js`
- `reference_repos/requested/dr-claw/server/openai-codex.js`
- `reference_repos/requested/dr-claw/server/utils/sessionIndex.js`
- `reference_repos/requested/dr-claw/docs/pipeline-outputs.md`
- `reference_repos/requested/dr-claw/docs/skills-taxonomy-v2.md`

This is useful because it shows how a research-agent product becomes usable by
humans across surfaces.

It is not useful as a direct runtime blueprint because much of its authority is
assembled at the app/server layer.

## 2. Architecture Shape We Should Learn From

## 2.1 Surface multiplicity is a product strength

Dr. Claw ships the same overall product idea across:

- web
- desktop/Electron
- terminal CLI

This validates our choice in `19-remote-host-control-plane.md` and
`20-happy-happier-integration-blueprint.md` to treat remote/mobile/web control
as a first-class operator plane rather than an afterthought.

Absorption rule:

- `research-cli` remains terminal-native at the kernel
- remote/mobile/web become projections and control planes over that kernel
- we do not create a second browser-owned runtime just because Dr. Claw has a
  browser app

## 2.2 Adapter breadth matters

`server/openai-codex.js` shows a practical adapter approach: wrap provider
specific behavior and normalize messages/events into one app-facing stream.

This reinforces our existing direction:

- provider diversity is a real user need
- backend adapters must be explicit modules
- provider-specific event mess should be normalized before it reaches operator
  surfaces

But we absorb it differently:

- in Dr. Claw the normalization is largely for UI/session plumbing
- in `research-cli` normalization must terminate in canonical
  `KernelEventEnvelope` events and `KernelStateBundle` checkpoints

## 2.3 Project discovery/import is valuable

`server/projects.js` is one of the most instructive files in the repo.

It does something practical that many agent products ignore:

- scan external agent stores
- recover project identity from provider-specific storage
- unify those discoveries in one project layer

That is worth borrowing as an interoperability feature.

But it must remain import-only.

Our rule:

- external stores may be scanned, indexed, and imported
- external sessions may be referenced and linked
- external provider state must never outrank `.pmcli/` truth

This is the exact place where Dr. Claw is a good product reference but a bad
kernel authority reference.

## 2.4 Workspace-safety UX is strong

`server/routes/projects.js` implements path validation, forbidden paths, and
workspace-root restrictions.

This is directly compatible with our design and should be carried over into the
foundation phase because excellent code-agent CLI behavior starts with safe and
predictable workspace semantics.

Adopt directly:

- forbidden path lists
- resolved-path validation
- symlink escape protection
- configured workspace root policy
- machine-explainable workspace rejection errors

These should land as kernel/operator safety, not as purely web-app validation.

## 2.5 Stage-oriented artifact layout is productively concrete

`docs/pipeline-outputs.md` gives a clean user-facing research structure:

- `Survey`
- `Ideation`
- `Experiment`
- `Publication`
- `Promotion`

This is not sufficient as a runtime model by itself, but it is very useful as a
human-facing artifact layout.

We should absorb:

- the idea that research outputs need stable top-level families
- stage-visible directories that make progress legible to users
- stage-aware default destinations for reports, ideas, experiments, papers, and
  promotion assets

We should not absorb:

- stage folders as the only execution truth
- stage tags as a substitute for execution contracts

Our corresponding authorities remain:

- `15-research-review-and-contracts.md`
- `18-proactive-project-ops.md`
- `26-memory-branch-research-runtime-policy.md`
- `33-advanced-systems-implementation-plan.md`

## 2.6 Skill taxonomy productization is genuinely useful

`docs/skills-taxonomy-v2.md` is a high-signal artifact.

Its strongest idea is not the exact enum values.

Its strongest idea is the separation of concerns:

- user intent
- technical capability
- domain
- governance/provenance

That aligns very well with our requirement for a native research skill pack
which must be:

- operable
- searchable
- governable
- composable into workflows

We should absorb the taxonomy principle almost directly.

## 2.7 Session stage tagging helps operators

`server/utils/sessionIndex.js` shows practical stage-tagging of sessions:

- `survey`
- `ideation`
- `experiment`
- `publication`
- `promotion`

This is worth keeping as an operator convenience layer.

But it must remain a projection of stronger truth:

- tags are for browsing, filtering, and dashboarding
- `StageExecutionMap` and runtime packets remain the real execution truth

## 3. What We Must Not Copy

## 3.1 Do not let the app layer become runtime authority

The main thing to reject is not any single function.

It is the overall tendency for a rich app/server layer to become the place
where state is normalized, interpreted, and effectively owned.

`research-cli` must not drift into:

- browser-owned truth
- server-owned session truth
- provider-store-derived truth
- UI-state-derived progress truth

Our truth remains:

- `.pmcli/project_state.json` storing the serialized `KernelStateBundle`
- canonical event log and transcript artifacts
- schema-backed persisted runtime objects

## 3.2 Do not confuse interoperability with native execution

Dr. Claw is comfortable sitting on top of multiple existing agent systems.

That is useful for import, orchestration, and product reach.

But our user requirement is stricter:

- this must itself be an excellent code agent CLI
- multi-agent, memory, and research features are built on top of that native
  excellence

So we reject any architecture where:

- our core coding ability depends on external app wrappers
- our main session model is borrowed from Claude/Codex/Cursor storage
- our provider integrations become the de facto runtime instead of adapters

## 3.3 Do not let stage folders replace governed artifact families

Stage folders are useful and legible.

They are not enough to solve:

- canonical/latest/archive pointers
- supersession
- branch winner promotion
- review lineage
- repo cleanup and de-duplication

So we keep our stronger model:

- stage directories for human navigation
- artifact families and promotion rules for machine governance

## 3.4 Do not collapse skill execution into a loose catalog

Dr. Claw's skill system is helpful for discovery and breadth.

But our requirement is stronger:

- skills must participate in native runtime contracts
- stage execution must be packetized and reviewable
- branch/debate/research loops must persist inspectable evidence

So the catalog is not enough by itself.

We need both:

- a productized skill library
- a contract-first execution runtime for those skills

## 4. Concrete Adoption Decisions For research-cli

## Decision A: add interop importers, not interop authority

After the core kernel is stable, add an interoperability layer that can:

- discover Claude/Codex/Cursor/Gemini style external sessions
- import or link them into a `research-cli` project
- annotate them as foreign-origin evidence

Suggested future module family:

```text
internal/interop/
  claude/
  codex/
  cursor/
  gemini/
```

These modules may emit imported artifacts and linked-session references.

They may not write alternate kernel truth.

## Decision B: promote workspace safety into M0-M2

The workspace validation quality seen in `server/routes/projects.js` is
important enough to pull earlier.

This should explicitly strengthen:

- `internal/workspace/resolve.go`
- `internal/permissions/policy.go`
- `tests/golden/sessions/project_scope_test.go`
- `tests/golden/permissions/permission_mode_test.go`

## Decision C: keep stage directories as user-facing research defaults

We should adopt the five-stage directory line as the default human-facing
research layout:

- `Survey/`
- `Ideation/`
- `Experiment/`
- `Publication/`
- `Promotion/`

But they must be generated through governed artifact-family rules and stage
execution contracts.

## Decision D: formalize a skill-manifest taxonomy model

The skill-manifest work in `33-advanced-systems-implementation-plan.md` should
explicitly inherit these taxonomy dimensions:

- `primary_intent`
- `intents`
- `capabilities`
- `domains`
- `keywords`
- `source`
- `status`
- `related_skills`

This will make the research skill pack substantially more operable than a loose
folder tree.

## Decision E: make stage tags a projection field everywhere

Session/stage tagging should exist in:

- session list views
- remote workbench views
- research dashboards
- branch/review filters

But always as projection metadata derived from stage/runtime evidence.

## Decision F: treat Dr. Claw as a product benchmark, not a kernel benchmark

This is the most important final decision.

Dr. Claw raises our product bar in:

- surface richness
- workflow packaging
- research artifact visibility
- skill discoverability
- interoperability ambition

But it does not replace the more important code-agent kernel references:

- Hermes
- claw-code
- OpenCode
- Crush class systems

Those remain the primary references for base CLI excellence.

Dr. Claw supplements them by showing how that excellence can be wrapped into a
research product.

## 5. Resulting Delta To Our Current Architecture

The good news is that Dr. Claw does not force a redesign.

Instead, it sharpens implementation priorities:

1. push workspace safety and path semantics hard in M0-M2
2. keep remote/mobile/web as projection over native kernel truth
3. make research stage directories visible and first-class for users
4. build a real skill-manifest taxonomy instead of a loose registry
5. plan an explicit late-phase interop/import layer for foreign session stores

So the architecture remains the same, but the productization bar rises.

That is exactly the role Dr. Claw should play in this study.

## 6. Final Verdict

Dr. Claw is a valuable reference project.

Its strongest lessons are:

- multi-surface packaging matters
- research outputs must be legible and staged
- skill libraries need information architecture, not just files
- external agent ecosystems can be indexed and imported usefully
- workspace safety should be treated as a product feature

Its main limitation for our purposes is equally clear:

- it is not a sufficiently strict single-runtime-truth kernel blueprint

Therefore:

- we borrow its productization ideas
- we reject it as runtime authority
- we use it to make `research-cli` more operable without weakening the kernel
  contracts already frozen in this repo
