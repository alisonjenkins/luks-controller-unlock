# Shared setup notes

> ⚠️ **This tool is new.** No per-distro guide here has been
> end-to-end booted by the author across every covered distro yet.
> Treat each guide as the *intended* path, not a known-good playbook.
> Climb [`TESTING.md`](../../TESTING.md) before installing anything in
> initrd, and keep your keyboard keyslot intact as the recovery path.

These steps are referenced from each per-distro guide. Read them once.

## Prerequisites (all distros)

* Rust toolchain (any recent stable, ≥ 1.75) with `cargo`. Easiest:
  use the repo's nix flake — `nix develop` provides a pinned toolchain
  and all C libraries.
* `pkg-config`, `gcc` (or `clang`), libdrm headers — provided by the
  flake. On non-NixOS hosts: `pacman -S pkgconf libdrm` /
  `apt install pkg-config libdrm-dev` etc.
* `cryptsetup` ≥ 2.6.
* A wired or USB-dongle game controller. Bluetooth controllers are
  out of scope for v1 (initrd has no `bluez`).
* An existing keyboard passphrase keyslot on the LUKS volume. Never
  remove it — it's your recovery path.

## Build the binary

```sh
git clone https://github.com/<you>/luks-controller-unlock.git
cd luks-controller-unlock

# With nix (recommended, gets the pinned toolchain + libdrm):
nix develop --command cargo build --release

# Without nix:
cargo build --release
```

Output: `target/release/luks-controller-unlock`.

For initrd use, a static musl build is friendlier than glibc:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

## Pre-flight: backup the LUKS header

Do this once, before enrolling. It's the difference between "I broke
my boot" and "I can never decrypt this disk again".

```sh
sudo cryptsetup luksHeaderBackup /dev/sdaX \
    --header-backup-file ~/luks-header-$(date +%F).img
```

Copy the backup file off the encrypted device — onto a USB stick or
another disk. Keep it.

## Enroll the controller PIN

Enroll runs from a regular shell. You need the existing keyboard
passphrase to add a new keyslot.

```sh
sudo target/release/luks-controller-unlock enroll --device /dev/sdaX
```

It:
1. Prompts for the existing passphrase on the tty (echo disabled).
2. Opens the first connected gamepad.
3. Asks you to press the PIN sequence twice (unless `--no-confirm`).
4. Shells out to `cryptsetup luksAddKey --batch-mode --pbkdf-memory 262144`.

The PIN is shown only as a dot row on stderr — buttons never echo.

`--pbkdf-memory 262144` (256 MiB) keeps Argon2id verification at boot
well below typical initrd OOM thresholds. Override with
`--pbkdf-memory-kib N` if you've measured something specific.

After enroll, before installing the initrd packaging, verify with:

```sh
sudo cryptsetup luksDump /dev/sdaX | grep -E "^  [0-9]+:"
```

You should see one more keyslot than before.

## Recommended kernel cmdline

The boot console draws `printk` directly into our framebuffer until a
future version switches the tty to `KD_GRAPHICS`. Until then, suppress
the kernel log overlay:

```
quiet loglevel=3
```

Add to your bootloader config (GRUB: `GRUB_CMDLINE_LINUX_DEFAULT`;
systemd-boot: the entry's `options=`).

## Common troubleshooting

| Symptom | Cause / fix |
|---|---|
| `no controller detected` | Tool matches devices that advertise `BTN_SOUTH`. Some cheap USB receivers expose the controller as a HID joystick. Run `test-input` to see what enumerates. |
| `no DRM card with a connected output` | Behind a DisplayPort hub / KVM that doesn't report `connected` until the OS handshakes. Plug the display in directly. |
| Steam Controller does nothing | `hid-steam` kernel module not loaded *in the initrd*. The dist/ packaging includes it; if you customised your initrd, re-add it. |
| Two prompts on screen at boot | The stock console password agent and ours both fire. Safe to ignore (keyboard fallback works either way) or mask the console agent with the drop-in under `dist/systemd/systemd-ask-password-console.service.d/`. |
| Plymouth flashes briefly | Expected. Our agent declares `Conflicts=plymouth-start.service` so plymouth releases DRM master before we draw. |
