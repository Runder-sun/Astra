# Third Party Notices

## Claw-Code Rust UI Components

Parts of Astra's terminal input and Markdown rendering infrastructure are
adapted from the Rust workspace of Claw-Code:

- Source path in this repository: `reference_repos/requested/claw-code/rust/`
- Upstream workspace license: MIT, declared in
  `reference_repos/requested/claw-code/rust/Cargo.toml`

The adapted code is limited to terminal UI infrastructure patterns such as
line editing, completion helpers, stream-safe Markdown rendering, and terminal
formatting. Astra's runtime, provider routing, session store, skills, remote
control, and project model remain implemented by this repository.
