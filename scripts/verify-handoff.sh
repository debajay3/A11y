#!/usr/bin/env bash
# Regression checks for the reviewed A11y handoff. Run --live on an unrestricted machine.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --release -p a11y-doc -p a11y-engine -p a11y-library --lib
cargo test --locked --release -p a11y-mcp --lib --test session --test stdio
cargo test --locked --release -p a11y-server --lib --bin a11y-library-server
cargo test --locked --release -p a11y-app --bin a11y -- --skip over_real_network
if [[ "${1:-}" == "--live" ]]; then
  cargo test --locked --release -p a11y-server --test server
  cargo test --locked --release -p a11y-app --bin a11y over_real_network
fi
