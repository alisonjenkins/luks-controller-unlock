# luks-controller-unlock

Unlock LUKS2 root filesystems with a game controller. For living-room PCs,
Steam Decks, and Steam Machines.

> Status: greenfield. Compiles, 19 unit tests pass, all entry points
> wired end-to-end. Not yet booted on every supported distro. Work
> through [`TESTING.md`](TESTING.md) before enrolling on a real disk.

---

## Why

Living-room machines are HTPCs. They sit in a media unit, plugged into
a TV. The keyboard either lives in a drawer or doesn't exist at all.
Asking a guest — or your future tired self at 11pm — to fish out a USB
keyboard, plug it in, type a 30-character LUKS passphrase blind into
the boot prompt, then put the keyboard away again, is the friction
that pushes people to one of two bad choices:

1. Run the box unencrypted because "it's just media".
2. Use a 6-character passphrase because anything longer is unusable
   on a TV.

Neither is good. The Steam Deck, Steam Machines, and any HTPC ship
with a perfectly capable input device already paired and within reach:
the controller. This tool lets you use it as the unlock device while
keeping a real keyboard passphrase as a fallback for any other
keyslot.

Threat model: defends against opportunistic theft and casual access by
a houseguest. Brute-force resistance comes from LUKS2's built-in
Argon2id KDF (the same KDF that protects keyboard passphrases) so a
PIN with a few buttons of entropy is meaningfully expensive to attack
offline. It's not a substitute for a TPM-sealed key on hardware that
has one.

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
