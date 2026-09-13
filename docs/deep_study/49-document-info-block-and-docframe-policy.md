---
doc_frame:
  id: docs.deep_study.49_document_info_block_and_docframe_policy
  title: Document Info Block And DocFrame Policy
  doc_type: policy
  lifecycle: active
  scope: project
  milestone: M7-M10
  mission_refs:
    - mission_frame
  summary: Defines schema-owned compact document context blocks for docs, milestones, skill outputs, compaction, and MissionFrame refresh proposals.
  key_claims:
    - Important project documents should expose a compact parseable context block.
    - DocFrames may feed MissionFrame refresh proposals but cannot replace the canonical MissionFrame.
    - The document index is rebuildable context, not runtime authority.
  interfaces:
    - schemas/doc_frame.schema.json
    - schemas/doc_index.schema.json
    - .pmcli/docs/index.json
  evidence_refs:
    - docs/deep_study/48-mission-frame-context-and-compact-policy.md
    - docs/deep_study/28-schema-registry-and-conformance-plan.md
  next_actions:
    - Add DocFrame and DocIndex schemas.
    - Add docs index, docs inspect, and docs frame refresh commands.
  generated_by: agent
  updated_at: 2026-04-25
---

# Document Info Block And DocFrame Policy

This document formalizes the documentation-management idea that every important
project document should expose a compact, machine-readable information block.

The goal is to let agents, skills, compaction, milestone planning, and review
packets recover enough context from documents without rereading the full corpus
or trusting free-form summaries.

The canonical name for this block is `DocFrame`.

## 1. Design Position

`DocFrame` is a document-scoped context anchor. It is not a replacement for the
document body and not an ordinary generated summary.

It answers:

- what this document is about
- what kind of document it is
- which milestone, feature, or project goal it supports
- which decisions, interfaces, and evidence references it exposes
- what downstream agents may safely use it for
- whether the document is active, draft, superseded, or archived

The block must be short enough to inject, parse, index, and compare. The body of
the document remains the full authority for detailed reasoning.

Canonical schema targets:

```text
schemas/doc_frame.schema.json
schemas/doc_index.schema.json
```

Canonical derived index target:

```text
.pmcli/docs/index.json
```

## 2. Relationship To MissionFrame

`MissionFrame` and `DocFrame` solve adjacent problems.

`MissionFrame` is project-scoped and answers:

```text
project_max_goal > milestone_goal > current_implementation_goal
```

`DocFrame` is document-scoped and answers:

```text
document purpose > exposed claims > evidence refs > next actions
```

The runtime may derive candidate project, milestone, or implementation summaries
from a set of `DocFrame` objects, but it must not silently overwrite the
canonical `MissionFrame`.

Required governance:

- `DocFrame` may propose a `MissionFrame` refresh.
- `MissionFrame` remains the canonical project-goal state.
- any automatic refresh must emit evidence references to the contributing
  `DocFrame` objects.
- compaction may use `DocFrame` objects to rebuild context packets, but not to
  invent new goals.

This keeps the user's idea compatible with the current design. It adds a
document-index plane under the existing goal plane instead of competing with it.

## 3. Authoring Format

The preferred authoring format is YAML front matter at the top of Markdown
documents:

```yaml
---
doc_frame:
  id: docs.deep_study.49_document_info_block_and_docframe_policy
  title: Document Info Block And DocFrame Policy
  doc_type: policy
  lifecycle: active
  scope: project
  milestone: M7-M10
  mission_refs:
    - mission_frame
  summary: Compact schema-owned information block for project documents.
  key_claims:
    - Important docs expose a parseable compact context block.
    - DocFrames can feed MissionFrame refresh proposals without replacing it.
  interfaces:
    - schemas/doc_frame.schema.json
    - schemas/doc_index.schema.json
  evidence_refs:
    - docs/deep_study/48-mission-frame-context-and-compact-policy.md
  next_actions:
    - Add schema and doc-index extraction command.
  updated_at: 2026-04-25
---
```

Long-term tooling may also accept fenced JSON for generated artifacts that
cannot safely use Markdown front matter:

````text
```research-cli-doc-frame
{ "...": "..." }
```
````

Authoring tools should normalize both formats into the same schema-owned
`DocFrame` object.

## 4. Minimum Data Shape

Minimum object:

