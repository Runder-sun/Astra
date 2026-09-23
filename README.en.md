# Astra

An experimental open-source research framework for individual researchers, with a local browser workbench. Organize literature, experiments, independent reviews, and the evidence behind each conclusion.

[中文](README.md) · [Website](https://runder-sun.github.io/Astra/) · [Download v0.1.0-alpha.1](https://github.com/Runder-sun/Astra/releases/tag/v0.1.0-alpha.1)

**Experimental release, not a validated autonomous scientist.** Full live research acceptance remains incomplete. `main` contains development changes beyond the download; the workbench and detailed guides are currently in Chinese.

![The actual alpha.1 workbench, before starting a research task](docs/assets/workbench-alpha1.png)

## Start locally

Use Linux, Node.js 22.19 or newer, npm, and the official Codex CLI with a signed-in supported account. Historical live checks used Codex CLI 0.153.4 and `gpt-5.6-luna`; other systems and CLI versions are unverified. See the dated [support record](docs/support.md) for the distinction between software checks and live model validation.

```bash
mkdir astra-test
cd astra-test
npm init -y
npm install --ignore-scripts https://github.com/Runder-sun/Astra/releases/download/v0.1.0-alpha.1/earendil-works-pi-astra-0.1.0-alpha.1.tgz
npx --no-install astra-workbench --root ./research --port 4319
```

Open `http://127.0.0.1:4319`. Create a bounded question, select 24 tasks for a first trial, and leave paper delivery unchecked. For example: “With a fixed random seed, compare the mean and median on small contaminated normal samples. Use only the Python standard library; retain source, raw results and failures. Do not claim methodological novelty.”

## What to expect

- Inspect stage requirements, reviews, unresolved issues, and adopted deliverables.
- Pause and provide guidance before continuing. Task budgets are not monetary or subscription limits.
- Keep the complete research directory. Closing the browser does not stop a task; restarting the service does not automatically take over its process.
- Review original evidence yourself. Process completion does not imply a supported hypothesis, reproducibility, or publication-ready output.

The workbench is for a single local user. Research can execute generated code; use an isolated environment. The package retains an internal Pi name but is distributed by Astra, independently of upstream Pi.

[User guide](docs/getting-started.md) · [Examples and evidence limits](docs/examples/README.md) · [Development](docs/development/README.md) · [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [MIT license](LICENSE)
