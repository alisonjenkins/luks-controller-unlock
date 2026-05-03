# Setup — SteamOS (Steam Deck / Steam Machine)

> ⚠️ **Untested on SteamOS.** This tool is new. The procedure below
> is the *intended* path on SteamOS; it has not yet been end-to-end
> booted on a Deck or Steam Machine by the author. Treat it as a
> recipe to verify, not a known-good playbook. SteamOS in a VM is
> awkward, so verification means bare-metal — read
> [`TESTING.md`](../../TESTING.md) before installing initrd packaging
> and keep the keyboard keyslot intact as the recovery path.

dracut + systemd initrd. Agent path uses the systemd ask-password
protocol; no `keyscript=` involved.

> SteamOS is A/B atomic: `/usr` is read-only and reverts on update.
> Installing under `/usr` works for the current boot but is wiped by
> the next OS update. Plan for re-install (or a writable overlay).

## 0. Prerequisites and pre-flight

Read [_shared.md](_shared.md) for the prerequisites, build steps, LUKS
header backup, and enroll procedure. Do those first. The rest of this
page assumes you have a `target/release/luks-controller-unlock` binary,
a header backup off-device, and a freshly-enrolled controller PIN
keyslot you've verified with `cryptsetup luksOpen`.

## 1. Disable rootfs read-only protection (Steam Deck)

```sh
sudo steamos-readonly disable
```

Re-enable later with `sudo steamos-readonly enable`. While disabled,
package operations still work but A/B atomicity is broken — keep the
window short.

## 2. Install the binary and dracut module

```sh
sudo install -m 755 target/release/luks-controller-unlock /usr/bin/

sudo cp -r dist/dracut/90luks-controller-unlock \
    /usr/lib/dracut/modules.d/

sudo install -m 644 \
    dist/systemd/luks-controller-unlock.service \
    /usr/lib/systemd/system/luks-controller-unlock.service
```

## 3. Regenerate the initrd

```sh
sudo dracut --force
```

Verify the agent ended up inside:

```sh
sudo lsinitrd | grep luks-controller-unlock
```

## 4. Re-enable rootfs read-only

```sh
sudo steamos-readonly enable
```

## 5. Reboot test

Work through [TESTING.md rung 5](../../TESTING.md). On the Deck the
internal controller is `event*` directly — no extra config. External
USB controllers also work via xpad / hid-sony / hid-playstation /
hid-nintendo, all of which the dracut module pulls into initrd.

## SteamOS-specific notes

* **Surviving updates.** Each SteamOS update wipes `/usr` and resets
  the initrd. After every update, run steps 1–4 again. The enrolled
  LUKS keyslot survives — only the in-initrd binary needs reinstalling.
* **Recovery boot.** SteamOS's recovery image does not include the
  agent. If you boot recovery, use the keyboard passphrase keyslot
  you preserved. Keep that slot.
* **Built-in controller vs. desktop mode.** The Deck's controller is
  always available in initrd. Steam Input / xboxdrv overrides only
  apply once the desktop session starts.
* **Non-Deck Steam Machines.** Same dracut module. Confirm your GPU
  driver is in the initrd (`amdgpu`, `i915`, `nouveau`, `radeon` are
  added by the module) and that the controller dongle/cable is in a
  port the kernel sees during `udev-trigger`.

## Reverting

```sh
sudo steamos-readonly disable
sudo rm -rf /usr/lib/dracut/modules.d/90luks-controller-unlock
sudo rm /usr/lib/systemd/system/luks-controller-unlock.service
sudo rm /usr/bin/luks-controller-unlock
sudo dracut --force
sudo steamos-readonly enable
```

The enrolled keyslot remains. To remove it, find the slot number with
`cryptsetup luksDump` and run `cryptsetup luksKillSlot /dev/sdaX N`.
**Do not kill your keyboard keyslot.**