```text
DocFrame {
  doc_id
  schema_version
  source_path
  title
  doc_type
  lifecycle
  scope
  milestone
  summary
  key_claims[]
  decisions[]
  interfaces[]
  evidence_refs[]
  next_actions[]
  non_goals[]
  generated_by
  updated_at
}
```

Recommended enum values:

```text
doc_type = policy | plan | spec | milestone | review | handoff |
           skill_output | reference_audit | implementation_note

lifecycle = draft | active | superseded | archived

scope = project | milestone | feature | session | artifact | skill
```

Required constraints:

- `summary` must fit in one sentence.
- `key_claims`, `decisions`, and `next_actions` must be bounded arrays.
- `evidence_refs` must point to real local paths, schema names, test names, or
  external references with clear provenance.
- `interfaces` must name concrete schemas, commands, modules, or persisted
  paths when applicable.
- `generated_by` must distinguish human-authored, agent-generated, and
  skill-generated blocks.

## 5. Generation Policy

Agents and skills may generate or refresh `DocFrame` blocks, but they must obey
the same contract discipline as other persisted project metadata.

Allowed writers:

- document authoring tools
- research skills that create reports, plans, or reviews
- milestone execution tooling
- explicit operator commands such as `docs frame refresh`

Forbidden behavior:

- silently rewriting document meaning during compaction
- changing `lifecycle` to `superseded` without evidence
- using a stale `DocFrame` as stronger evidence than the document body
- allowing each skill to invent a private metadata shape
- letting a skill publish or refresh a public-latest document without a
  validated `SkillOutputEnvelope` and publication decision once the general
  skill-output runtime is live

Every generated block should include:

- the source document path
- the generation or refresh actor
- evidence references used to create the block
- a timestamp
- a validation status in the derived index

General publication policy:

- `DocFrame` is the document context block, not the publication authority
- the publication gate decides whether a DocFrame-bearing document is private,
  a review candidate, or public-latest
- public-latest publication must update the artifact-family pointer and
  canonical surface atomically with the document frame
- prompt/resume/compact should consume public-latest DocFrames by default and
  consume private candidates only when the operator selects them

## 6. Runtime Uses

`DocFrame` objects are useful because they create a low-cost context layer above
raw documents.

Required first-class uses:

- `docs index`: parse project documents and write `.pmcli/docs/index.json`
- `docs inspect <path>`: show the normalized `DocFrame` and validation state
- `docs frame refresh <path>`: regenerate or update the block through an
  approved agent or skill
- compact/resume: include relevant `DocFrame` projections when rebuilding
  long-horizon context
- review packets: include `DocFrame` projections for packet contents so blind
  reviewers can understand what each document contributes
- milestone planning: derive milestone summaries and implementation gaps from
  active milestone `DocFrame` objects

`DocFrame` should be treated as a context index, not as a second source of
truth. When the block and body disagree, the body wins and the block is stale.

## 7. Index Semantics

The derived `.pmcli/docs/index.json` should contain:

```text
DocIndex {
  index_id
  project_id
  generated_at
  doc_frames[]
  invalid_docs[]
  missing_required_frames[]
  stale_frames[]
  mission_frame_candidates[]
}
```

The index is rebuildable. It must not become runtime authority.

Required invalidation triggers:

- source document mtime/hash changed after `DocFrame.updated_at`
- schema version changed
- referenced evidence path disappeared
- lifecycle points to a superseding document that does not exist
- milestone summary disagrees with active `MissionFrame`

## 8. First Implementation Slice

Minimum useful slice:

1. create `schemas/doc_frame.schema.json`
2. create `schemas/doc_index.schema.json`
3. add `docs index`, `docs inspect`, and `docs frame refresh --dry-run`
4. require new `docs/deep_study` policy/plan docs to include `DocFrame`
5. make compact/resume able to read `.pmcli/docs/index.json` as optional
   context
6. add conformance tests for valid, invalid, stale, and missing-block cases

Migration should be incremental. Existing documents may be indexed as
`missing_required_frames` until the project deliberately backfills them.

## 9. Value Over Reference Designs

This addition is valuable because most reference systems preserve context
through either raw transcript summaries or memory records. `DocFrame` creates a
third path:

- lighter than rereading documents
- more grounded than free-form summaries
- more structured than ordinary Markdown headings
- easier to validate than prompt-only context blocks

It improves the long-horizon project task without contradicting the current
design. The important guardrail is that `DocFrame` feeds context and proposals;
it does not replace schemas, document bodies, or the canonical `MissionFrame`.
