# luks-controller-unlock

Unlock LUKS2 root filesystems with a game controller. For living-room PCs,
Steam Decks, and Steam Machines where typing a passphrase on a TV with no
keyboard is awful.

## What it is

Two phases:

1. **Enroll** (booted system, CLI): you press a sequence of controller
   buttons to define a PIN. The PIN is encoded deterministically to an
   ASCII passphrase and registered as a normal LUKS2 keyslot via
   `cryptsetup luksAddKey`.
2. **Unlock** (initrd, every boot): the agent draws a fullscreen UI on
   the GPU, watches your controller, and replies to systemd's
   ask-password (or initramfs-tools' `keyscript=`) with the encoded
   passphrase. Standard keyboard passphrase entry continues to work for
   any other keyslot you've enrolled.

The PIN alphabet is the 12 buttons present on every supported controller:
A, B, X, Y, LB, RB, LT, RT, Dpad N/S/E/W. **START** submits.
**B held ≥ 500 ms** is backspace (a B-tap is the literal `B` symbol). PIN
length is variable with no upper bound (capped at 256 buttons).

## Status

Greenfield project. The compile passes, all unit tests pass, and the
agent / enroll / keyscript paths are wired end-to-end. **It has not yet
been booted in anger on every target distro** — see "Verifying" below.

Single-controller, no Bluetooth, no hot-plug for v1. See `BLOCKERS.md`
(once it exists) for a list of follow-ups.

## Supported targets

| Distro | Initrd | Integration |
|---|---|---|
| SteamOS (Steam Deck/Steam Machine) | dracut + systemd | dracut module → systemd ask-password agent |
| Arch | mkinitcpio + `sd-encrypt` | mkinitcpio install hook → systemd ask-password agent |
| NixOS | systemd stage 1 | NixOS module → systemd ask-password agent |
| Debian / Ubuntu | initramfs-tools (no systemd) | crypttab `keyscript=` wrapper |

The legacy non-systemd paths (mkinitcpio `encrypt` busybox, NixOS
scripted stage 1) are **not** supported. The install hooks detect this
and refuse with a clear message.

## Building

The repo ships a Nix flake with a development shell that pulls in a
pinned Rust toolchain, gcc, libdrm, cryptsetup, and the helper crates.

```sh
nix develop
cargo build --release
```

Without nix: any recent stable Rust toolchain (>= 1.75), gcc/clang, and
`pkg-config` headers for libdrm. Run `cargo build --release`.

The output binary is at `target/release/luks-controller-unlock`. For
initrd use, build with the musl target:

```sh
cargo build --release --target x86_64-unknown-linux-musl
```

## Enrolling

Enroll runs from a regular shell. You'll need an existing LUKS2
passphrase to add the new keyslot.

```sh
sudo luks-controller-unlock enroll --device /dev/sda3
```

It prompts for the existing passphrase on the tty (echo disabled),
opens the first connected gamepad, and asks you to enter the PIN twice
(unless you pass `--no-confirm`). The PIN is shown only as a row of
dots on stderr — the buttons themselves never echo.

`cryptsetup luksAddKey` is invoked with `--pbkdf-memory 262144` (256
MiB) so the Argon2 verification at boot stays well below initrd OOM
thresholds. Override with `--pbkdf-memory-kib` if you've measured
something specific to your hardware.

## Installing the unlock path per distro

### SteamOS (dracut)

```sh
sudo cp -r dist/dracut/90luks-controller-unlock /usr/lib/dracut/modules.d/
sudo cp target/release/luks-controller-unlock /usr/bin/
sudo dracut --force
```

The Steam Deck's `/usr` is read-only on SteamOS A/B. Install to the
read-write `/etc` overlay or repeat after a system update. Re-enroll
is **not** required after an update — only the binary and the dracut
module need to be reinstalled.

### Arch (mkinitcpio)

```sh
sudo cp dist/mkinitcpio/install/luks-controller-unlock /etc/initcpio/install/
sudo cp dist/mkinitcpio/hooks/luks-controller-unlock /etc/initcpio/hooks/
sudo cp target/release/luks-controller-unlock /usr/bin/
sudo cp dist/systemd/luks-controller-unlock.service /usr/lib/systemd/system/
```

Edit `/etc/mkinitcpio.conf` and ensure your `HOOKS=` line uses
`sd-encrypt` (not the legacy `encrypt`) and add `luks-controller-unlock`
after it:

```
HOOKS=(base systemd autodetect modconf kms keyboard sd-vconsole block sd-encrypt luks-controller-unlock filesystems fsck)
```

Then regenerate:

```sh
sudo mkinitcpio -P
```

### NixOS

Add the module to your configuration:

```nix
{ config, pkgs, ... }: {
  imports = [ ./path/to/dist/nix/module.nix ];
  boot.initrd.systemd.enable = true;
  boot.initrd.luks-controller-unlock = {
    enable = true;
    package = pkgs.callPackage ./path/to/dist/nix/default.nix {};
  };
}
```

Then `sudo nixos-rebuild switch`.

### Debian / Ubuntu

initramfs-tools doesn't speak the systemd ask-password protocol, so on
this distro we hook in via crypttab's `keyscript=` mechanism instead.

```sh
sudo cp dist/debian/hooks/luks-controller-unlock /etc/initramfs-tools/hooks/
sudo chmod +x /etc/initramfs-tools/hooks/luks-controller-unlock
sudo cp dist/debian/keyscript.sh /etc/initramfs-tools/luks-controller-keyscript
sudo chmod +x /etc/initramfs-tools/luks-controller-keyscript
sudo cp target/release/luks-controller-unlock /usr/bin/
```

In `/etc/crypttab`, add `keyscript=` to the relevant entry:

```
cryptroot UUID=… none luks,keyscript=/etc/initramfs-tools/luks-controller-keyscript
```

Then regenerate:

```sh
sudo update-initramfs -u
```

## Recommended kernel cmdline

The boot console will draw `printk` messages directly into our
framebuffer unless suppressed. Add `quiet loglevel=3` to your kernel
cmdline so the unlock UI isn't covered by boot text. (A future version
will switch the tty to `KD_GRAPHICS` mode automatically.)

