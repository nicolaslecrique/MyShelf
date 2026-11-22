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

# Start the MCP server (built-in defaults, overridden by myshelf.toml if present).
run:
    cargo run

# Build the production Docker image.
docker-build:
    docker build -t myshelf .

# Run terraform in infra/scaleway, e.g. `just tf plan`. The state backend speaks
# S3 and only reads the AWS_* names, so it gets the Scaleway key under them.
tf *args:
    AWS_ACCESS_KEY_ID="$SCW_ACCESS_KEY" AWS_SECRET_ACCESS_KEY="$SCW_SECRET_KEY" terraform -chdir=infra/scaleway {{args}}

# Once per checkout: download the provider and connect to the state bucket.
tf-init: (tf "init" "-backend-config=backend.tfbackend")

# Deploy the image CI published for the current commit to Scaleway (HEAD must
# be pushed to main and its CI run green; see README).
deploy: (tf "apply" "-var" "image_tag=$(git rev-parse HEAD)")
