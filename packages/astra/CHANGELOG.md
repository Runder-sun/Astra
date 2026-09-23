# Astra changes

## [Unreleased]

### Changed

- Unified public installation instructions around the fixed alpha.1 download; moved technical reference material into developer documentation.
- Moved historical Rust sources out of the current tree; the published alpha.1 tag remains the archive reference. Current migration reporting stays available.
- Pinned direct Pi dependencies to 0.84.1 for future packages.
- Separated Astra packaging and source export from upstream release commands.

### Fixed

- Source export handles removed files and includes current user guides and Astra workflows.
- Runtime boundary checks no longer require historical Rust files.
- Packaged workbench checks install production dependencies before testing the isolated server.

Older development notes remain in [changes.md](changes.md). Published alpha.1 notes and assets are unchanged.
