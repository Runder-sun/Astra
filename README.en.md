# Astra

An experimental open-source research framework for individual researchers, with a local browser workbench. Organize literature, experiments, independent reviews, and the evidence behind each conclusion.

[中文](README.md) · [Website](https://runder-sun.github.io/Astra/) · [Download v0.1.0-alpha.2](https://github.com/Runder-sun/Astra/releases/tag/v0.1.0-alpha.2)

**Current download: v0.1.0-alpha.2, an experimental release.** This version improves scheduling, frozen evidence, reviews and interrupted work. Full live research acceptance remains incomplete; convergence and publication quality are not guaranteed. The workbench and detailed guides are currently in Chinese.

![The actual alpha.1 workbench, before starting a research task](docs/assets/workbench-alpha1.png)

## Start locally

Use Linux, Node.js 22.19 or newer, npm, and the official Codex CLI with a signed-in supported account. Historical live checks used Codex CLI 0.153.4 and `gpt-5.6-luna`; other systems and CLI versions are unverified. See the dated [support record](docs/support.md) for the distinction between software checks and live model validation.

```bash
mkdir astra-test
cd astra-test
npm init -y
npm install --ignore-scripts https://github.com/Runder-sun/Astra/releases/download/v0.1.0-alpha.2/earendil-works-pi-astra-0.1.0-alpha.2.tgz
npx --no-install astra-workbench --root ./research --port 4319
```

Open `http://127.0.0.1:4319`. Create a bounded question, select 24 tasks for a first trial, and leave paper delivery unchecked. For example: “With a fixed random seed, compare the mean and median on small contaminated normal samples. Use only the Python standard library; retain source, raw results and failures. Do not claim methodological novelty.”

## What to expect

- Inspect stage requirements, reviews, unresolved issues, and adopted deliverables.
- Pause and provide guidance before continuing. Task budgets are not monetary or subscription limits.
- Keep the complete research directory. Closing the browser does not stop a task; restarting the service does not automatically take over its process.
- Review original evidence yourself. Process completion does not imply a supported hypothesis, reproducibility, or publication-ready output.

## How research proceeds

A bounded objective leads to a reviewed plan, separate execution tasks, frozen outputs and independent review. The main research agent decides whether to adopt the result, repair it, explore another direction or finish. Versions, failures and unresolved requirements remain inspectable; the stages are not a fixed pipeline.

Search results, abstracts and captured full text have different evidence limits. Optional paper delivery still requires reproducible experiments and a human review of the manuscript.

## Changes in alpha.2

- Incremental repairs retain exact frozen files across review, downstream inputs and downloads.
- Worker batches shrink to the remaining turn budget instead of stopping with usable capacity.
- Requests and child streams preserve split UTF-8 text and safe bounded Unicode tails.
- Session audits verify registered deliveries, reviewer targets and actual cleanup archives.
- Recovery checks durable deliveries before retrying and preserves review, adoption and user gates.

The latest kernel repair passed 50 independent acceptance criteria and 657 targeted offline tests. These measure software behavior, not scientific success. Package and installation checks are recorded separately in the [alpha.2 release record](docs/releases/v0.1.0-alpha.2.md).

## Upgrade safely

Pause work, confirm old processes have stopped, and back up the complete research directory before installing into a new directory. Do not let old and new versions write the same job concurrently. Historical budget gates keep their original requirements; inspect the saved work and budget before resuming.

The workbench is for a single local user. Research can execute generated code; use an isolated environment. The package retains an internal Pi name but is distributed by Astra, independently of upstream Pi.

[User guide](docs/getting-started.md) · [Examples and evidence limits](docs/examples/README.md) · [Development](docs/development/README.md) · [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [MIT license](LICENSE)
