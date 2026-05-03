# CLAUDE.md

Guidance for Claude Code (claude.ai/code) when working in this repo.

## Project

`luks-controller-unlock` — Rust binary that unlocks LUKS2 volumes from
initrd using a game controller (DRM/KMS UI, evdev input, systemd
ask-password protocol). Single-bin crate, MSRV 1.75. Targets Linux
exclusively at runtime. Cargo manifest lives at the repo root; nix
packaging is in `dist/nix/`.

## Always work inside the nix devshell

This repo is a Nix flake with `direnv` integration via a custom `.envrc`.
**Do not run `cargo` / `rustc` / `pkg-config` from outside the devshell** —
the toolchain, cross linker, and `libdrm` `.pc` file are all provisioned
by the shell.

- `direnv allow .` in the repo populates the shell automatically.
- Otherwise: `nix develop` enters the same shell.
- The devshell branches on host OS (see flake.nix):
  - **Linux** — native `mkShell` with libdrm/systemdMinimal/cryptsetup as
    real `buildInputs`. `cargo build` emits a native binary.
  - **macOS (aarch64-darwin)** — cross devshell. fenix `combine`
    overlays the aarch64-linux rust-std on the darwin host toolchain;
    `zig cc -target aarch64-linux-gnu` is the cargo linker;
    `legacyPackages.aarch64-linux.libdrm` is exposed via
    `PKG_CONFIG_PATH` / `PKG_CONFIG_SYSROOT_DIR`. `CARGO_BUILD_TARGET`
    defaults to `aarch64-unknown-linux-gnu`, so every `cargo build`
    produces an aarch64-linux ELF — those binaries cannot run on the
    Mac itself.

## Use the justfile, not raw cargo invocations

`just` is on PATH inside the devshell. `just list` (or just `just`) prints
all recipes. Common ones:

| recipe | alias | what it does |
|---|---|---|
| `check` | `c` | `cargo check --all-targets` |
| `clippy` | `k` | `cargo clippy --all-targets -- -D warnings` |
| `build` | `b` | `cargo build` |
| `build-release` | `br` | `cargo build --release` |
| `test` | `t` | runs tests — see below |
| `fmt` | `f` | `cargo fmt` + `nix fmt` |
| `fmt-check` | `fc` | verify formatting (CI-style) |
| `nix-build` | `nb` | `nix build .#packages.aarch64-linux.default` |
| `flake-check` |  | `nix flake check` |
| `clean` | `C` | `cargo clean` |

## Running tests

`just test` is the right command on both OSes — but the path differs:

- **Linux** — runs `cargo nextest run` natively.
- **macOS** — invokes `nix build .#packages.aarch64-linux.default`,
  which runs the Rust test suite inside `rustPlatform.buildRustPackage`'s
  `checkPhase` on the nix-darwin `linux-builder` VM. The build is content-
  addressed: tests only re-run when the source actually changes. ~1m20s
  cold for a full release+test cycle, ~50ms when cached. Output is
  copied back to the local store.

There is no per-test ssh runner. Iterating on a single test on macOS
means either editing the source (which invalidates the nix cache) or
running `cargo nextest run` directly on a Linux machine.

## File layout

- `src/main.rs` — clap CLI dispatch
- `src/agent.rs` — systemd ask-password agent loop
- `src/enroll.rs` — PIN enrollment
- `src/keyscript.rs` — cryptsetup integration
- `src/selftest.rs` — runtime validation
- `src/pin.rs` — PIN representation + canonical encoding
- `src/input/` — evdev controller handling
- `src/ui/` — DRM/KMS surface, tiny-skia rendering
- `dist/nix/default.nix` — `rustPlatform.buildRustPackage` definition
- `dist/nix/module.nix` — NixOS module that wires this into `boot.initrd`
- `flake.nix` — devshells (per-OS branch) + flake outputs
- `.envrc` — direnv loader; caches `nix print-dev-env` output to skip
  per-flake-input gcroot creation. See `~/git/personal/nix-config/.envrc`
  for the rationale.

## Things not to do

- Don't add `cargo build` artefacts or `target/` to the Nix derivation —
  it uses `lib.cleanSource` deliberately.
- Don't widen `meta.platforms` in `dist/nix/default.nix` — runtime needs
  Linux (DRM + evdev + systemd).
- Don't change `CARGO_BUILD_TARGET` or unset the cross linker env on
  macOS — those are the cross-compile contract; cargo will fall back to
  trying to link a Mach-O binary against libdrm and break.
- Don't install `cargo-edit` / `rust-analyzer` etc via rustup; the
  devshell already provides them.
