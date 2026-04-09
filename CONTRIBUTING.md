# Contributing to rust-pad

Thank you for your interest in contributing to rust-pad! This document covers the conventions,
tooling, and workflow you need to get started.

## Table of Contents

- [Getting Started](#getting-started)
- [Development Environment](#development-environment)
- [Project Structure](#project-structure)
- [Git Workflow](#git-workflow)
- [Code Style](#code-style)
- [Error Handling](#error-handling)
- [Testing](#testing)
- [Performance](#performance)
- [Cross-Platform Guidelines](#cross-platform-guidelines)
- [UI Development](#ui-development)
- [Dependencies](#dependencies)
- [Security](#security)
- [Quality Checklist](#quality-checklist)
- [What Not to Do](#what-not-to-do)

---

## Getting Started

1. Fork the repository and clone your fork.
2. The project uses a pinned Rust toolchain (`rust-toolchain.toml` specifies the exact version).
   Running any `cargo` command will automatically install the correct toolchain if you have
   `rustup` installed.
3. Build and test the full workspace:
   ```bash
   cargo build --workspace
   cargo test --workspace
   ```
4. On **Linux**, you need system libraries for the GUI and file dialogs:
   ```bash
   sudo apt-get install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
     libxkbcommon-dev libssl-dev libgtk-3-dev
   ```

## Development Environment

| Tool       | Version / Source                               |
|------------|------------------------------------------------|
| Rust       | Pinned in `rust-toolchain.toml`                |
| Components | `rustfmt`, `clippy` (installed automatically)  |
| Editor     | Any; the project has no editor-specific config |

The workspace compiles on **Windows**, **macOS**, and **Linux**. CI runs on all three, so make sure
your changes build and pass tests on all platforms.

## Project Structure

rust-pad is a Cargo workspace with five crates under `crates/`:

| Crate                  | Role                                                                                                                             |
|------------------------|----------------------------------------------------------------------------------------------------------------------------------|
| `rust-pad`             | Binary crate. Thin entry point: parses CLI args, initializes logging, launches the app.                                          |
| `rust-pad-core`        | Core library. Text buffer (`ropey::Rope` wrapper), cursor, document model, encoding detection, search engine. No GUI dependency. |
| `rust-pad-ui`          | UI layer. egui/eframe application, editor widget, tabs, dialogs, menus, syntax highlighting.                                     |
| `rust-pad-config`      | Configuration, theme management, session persistence, permissions.                                                               |
| `rust-pad-mod-history` | Undo/redo history with tiered storage (in-memory + `redb` database for persistence).                                             |

**Key architectural rules:**

- **Core has no GUI dependency.** Business logic, text manipulation, and file I/O belong in
  `rust-pad-core` or `rust-pad-config`, never in the UI crate.
- **Keep `main.rs` thin.** It orchestrates; logic lives in library code.
- **One module per concern.** Split files when they exceed ~300 lines.
- Each crate is listed in the root `Cargo.toml` workspace `members`.

## Git Workflow

### Branches

| Prefix   | Purpose                                | Example             |
|----------|----------------------------------------|---------------------|
| `feat/`  | New features                           | `feat/auto-indent`  |
| `task/`  | Refactors, chores, infrastructure work | `task/sec-phase-2`  |
| `chore/` | Minor maintenance                      | `chore/fixing-typo` |

The default branch is `master`. Create a feature or task branch from `master` (or the current
development branch if one exists), open a pull request, and merge after CI passes.

### Commits

- Use **imperative mood**, verb-first: "Add horizontal scrolling", "Fix tab jitter",
  "Remove redundant tests".
- Keep the subject line concise. Use the body for details when needed.
- Reference related issues or PRs where applicable.

### Pull Requests

- One logical change per PR. Large features can be split into phases (e.g., `sec-phase-1`,
  `sec-phase-2`).
- PRs must pass CI (build + clippy + tests + fmt check on all three platforms) before merging.
- Include a clear description of what changed and why.
- Update `CHANGELOG.md` under the appropriate version section with your changes, following the
  existing format (Added / Changed / Fixed subsections with descriptive entries).

## Code Style

The project uses the default `rustfmt` and `clippy` configurations. **Do not change them.**

### Naming

| Element                              | Convention         | Example                         |
|--------------------------------------|--------------------|---------------------------------|
| Functions, variables, modules, files | `snake_case`       | `load_or_create()`              |
| Types (structs, enums, traits)       | `PascalCase`       | `TextBuffer`, `LineChangeState` |
| Constants                            | `UPPER_SNAKE_CASE` | `FLUSH_INTERVAL_SECS`           |

### Imports

Group imports in this order, separated by blank lines:

1. `crate::` (local crate)
2. External crates (`anyhow`, `ropey`, `egui`, etc.)
3. `std::`

No glob imports (`use foo::*`) in production code.

### Functions and Comments

- Keep functions to ~50 lines. Extract helpers when they grow beyond that.
- Prefer `match` over `if let` chains when handling multiple variants.
- Use guard clauses for early returns.
- Doc comments (`///`) on all public items, with `# Errors` and `# Panics` sections where
  applicable.
- Comments explain **why**, not what. No commented-out code.

### Derive Macros

- `#[derive(Debug, Clone)]` on config/model structs.
- Add `Copy`, `PartialEq`, `Eq` where appropriate.
- Use enums for discrete modes/states; `Option<T>` for optional fields.

## Error Handling

**`anyhow` is the sole error-handling crate.** Do not introduce `thiserror`, `eyre`, or alternatives.

- All fallible functions return `anyhow::Result<T>`.
- Attach context at every level:
  ```rust
  let content = std::fs::read_to_string(&path)
      .with_context(|| format!("failed to read {}", path.display()))?;
  ```
- Propagate errors with `?`.
- Use `anyhow::bail!("message")` or `anyhow::anyhow!("message")` for ad-hoc errors.
- **Never** use `unwrap()` or `expect()` outside test code.

## Testing

The project has 1,100+ tests. Maintain and extend test coverage where it makes sense.

### Conventions

- Unit tests live in `#[cfg(test)] mod tests` at the bottom of each file, using `use super::*`.
- Integration tests live in `crates/<crate>/tests/`.
- Use `tempfile::tempdir()` for any test that touches the file system.
- Follow **Arrange-Act-Assert**.
- Use helper functions to build test fixtures; avoid duplicating setup across tests.
- Use `assert!(matches!(...))` for enum variant checks.
- Test happy paths, error conditions, and meaningful boundary values.
- For UI components, test logic and state management independently from rendering.
  The project uses `egui_kittest` for UI integration tests.

### What to Test

- Any new public function or method.
- Bug fixes (add a regression test that would have caught the bug).
- Edge cases for text operations (empty buffers, Unicode, large files, line endings).

### What Not to Test

- Don't add tests purely for coverage numbers.
- Don't test trivial getters/setters or derived trait implementations.

## Performance

The project uses a version-based caching strategy to keep the UI responsive:

- `Document.content_version: u64` is bumped on every buffer mutation.
- Caches (max line chars, search occurrences, render galley) are keyed by this version and only
  recomputed when the content actually changes.

### General Rules

- Use `BufReader`/`BufWriter` for file I/O.
- Pre-allocate `Vec` capacity when size is known.
- Use `Cow<'_, str>` to avoid unnecessary allocations.
- Use `std::sync::LazyLock` for expensive one-time initializations (e.g., compiled regexes).
- For rendering code, keep frame budgets in mind (~16ms for 60fps). Avoid allocations in hot
  render paths.
- Use `buffer.byte_to_char()` (O(log n) via ropey) instead of `text[..pos].chars().count()` (O(n)).
- **Profile before optimizing.** Use `cargo flamegraph` or `criterion` for benchmarks.

## Cross-Platform Guidelines

rust-pad targets Windows, macOS, and Linux equally. Keep these in mind:

- **Line endings:** Internal representation is always LF. Convert to platform-appropriate endings
  on save only.
- **File paths:** Use `std::path::Path` / `PathBuf`. Never hardcode separators.
- **Config and data directories:** Use the `dirs` crate for platform-standard paths
  (`~/.config/rust-pad/` on Linux, `~/Library/Application Support/` on macOS, `%APPDATA%` on
  Windows). The `--portable` flag overrides this.
- **Permissions:** Set restrictive permissions (0700 dirs, 0600 files) on Unix. On Windows, NTFS
  ACLs handle user-scoped access automatically, so permission calls are no-ops.
- **Platform-specific code:** Isolate behind `#[cfg(target_os = "...")]` or
  `#[cfg(unix)]` / `#[cfg(not(unix))]`. Use these sparingly; prefer cross-platform abstractions.
- **Keyboard shortcuts:** Respect platform conventions (Cmd vs Ctrl on macOS).

## UI Development

Before making any visual changes, read `ui-guidelines.md` at the root of the repo for detailed
design guidelines (Material Design adapted for desktop, component specifications, accessibility
checklist, theming, and layout rules).

### Key UI Patterns

- The custom `EditorWidget` renders with egui primitives (not `TextEdit`).
- `SyntaxHighlighter` wraps syntect with egui integration.
- Use `ui.fonts_mut(|f| f.layout_job(job))` for galley layout (egui 0.33+ API).
- Menu bar: `egui::MenuBar::new().ui(ui, |ui| {})`.
- Close menus: `ui.close()` (not `ui.close_menu()`).
- Keyboard events: collect from `ctx.input()`, process in a `match` block.
- Never block the UI/main thread. Offload heavy work to background threads; use channels to send
  results back.

## Dependencies

All dependency versions are centralized in the root `Cargo.toml` under `[workspace.dependencies]`.
Individual crates reference them with `dependency.workspace = true`.

### Adding a New Dependency

1. Check if an existing dependency already covers the need.
2. Evaluate the crate: download count, recent maintenance activity, `unsafe` usage, platform
   support.
3. Add it to `[workspace.dependencies]` in the root `Cargo.toml`.
4. Reference it in the crate's `Cargo.toml` with `dependency.workspace = true`.
5. Feature-gate optional functionality to keep compile times and binary size down.

### Critical Version Pins

- `bincode = "=1.3.3"` is **permanently locked** (exact version pin). It is used for persistent
  undo history serialization. Changing the version would break deserialization of existing history
  databases. **Do not update this dependency.**

## Security

The project has undergone multiple security hardening phases. Maintain these standards:

- **Deserialization limits:** All data read from `redb` databases has size bounds to prevent OOM
  from corrupted files. Maintain these limits when adding new persistent data.
- **File size validation:** Files are checked against a configurable size limit before loading.
- **Encoding validation:** UTF-16 files with odd byte counts are rejected to prevent silent data
  corruption.
- **Permissions:** Config and data files are created with restrictive permissions (owner-only).
- **CI supply chain:** All third-party GitHub Actions are pinned to full commit SHAs, not mutable
  version tags. When adding or updating actions, always pin to a specific commit SHA.
- **Release integrity:** Release artifacts include SHA256 checksums for verification.

## Quality Checklist

Before submitting a PR, run these commands and ensure they all pass:

```bash
# Full workspace checks
cargo build --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo fmt --all -- --check
```

Or for a single crate:

```bash
cargo build -p <crate-name>
cargo clippy -p <crate-name> -- -D warnings
cargo test -p <crate-name>
cargo fmt -p <crate-name> -- --check
```

CI runs these same checks on Windows, macOS, and Linux. A PR cannot be merged if any check fails.

## What Not to Do

- **No `unsafe` code** unless explicitly approved and justified.
- **No `unwrap()` / `expect()`** in non-test code.
- **No alternative error libraries** (`thiserror`, `eyre`, etc.) without explicit approval.
- **No changes to `rustfmt` or `clippy` configuration.**
- **No blocking the UI thread** in GUI code.
- **No glob imports** (`use foo::*`) in production code.
- **No updating `bincode`** past `1.3.3`.