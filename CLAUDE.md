# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**lado** is a Git diff viewer GUI built with Slint (Rust UI framework). It displays diffs similar to GitHub's interface with unified and side-by-side views.

## Build Commands

```bash
cargo build              # Build debug
cargo build --release    # Build release
cargo run -- <target>    # Run with diff target
cargo test               # Run all tests
cargo clippy             # Lint
```

## CLI Usage

```bash
lado                     # Compare HEAD to main/master
lado <branch>            # Compare HEAD to branch
lado <commit>            # Compare HEAD to commit hash
lado 42                  # Compare PR #42 (uses gh CLI)
lado --completions zsh   # Generate shell completions
```

## Architecture

### Data Flow
```
CLI args → DiffTarget → git2 (compute diff) → Slint UI models → render
              ↓
         PR number? → gh CLI (fetch base/head refs)
```

### Rust Modules

- **`app.rs`** - Application state, connects git data to Slint UI via `MainWindow`
- **`cli.rs`** - Clap argument parsing, `DiffTarget` enum (DefaultBranch/Ref/PullRequest)
- **`git/repository.rs`** - Opens repo, resolves refs, computes diffs via git2
- **`git/diff.rs`** - Data structures: `FileChange`, `DiffHunk`, `DiffLine`
- **`git/file_tree.rs`** - Builds hierarchical tree from flat file list (not yet integrated)
- **`github.rs`** - Fetches PR info via `gh pr view --json`
- **`models/`** - Converts git types to Slint-compatible structs
- **`highlighting/syntax.rs`** - Syntect integration (not yet integrated)

### Slint UI Files (`ui/`)

- **`main.slint`** - MainWindow component, exports `FileEntry` and `DiffLine` structs to Rust
- **`theme.slint`** - Color palette (dark theme)
- **`structs.slint`** - Shared data structures
- **`components/`** - UI components (file_tree, diff_view, unified, side_by_side, toolbar)

### Rust ↔ Slint Integration

The `slint::include_modules!()` macro in `main.rs` generates Rust types from `.slint` files. The `MainWindow` type and structs like `FileEntry`, `DiffLine` are accessed directly in Rust code.

Slint properties are set via `window.set_*()` methods. Callbacks are registered with `window.on_*()`.

## Testing

```bash
cargo test                           # All tests
cargo test cli::tests                # CLI parsing tests
cargo test git::file_tree::tests     # File tree tests
cargo test highlighting::syntax      # Syntax highlighting tests
```

## Issue Tracking (Beads)

Uses [beads](https://github.com/steveyegge/beads) for distributed issue tracking.

```bash
bd list                              # View all issues
bd ready                             # View ready-to-work issues
bd show diff-xxx                     # View issue details
bd update diff-xxx --status in-progress  # Start work
bd close diff-xxx                    # Complete issue
bd sync                              # Commit and push changes
```

Include issue IDs in commits: `git commit -m "Add feature (diff-xxx)"`
