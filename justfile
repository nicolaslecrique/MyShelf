# Single source of truth for all checks, used by both CI and local runs.

# Run everything, cheapest first, fail fast.
check: fmt clippy test

fmt:
    cargo fmt --all -- --check

clippy:
    cargo clippy --all-targets --all-features --locked -- -D warnings

test:
    cargo test --all-features --locked

# Start the MCP server (config from myshelf.toml).
run:
    cargo run
