# Setup — Debian / Ubuntu (initramfs-tools)

Debian and Ubuntu use initramfs-tools, which does **not** run systemd
in stage 1. The systemd ask-password protocol our agent normally uses
is therefore not available. Integration on this distro goes through
crypttab's `keyscript=` mechanism instead — the same hook point that
existing tools like `decrypt_keyctl` and `decrypt_derived` use.

## 0. Prerequisites and pre-flight

Read [_shared.md](_shared.md) and complete the build, header backup,
and enroll steps. The rest of this page assumes
`target/release/luks-controller-unlock` exists, you have a header
backup off-device, and the new PIN keyslot has been verified with
`cryptsetup luksOpen`.

## 1. Install the binary

```sh
sudo install -m 755 target/release/luks-controller-unlock /usr/bin/
```

## 2. Install the keyscript wrapper

```sh
sudo install -m 755 \
    dist/debian/keyscript.sh \
    /etc/initramfs-tools/luks-controller-keyscript
```

This is a tiny shell script that just exec's the binary's `keyscript`
subcommand. Initramfs-tools' `cryptroot` will run it during boot;
stdout becomes the LUKS passphrase fed to cryptsetup.

## 3. Install the initramfs-tools hook

```sh
sudo install -m 755 \
    dist/debian/hooks/luks-controller-unlock \
    /etc/initramfs-tools/hooks/luks-controller-unlock
```

The hook copies the binary, cryptsetup, and the keyscript into the
initrd, plus the DRM/KMS and HID kernel modules.

## 4. Wire `keyscript=` into `/etc/crypttab`

Edit `/etc/crypttab`. Find the entry for your LUKS root, add
`keyscript=/etc/initramfs-tools/luks-controller-keyscript` to the
options field:

Before:
```
cryptroot UUID=... none luks
```

After:
```
cryptroot UUID=... none luks,keyscript=/etc/initramfs-tools/luks-controller-keyscript
```

## 5. Allow keyscripts in the initrd

initramfs-tools strips keyscript support out of the initrd by default
unless you opt in:

```sh
echo 'CRYPTSETUP=y' | sudo tee /etc/initramfs-tools/conf.d/cryptsetup
echo 'KEYFILE_PATTERN=' | sudo tee -a /etc/initramfs-tools/conf.d/cryptsetup
```

(Some versions of cryptsetup-initramfs add this for you on
installation. If `update-initramfs` complains about a missing
keyscript, this is the fix.)

## 6. Regenerate the initrd

```sh
sudo update-initramfs -u
```

Look for `luks-controller-unlock` and `luks-controller-keyscript` in
the output. Verify with:

```sh
sudo lsinitramfs /boot/initrd.img-$(uname -r) | grep -E "luks-controller|keyscript"
```

## 7. Reboot test

Follow [TESTING.md rung 5](../../TESTING.md). On Debian/Ubuntu, the
keyscript path doesn't have a separate "console agent" so stage 5.3
(masking the console agent) doesn't apply. Keyboard fallback comes
from `cryptroot` reverting to its built-in tty prompt if our keyscript
exits non-zero.

## Debian/Ubuntu-specific notes

* **Multi-disk crypttab.** Each LUKS entry needs its own `keyscript=`
  if you want the controller to unlock it. Running the keyscript
  multiple times in a single boot is supported — the agent waits for
  a fresh PIN entry each time.
* **GPU modules.** The hook adds `amdgpu`, `i915`, `nouveau`,
  `radeon`. Edit the hook for other GPUs.
* **Ubuntu's `cryptsetup-initramfs` package.** Required for
  initramfs-tools to know about LUKS. Already installed if you set up
  encryption with the Ubuntu installer.
* **No Plymouth conflict.** initramfs-tools doesn't run Plymouth in
  the same way systemd-based initrds do, so you won't see the
  "Plymouth flash" issue. The screen does get cleared when our DRM
  surface comes up.
* **Wrong PIN behaviour.** On wrong PIN, our keyscript exits non-zero
  after `--max-attempts` empty PINs (default 3). cryptroot then
  retries via its own tty prompt — the keyboard fallback. To loop on
  the controller forever, set `--max-attempts` very high (edit the
  wrapper script).

## Reverting

```sh
# Remove keyscript= from /etc/crypttab
sudo $EDITOR /etc/crypttab

sudo rm /etc/initramfs-tools/hooks/luks-controller-unlock
sudo rm /etc/initramfs-tools/luks-controller-keyscript
sudo rm /usr/bin/luks-controller-unlock

sudo update-initramfs -u
```

The enrolled keyslot remains. Remove with `cryptsetup luksKillSlot`
(find the slot via `cryptsetup luksDump`). **Do not kill your
keyboard keyslot.**
