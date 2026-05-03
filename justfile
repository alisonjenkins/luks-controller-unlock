alias l := list
alias c := check
alias k := clippy
alias b := build
alias br := build-release
alias t := test
alias f := fmt
alias fc := fmt-check
alias nb := nix-build
alias C := clean

# List all available recipes.
list:
    @just --list

# Type-check the workspace (uses cross target aarch64-linux on macOS).
check:
    cargo check --all-targets

# Run clippy lints, deny warnings.
clippy:
    cargo clippy --all-targets -- -D warnings

# Build a debug binary. Produces aarch64-linux ELF on macOS via zig linker.
build:
    cargo build

# Build an optimised release binary.
build-release:
    cargo build --release

# Run cargo tests. Native cargo nextest on linux; routes via linux-builder on macOS (cached on unchanged source).
test:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ "$(uname)" == "Darwin" ]]; then
        echo "→ Running tests on linux-builder via nix (rustPlatform checkPhase)..." >&2
        nix build .#packages.aarch64-linux.default --print-build-logs --no-link
    else
        cargo nextest run
    fi

# Format Rust and Nix sources in place.
fmt:
    cargo fmt
    nix fmt

# Verify formatting without writing changes (CI-style).
fmt-check:
    cargo fmt -- --check

# Build the package via nix; runs cargo test in checkPhase. Routes through the linux-builder on macOS.
nix-build:
    nix build .#packages.aarch64-linux.default

# Run nix flake check (eval + per-system derivation evaluation).
flake-check:
    nix flake check

# Update all flake inputs to latest revisions.
flake-update:
    nix flake update

# Remove cargo build artefacts.
clean:
    cargo clean
