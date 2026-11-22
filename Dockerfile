# syntax=docker/dockerfile:1

# Official cargo-chef image (rust:1-slim with cargo-chef preinstalled, so it
# isn't compiled from source on each new Rust release). Build stages only: the
# runtime image below is distroless. cargo-chef pinned, Rust follows 1.x.
# Bookworm (Debian 12) to match the runtime's glibc: a binary built against a
# newer glibc (trixie, the plain `-slim` default) may not start on it.
FROM lukemathwalker/cargo-chef:0.1.78-rust-1-slim-bookworm AS chef
WORKDIR /app

# Compute the dependency recipe (only changes when Cargo.toml/lock change).
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

# Build dependencies first (cached layer), then the app.
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

# Minimal runtime: glibc + CA certs + non-root user, no shell.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /app/target/release/myshelf /myshelf
# Scaleway Serverless Containers: set the container port to 8080.
ENV MYSHELF_LISTEN_ADDRESS=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["/myshelf"]