## Verifying

For the full lockout-safe verification ladder (host dry checks → loop
image → fake ask file → VM → bare metal stages), see
[`TESTING.md`](TESTING.md). **Do not enroll on a real disk before
working through rungs 1–4.**

Quick smoke tests:

```sh
sudo luks-controller-unlock selftest
```

Checks DRM card, controller, cryptsetup version, hid-steam availability.

```sh
# From a free VT (Ctrl-Alt-F2) so we can grab DRM master.
sudo luks-controller-unlock test-ui
```

Renders the unlock UI; push buttons to see the dot row update. START exits.

```sh
sudo luks-controller-unlock test-input
```

Prints canonical button events as you press them — useful when adding
support for an unfamiliar controller.

## Troubleshooting

* **"no controller detected"**: the tool only matches devices that
  advertise `BTN_SOUTH`. Some cheap USB receivers expose the controller
  as a HID joystick instead. Re-pair through the kernel's xpad driver
  (Xbox-compatible mode) or run `test-input` to see what's enumerated.
* **"no DRM card with a connected output"**: the tool grabs the first
  connected connector. If you're behind a DisplayPort hub or KVM that
  doesn't report connected status until the OS handshakes, plug the
  display in directly.
* **Steam Controller doesn't work**: requires the `hid-steam` kernel
  module loaded *in the initrd*. The dist/ packaging includes it; if
  you customised your initrd, double-check.
* **Two prompts on screen at boot**: the stock console password agent
  and ours both fire. Either ignore it (keyboard fallback works either
  way) or mask the console agent with the drop-in at
  `dist/systemd/systemd-ask-password-console.service.d/`.
* **Plymouth flashes briefly**: expected. Our agent declares
  `Conflicts=plymouth-start.service` so plymouth releases DRM master
  before we draw. Smoothing this is a follow-up.

## License

MIT OR Apache-2.0
