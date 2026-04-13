# lado - Git diff viewer GUI

# Build debug binary
build:
    cargo build

# Build release binary
release:
    cargo build --release

# Install to ~/.cargo/bin
install:
    cargo install --path .

# Run with arguments
run *ARGS:
    cargo run -- {{ARGS}}

# Run all tests
test:
    cargo test

# Run clippy lints
lint:
    cargo clippy

# Format code
fmt:
    cargo fmt

# Check formatting and lints without modifying
check:
    cargo fmt --check
    cargo clippy -- -D warnings

# Clean build artifacts
clean:
    cargo clean
