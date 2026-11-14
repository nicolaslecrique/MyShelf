# syntax=docker/dockerfile:1

# Pinned to the Rust edition/toolchain used locally (edition 2024).
FROM rust:1-slim AS chef
RUN cargo install cargo-chef --locked
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
