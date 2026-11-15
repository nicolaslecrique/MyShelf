# Single source of truth for all checks, used by both CI and local runs.

# Run everything, cheapest first, fail fast.
check: fmt clippy test deny

fmt:
    cargo fmt --all -- --check

clippy:
    cargo clippy --all-targets --all-features --locked -- -D warnings

# Runs the tests under coverage (needs cargo-llvm-cov) and fails if line coverage
# drops below 95%. Stable Rust can't exclude code from coverage, so it's a floor.
test:
    cargo llvm-cov --all-features --locked --show-missing-lines --fail-under-lines 95

# Licenses, advisories, banned/unknown sources (needs cargo-deny).
deny:
    cargo deny check

# Start the MCP server (config from myshelf.toml).
run:
    cargo run

# Build the production Docker image.
docker-build:
    docker build -t myshelf .

# Deploy the image CI published for the current commit (HEAD must be pushed
# to main and its CI run green).

# Deploy the current commit to Scaleway (see README).
deploy:
    terraform -chdir=infra/scaleway apply -var "image_tag=$(git rev-parse HEAD)"
