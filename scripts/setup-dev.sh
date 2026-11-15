#!/usr/bin/env bash
# One-time dev environment setup. Safe to re-run.
set -euo pipefail

if ! command -v cargo >/dev/null; then
  echo "cargo not found: install Rust first (https://rustup.rs)" >&2
  exit 1
fi

rustup component add rustfmt clippy

if command -v just >/dev/null; then
  echo "just already installed: $(just --version)"
else
  cargo install just --locked
fi

if command -v cargo-deny >/dev/null; then
  echo "cargo-deny already installed: $(cargo deny --version)"
else
  cargo install cargo-deny --locked
fi

echo "Done. Run 'just' to execute all checks."
