# Setup — Arch Linux (mkinitcpio)

> ⚠️ **Untested on Arch.** This tool is new. The procedure below is
> the *intended* path on Arch with `sd-encrypt`; it has not yet been
> end-to-end booted by the author. Treat it as a recipe to verify,
> not a known-good playbook. Climb [`TESTING.md`](../../TESTING.md)
> before installing anything in initrd and keep your keyboard keyslot
> intact as the recovery path.

mkinitcpio + `sd-encrypt` initrd. Agent path uses the systemd
ask-password protocol.

> The legacy busybox `encrypt` hook is **not** supported. The install
> hook detects this and refuses with a clear message. If you're on
> `encrypt`, switch to `sd-encrypt` first (separate exercise — Arch
> wiki has a thorough page).

## 0. Prerequisites and pre-flight

Read [_shared.md](_shared.md) and complete the build, header backup,
and enroll steps. The rest of this page assumes
`target/release/luks-controller-unlock` exists, you have a header
backup off-device, and the new PIN keyslot has been verified with
`cryptsetup luksOpen`.

## 1. Confirm `sd-encrypt` is in HOOKS

```sh
grep ^HOOKS= /etc/mkinitcpio.conf
```

You want a line like:

```
HOOKS=(base systemd autodetect modconf kms keyboard sd-vconsole block sd-encrypt filesystems fsck)
```

If you see `encrypt` instead of `sd-encrypt`, fix that first and
regenerate the initrd before continuing.

## 2. Install the binary

```sh
sudo install -m 755 target/release/luks-controller-unlock /usr/bin/
```

Optionally, install via a PKGBUILD if you want pacman to track it.
Not required.

## 3. Install the systemd unit

```sh
sudo install -m 644 \
    dist/systemd/luks-controller-unlock.service \
    /usr/lib/systemd/system/luks-controller-unlock.service
```

## 4. Install the mkinitcpio install hook

```sh
sudo install -m 755 \
    dist/mkinitcpio/install/luks-controller-unlock \
    /etc/initcpio/install/luks-controller-unlock

sudo install -m 755 \
    dist/mkinitcpio/hooks/luks-controller-unlock \
    /etc/initcpio/hooks/luks-controller-unlock
```

## 5. Add the hook to HOOKS

Edit `/etc/mkinitcpio.conf` and add `luks-controller-unlock` *after*
`sd-encrypt`:

```
HOOKS=(base systemd autodetect modconf kms keyboard sd-vconsole block sd-encrypt luks-controller-unlock filesystems fsck)
```

## 6. (Optional) mask the console agent

By default the stock keyboard prompt is also drawn at boot. Both
prompts on screen is the safe configuration — keyboard fallback is
visible. To suppress the duplicate console prompt once you've
boot-tested the controller path:

```sh
sudo mkdir -p /etc/luks-controller-unlock
sudo cp \
    dist/systemd/systemd-ask-password-console.service.d/10-mask-when-controller.conf \
    /etc/luks-controller-unlock/mask-console-agent
```

The install hook bundles this drop-in only when that file exists.

Don't do step 6 until you've done a clean reboot test in step 8.

## 7. Regenerate the initrd

```sh
sudo mkinitcpio -P
```

Verify the agent is bundled:

```sh
sudo lsinitcpio /boot/initramfs-linux.img | grep luks-controller-unlock
```

You should see at least the binary and the systemd unit symlink.

## 8. Reboot test

Follow [TESTING.md rung 5](../../TESTING.md) — enroll-only first
(skip steps 2–7 above for that), then agent-with-keyboard-fallback,
then optional console-agent mask.

## Arch-specific notes

* **Kernel updates trigger initrd regeneration.** A `pacman -S linux`
  re-runs `mkinitcpio`, so the agent stays bundled across kernel
  updates as long as the install hook is in place.
* **Multi-kernel setups.** `mkinitcpio -P` regenerates every preset.
  If you use linux-lts or linux-zen too, all of them get the agent.
* **GPU modules.** The install hook adds `amdgpu`, `i915`, `nouveau`,
  `radeon`. If you have a different GPU (older Nvidia closed driver,
  some ARM SBCs), edit the install hook's `add_module` lines.
* **Plymouth.** If you use Plymouth, the agent's
  `Conflicts=plymouth-start.service` causes Plymouth to be stopped
  when the agent starts. You'll see a brief flash. Acceptable for v1.

## Reverting

```sh
# Remove the hook from HOOKS in /etc/mkinitcpio.conf
sudo $EDITOR /etc/mkinitcpio.conf
sudo rm /etc/initcpio/install/luks-controller-unlock
sudo rm /etc/initcpio/hooks/luks-controller-unlock
sudo rm /usr/lib/systemd/system/luks-controller-unlock.service
sudo rm /usr/bin/luks-controller-unlock
sudo rm -rf /etc/luks-controller-unlock
sudo mkinitcpio -P
```

The enrolled keyslot remains. To remove it: find the slot with
`cryptsetup luksDump`, run `cryptsetup luksKillSlot /dev/sdaX N`.
**Do not kill your keyboard keyslot.**
