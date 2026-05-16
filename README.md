# luks-controller-unlock

Unlock LUKS2 root filesystems with a game controller. Built primarily
for Steam Machines and the Steam Deck — also useful on any HTPC with a
paired controller.

> **Status:** booted end-to-end on a Steam Deck OLED running NixOS
> (Jovian-NixOS, impermanence root, Valve `linux-*-valve1` kernel).
> Unlock UI auto-rotates 90° for the Deck's portrait-native panel,
> PIN entry via face buttons + d-pad works, and the system reaches
> Steam Big Picture. 30 unit tests pass. The other distro guides in
> [`docs/install/`](docs/install/) are still intended-recipes rather
> than known-good playbooks. Work through [`TESTING.md`](TESTING.md)
> before enrolling on any real disk, and keep your keyboard keyslot
> as the recovery path.

---

## Why

A Steam Deck or Steam Machine has a LUKS root and no comfortable way
to type a long passphrase at boot. The Deck has no built-in keyboard;
a Steam Machine sits under a TV with the keyboard, if there is one,
in a drawer. Asking yourself — or a less-technical household member —
to dock a USB keyboard and type 30 characters blind into the boot
prompt every cold boot is the friction that pushes people to one of
two bad choices:

1. Run the box unencrypted because "it's just a games box".
2. Use a 6-character passphrase because anything longer is unusable
   on a couch.

Neither is good. Same story on any HTPC with a controller already
paired. The fix is to use the input device that's already there: the
controller. This tool lets you unlock with a controller PIN while
keeping your existing keyboard passphrase intact as the recovery path.

Threat model: defends against opportunistic theft and casual access by
a houseguest. The controller PIN is registered as an ordinary LUKS2
keyslot via `cryptsetup luksAddKey` — no new crypto, no custom KDF.
Brute-force resistance is whatever Argon2id gives you, the same KDF
that protects keyboard passphrases; a PIN with a few buttons of
entropy is meaningfully expensive to attack offline. Your existing
keyboard passphrase keyslot is left untouched as the recovery path.

---

## What

Two phases:

1. **Enroll** (booted system, CLI). You press a sequence of controller
   buttons to define a PIN. The tool encodes the PIN deterministically
   to an ASCII passphrase and registers it as a normal LUKS2 keyslot
   via `cryptsetup luksAddKey`. No new crypto: the keyslot is exactly
   what `luksAddKey` would produce for any other passphrase.
2. **Unlock** (initrd, every boot). A small agent draws a fullscreen
   UI on the GPU, reads the controller, and replies to systemd's
   ask-password protocol (or to initramfs-tools' `keyscript=`) with
   the encoded passphrase. Standard keyboard passphrase entry keeps
   working for any other keyslot.

PIN alphabet: the 12 buttons present on every supported controller —
A, B, X, Y, LB, RB, LT, RT, Dpad N/S/E/W. **START** submits.
**B held ≥ 500 ms** is backspace (a B-tap is the literal `B` symbol).
PIN length is variable, capped at 256 buttons.

Supported controllers: anything that exposes the kernel's standard
gamepad evdev convention. Xbox via xpad, DualShock 4 via hid-sony,
DualSense via hid-playstation, Switch Pro via hid-nintendo, Steam
Controller via hid-steam, 8BitDo Pro 2 via xpad. Wired or USB dongle
only — no Bluetooth in v1.

---

## Setup — pick your distro

| Distro | Initrd | Guide |
|---|---|---|
| SteamOS (Steam Deck / Steam Machine) | dracut + systemd | [docs/install/steamos.md](docs/install/steamos.md) |
| Arch | mkinitcpio + `sd-encrypt` | [docs/install/arch.md](docs/install/arch.md) |
| NixOS | systemd stage 1 | [docs/install/nixos.md](docs/install/nixos.md) |
| Debian / Ubuntu | initramfs-tools | [docs/install/debian-ubuntu.md](docs/install/debian-ubuntu.md) |

Legacy non-systemd paths (mkinitcpio `encrypt` busybox, NixOS
scripted stage 1) are not supported. The installers detect this and
refuse with a clear message.

Before installing the initrd packaging on a real machine, work through
the lockout-safe verification ladder in [`TESTING.md`](TESTING.md).

---

## Quick smoke tests (any distro)

After building the binary (per the relevant install guide):

```sh
sudo luks-controller-unlock selftest      # DRM + controller + cryptsetup probe
sudo luks-controller-unlock test-input    # press buttons, see canonical events
sudo luks-controller-unlock test-ui       # render UI on a free VT, no cryptsetup
```

`test-ui` needs DRM master, so log in on a free VT (Ctrl-Alt-F2)
first. None of these touch LUKS data.

---

## What's in the repo

```
src/                  Rust source
  pin.rs              Canonical button alphabet + deterministic PIN encoding
  input/              evdev poll + per-controller mapping + B-hold backspace
  ui/drm.rs           DRM/KMS dumb buffer surface
  ui/render.rs        tiny-skia card layout + 5x7 embedded ASCII font
  agent.rs            systemd ask-password protocol over inotify + AF_UNIX
  keyscript.rs        initramfs-tools entry (stdout passphrase)
  enroll.rs           tcsetattr-based existing-pass prompt + cryptsetup luksAddKey
  selftest.rs         host-side install-time checks
dist/                 Per-distro packaging (dracut, mkinitcpio, nix, debian, systemd)
docs/install/         Per-distro setup guides
README.md             You are here
TESTING.md            Lockout-safe verification ladder
flake.nix             Nix devshell with pinned Rust toolchain + libdrm + cryptsetup
```

---

## License

MIT OR Apache-2.0
