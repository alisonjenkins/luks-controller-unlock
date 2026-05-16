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

## Hard-won lessons (recorded so we don't relearn them)

### cryptsetup `--key-file=-` is a binary keyfile, not a passphrase prompt

`cryptsetup luksAddKey` reads `-` (stdin) as a binary keyfile and
does **not** strip trailing newlines. Writing `existing\nnew\n` on
stdin makes the existing-pass `existing\nnew` (one trailing `\n`
stripped by happy accident) and the new keyslot `<empty>`. Enroll
must:
- pass `--keyfile-size $existing.len()` so cryptsetup reads exactly
  that many bytes for the existing pass, and
- write the new key WITHOUT any trailing newline.

See `src/enroll.rs::add_keyslot`.

### systemd-cryptsetup waits forever on a single ask file

If the agent fails to reply to an `ask.<random>` file in
`/run/systemd/ask-password/`, systemd-cryptsetup does **not**
generate a new ask file — it blocks indefinitely. The agent must:
- NOT mark a request as `handled` until it has successfully replied
  (a DRM-open ENOENT during early-boot amdgpu init is a transient
  error; the agent must retry the same ask file), and
- back off briefly between retries so a permanent error doesn't
  busy-spin.

See `src/agent.rs::run`.

### hid-steam swaps physical X/Y vs the Linux gamepad convention

Both upstream Linux 7.x hid-steam and Valve's `linux-*-valve1`
hid-steam fork emit `BTN_WEST` for the Deck's physical Y button
and `BTN_NORTH` for physical X — the inverse of Xbox / DualSense /
Switch Pro. **Do not try to "correct" this.** The PIN encoding is
arbitrary as long as it's self-consistent between enroll and
unlock; pressing the same physical button always produces the same
kernel code and therefore the same encoded char. The cosmetic
mismatch (test-input UI says "X" when user presses Y) is acceptable.
A prior attempt to apply a per-driver swap broke enroll/unlock
consistency and bricked unlock; see commits `2de7e4a` / `33b304d` /
`eb85da7`.

### Embedded portrait panels need rotation at blit time

Steam Deck-class devices report an `EmbeddedDisplayPort` connector
with portrait native dimensions (e.g. 800x1280) but are held in
landscape. The renderer paints a logical landscape pixmap; the
`blit_rgba_to_xrgb` in `src/ui/render.rs` rotates 90° CW when
`DrmSurface::open` set `Rotation::Cw90`. Detection heuristic in
`src/ui/drm.rs`: `connector == EmbeddedDisplayPort && height > width`.
Future: read the DRM connector's panel-orientation property to
handle "left side up" panels too. Don't try to fix this by
modesetting at swapped dimensions — the kernel won't accept it.

### `boot.initrd.systemd.storePaths` doesn't follow ExecStart= closure

Any `writeShellScript` (or other store path) referenced from an
initrd-systemd unit's `serviceConfig.ExecStart` must be listed
explicitly in `boot.initrd.systemd.storePaths`, otherwise the unit
runs but exec'ing the script fails with 203/EXEC because the path
is a dangling reference in the initrd cpio. Same for any tools the
script calls (full /nix/store paths are baked into a writeShellScript,
so you must storePath each one). See `dist/nix/module.nix` —
`agentWrapper`, `journalDump` and their referenced bash + coreutils
+ util-linux are all listed.

### `boot.initrd.systemd.mounts` + `StandardOutput=append:` race

systemd opens the unit's `StandardOutput=` redirection BEFORE it
honours the `After=` ordering on a separate mount unit. A mount of
the ESP at `/boot-debug` declared via `boot.initrd.systemd.mounts`
isn't active when the agent unit tries to open
`append:/boot-debug/luks-controller-unlock.log` → 209/STDOUT and
the agent never runs. Workaround in `dist/nix/module.nix`: wrap
the agent in a shell script that does the mount itself (and falls
back to stderr if mount fails so we still get journal output).

### NixOS impermanence `/var/log` is empty unless journald is `persistent`

Default systemd-journald `Storage=auto` only persists if
`/var/log/journal/` already exists. On a fresh impermanence root
that bind-mounts an empty `/persistence/var/log` over `/var/log`,
the directory doesn't exist → journald uses volatile `/run/log/journal`
→ logs vanish on reboot. Set `services.journald.extraConfig =
"Storage=persistent"` to force creation of the directory on first
boot. This was the missing piece that made post-boot diagnostics
possible.

### Multi-cpio initrd extraction

NixOS systemd-stage-1 produces a concatenated cpio archive:
kernel-modules cpio + (zero-padded) zstd-compressed main rootfs
cpio. `cpio -id < combined` only reads the first segment. To get
the rootfs:
1. Find `TRAILER!!!`, advance past it to a 4-byte boundary.
2. Skip leading null bytes (padding).
3. The remainder starts with the zstd magic `28 b5 2f fd`.
4. `zstd -dc | cpio -id` to extract.

Useful when verifying that a unit / wrapper script actually landed
in the initrd before deploying.

### TPM2 + systemd-cryptsetup segfault (systemd 258.7)

systemd 258.7's `systemd-cryptsetup` segfaults in
`libsystemd-shared` during the TPM2 unlock attempt on a Deck even
when no TPM2 keyslot is enrolled. The crash happens after
"Successfully created primary key on TPM" and BEFORE the
keyfile/ask-password fallback. Workaround: remove
`tpm2-device=auto` from crypttab options for the affected volume.
Re-enable once systemd is patched.

### Steam Deck IMU drives the gamepad device at ~250 Hz

The Deck's built-in controller's IMU pumps ABS_X/Y/RX/RY events
constantly while the agent is open. Per-event debug-level logging
(e.g. "raw event") drowns the journal in seconds. Keep raw-event
trace at `trace!` not `debug!`. The wrapper unit should pass `-v`
(debug), NOT `-vv` (trace), to the agent for normal runs.
