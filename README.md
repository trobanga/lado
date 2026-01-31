# lado

A Git diff viewer GUI with unified and side-by-side views, built with [Slint](https://slint.dev/) and Rust.

<!-- TODO: Add screenshot here -->
<!-- ![lado screenshot](docs/screenshot.png) -->

## Features

- **Unified and side-by-side diff views** - Toggle between viewing modes with a single click
- **Hierarchical file tree** - Browse changed files in a collapsible tree structure
- **GitHub PR support** - View diffs for pull requests using the `gh` CLI
- **Multiple diff targets** - Compare against branches, commits, or PRs
- **Dark theme** - Easy on the eyes

## Installation

### From source

```bash
git clone https://github.com/trobanga/lado.git
cd lado
cargo install --path .
```

### Requirements

- Rust 1.70+
- For PR support: [GitHub CLI](https://cli.github.com/) (`gh`) must be installed and authenticated

## Usage

```bash
# Compare HEAD against main/master branch
lado

# Compare HEAD against a specific branch
lado feature-branch

# Compare HEAD against a specific commit
lado abc123

# View a pull request diff (requires gh CLI)
lado 42
lado #42

# Generate shell completions
lado --completions bash > ~/.local/share/bash-completion/completions/lado
lado --completions zsh > ~/.zsh/completions/_lado
lado --completions fish > ~/.config/fish/completions/lado.fish
```

## Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Run linter
cargo clippy
```

## License

Apache-2.0
