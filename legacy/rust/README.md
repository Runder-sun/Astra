# Astra legacy Rust runtime

This directory preserves the pre-Pi Astra runtime for migration forensics,
behavioral baselines, and parity checks. It is not a product entry point.

The repository's default build, test, package, and release flows use the Pi
monorepo and `packages/astra`; they do not compile or package this Cargo crate.
No new provider, agent-loop, CLI, TUI, remote, or research product capability
should be added here.

Run archived checks explicitly from the repository root:

```bash
cargo fmt --check --manifest-path legacy/rust/Cargo.toml
cargo test --manifest-path legacy/rust/Cargo.toml --lib
bash legacy/rust/scripts/run_mock_parity_harness.sh
bash legacy/rust/scripts/run_parity_demo_suite.sh
```

The Rust package scripts resolve paths relative to this directory. Generated
archives and Cargo build output stay under `legacy/rust/dist/` and
`legacy/rust/target/` unless their existing environment overrides are used.
