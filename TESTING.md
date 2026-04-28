# Testing Procedure

How to verify `luks-controller-unlock` end-to-end without locking yourself out
of an encrypted system. Climb one rung at a time. If a rung fails, stop and
fix before moving up.

The ladder is designed so the first three rungs touch nothing on disk that
matters and the fourth uses a throwaway VM. Bare metal only after the VM
cycle works repeatedly.

---

## Lockout-impossible invariants

Hold these for the entire ladder. Break any of them and you can brick the
boot.

| Rule | Why |
|---|---|
| Keep an unaltered keyboard-passphrase LUKS keyslot at all times. | Survives any agent bug. |
| Never run `cryptsetup luksKillSlot` on the keyboard keyslot. | One typo and you're locked out. |
| Do **not** mask `systemd-ask-password-console.service` until rung 5 stage 3. | The console agent is the built-in keyboard fallback. |
| Keep a LUKS header backup off the encrypted device. | Survives a bricked header. |
| Keep a live USB with `cryptsetup` available. | The recovery path. |

### Pre-flight (do this once, before rung 1)

```sh
# Header backup — restorable on disaster.
sudo cryptsetup luksHeaderBackup /dev/sdaX \
    --header-backup-file ~/luks-header-$(date +%F).img

# Confirm at least one keyboard passphrase keyslot exists.
sudo cryptsetup luksDump /dev/sdaX | grep -E "^Keyslots:|^  [0-9]+:"

# Sanity: live USB plugged in (or burned and verified) with a recent
# cryptsetup. Test boot it once if you've never done so.
```

Header backup file goes on a USB stick or another disk — **not** the
encrypted device it backs up.

---

## Rung 1 — host-side unit and dry checks (zero risk)

Touches no LUKS data. Runs in the dev shell.

```sh
cd luks-controller-unlock
nix develop --command cargo test                # 19 tests must pass
nix develop --command cargo run -- selftest     # DRM + controller + cryptsetup probes
nix develop --command cargo run -- test-input   # press buttons, see canonical events
```

`test-ui` needs DRM master, which conflicts with a running X/Wayland
session. Switch to a free VT (Ctrl-Alt-F2), log in, then:

```sh
sudo target/release/luks-controller-unlock test-ui --seconds 10
```

Pass criteria:
- 19/19 unit tests green.
- `selftest` reports `pass:` for DRM card and (if controller plugged) controller; reports `fail:` only for things you don't have.
- `test-input` prints one event per press; `B` released within 500 ms shows as `Press(B)`; held longer shows as `Backspace`; `START` shows as `Submit`.
- `test-ui` paints the centered card; pressing buttons updates the dot row; releasing erases dots; `START` exits cleanly and the console returns.

Fail mode and recovery:
- Test failure → fix code, no risk.
- DRM grab fails on free VT → another agent owns the card. Stop X (`systemctl isolate multi-user.target`) and retry.
- No controller detected → see Troubleshooting in `README.md`. Don't proceed.

---

## Rung 2 — throwaway LUKS image (zero risk to real data)

Loop-mount a sparse file, format it, enroll on it, verify the derived
passphrase actually unlocks.

```sh
truncate -s 100M /tmp/test.img
echo -n 'host-existing-pass' | \
    sudo cryptsetup luksFormat /tmp/test.img --batch-mode --key-file=-
LOOP=$(sudo losetup -fP --show /tmp/test.img)
echo "loop: $LOOP"

# Enroll. Use a short PIN for reproducibility, e.g. A B X Y.
# Existing passphrase prompt → type host-existing-pass.
# Two PIN passes → press A, B, X, Y, START on the controller (×2).
sudo target/release/luks-controller-unlock enroll --device "$LOOP"
```

Verify the keyslot was actually added and that the deterministic encoding
matches what `pin.rs` produces:

```sh
sudo cryptsetup luksDump "$LOOP" | grep -E "Keyslots:|^  [0-9]+:"
# A B X Y → ASCII 'a' 'b' 'c' 'd' (see CanonicalButton enum).
echo -n 'abcd' | sudo cryptsetup luksOpen "$LOOP" testvol --key-file=-
sudo cryptsetup close testvol

sudo losetup -d "$LOOP"
rm /tmp/test.img
```

Pass criteria:
- `luksDump` shows one more keyslot than before enrollment.
- `luksOpen` with the manually-derived passphrase succeeds with no prompt.
- `luksClose` succeeds.

Fail mode and recovery:
- Enrollment exits non-zero → cryptsetup invocation in `enroll.rs` is wrong. Fix and retry on the same loop image. **Do not** enroll on a real device.
- `luksOpen` rejects the derived passphrase → the encoding in `pin.rs` does not match what `enroll.rs` writes to cryptsetup. Critical bug — fix before proceeding.

Canonical button → ASCII byte (from `src/pin.rs`):

```
A=a   B=b   X=c   Y=d
LB=e  RB=f  LT=g  RT=h
DpadN=i  DpadS=j  DpadE=k  DpadW=l
```

---

## Rung 3 — agent dry-run with a fake ask file (zero risk)

Tests the systemd ask-password protocol without involving cryptsetup.

```sh
mkdir -p /tmp/askpw
# Receiver socket — `nc` will print the agent's reply.
nc -lU /tmp/askpw/sock.fake &
LISTENER=$!

cat > /tmp/askpw/ask.test <<'EOF'
[Ask]
PID=1
Socket=/tmp/askpw/sock.fake
Echo=0
NotAfter=0
Message=DRY RUN — press buttons then START
Id=test:dryrun
EOF

# Free VT required.
sudo target/release/luks-controller-unlock agent \
    --watch-dir /tmp/askpw --card /dev/dri/card0
```

Press a known PIN, START. The `nc` listener prints `+abcd...` (the leading
`+` is the success marker; remaining bytes are the encoded PIN).

After the reply, manually delete the ask file to signal "request handled":

```sh
rm /tmp/askpw/ask.test
# Agent should release the UI and return to inotify-wait state.
```

Cleanup:

```sh
kill $LISTENER 2>/dev/null
sudo rm -rf /tmp/askpw
```

Pass criteria:
- Reply byte string starts with `+`.
- Remaining bytes match the canonical encoding of the PIN you pressed.
- Agent releases DRM master after the reply (console returns).
- Removing the ask file makes the agent stop drawing.

Fail mode and recovery:
- No bytes received on `nc` → socket protocol bug. Stop, fix `send_reply()` in `agent.rs`. No data risk.
- Agent crashes → no data risk; check stderr.
- DRM master not released → kill the agent (Ctrl-C). Console will come back.

---

## Rung 4 — VM boot test (zero risk to host)

Install a target distro in a VM with LUKS root. Pass through your
controller via QEMU's evdev passthrough. Snapshot before installing the
agent so failed boots are recoverable in seconds.

```sh
# QEMU example. Adjust event device + drive paths.
qemu-system-x86_64 \
    -enable-kvm -m 4G -cpu host \
    -drive file=arch.qcow2,if=virtio \
    -device virtio-input-host-pci,evdev=/dev/input/event10 \
    -display gtk,gl=on
```

Inside the VM:

1. Enroll the PIN on the VM's LUKS root: `luks-controller-unlock enroll --device /dev/vda3`. Verify with `cryptsetup luksOpen` exactly as in rung 2.
2. **Snapshot the VM disk before installing initrd packaging.**
3. Install the dist/ packaging for the target distro per the README.
4. Reboot. Verify the controller PIN unlocks.
5. Reboot. Verify the keyboard passphrase still unlocks (other keyslot).
6. Reboot. Enter wrong PIN. Verify the lockout backoff (1s, 2s, 4s, …, 30s cap). Then enter correct PIN — must unlock.
7. Reboot. Power off mid-prompt. Reboot again — must still unlock with keyboard.

If any step fails, restore the snapshot, fix, retry. **Do not climb to
bare metal until 1–7 succeed cleanly twice in a row.**

Recommended VM matrix (one per supported initrd):
- Arch with `sd-encrypt` HOOKS.
- Debian 12 with default initramfs-tools.
- NixOS with `boot.initrd.systemd.enable = true`.

SteamOS in a VM is awkward; defer SteamOS testing to bare-metal Steam Deck
with extra caution.

---

## Rung 5 — bare metal, incremental

Three stages. Reboot between each.

### Stage 5.1 — enroll only

No initrd packaging installed yet. Just the `enroll` subcommand on the
host's real LUKS root.

```sh
sudo cryptsetup luksDump /dev/sdaX | grep -E "^  [0-9]+:" | wc -l    # before
sudo target/release/luks-controller-unlock enroll --device /dev/sdaX
sudo cryptsetup luksDump /dev/sdaX | grep -E "^  [0-9]+:" | wc -l    # after, +1
sudo reboot
```

Boot must succeed with the existing keyboard passphrase. If anything
fails to boot, the LUKS header is the suspect — restore from
`~/luks-header-YYYY-MM-DD.img`:

```sh
# From live USB
cryptsetup luksHeaderRestore /dev/sdaX --header-backup-file /path/to/header.img
```

### Stage 5.2 — install agent, keep console fallback

Install the per-distro packaging from `dist/` per the README. Do **not**
deploy `dist/systemd/systemd-ask-password-console.service.d/`. Both prompts
on screen at boot is the safe configuration: if the agent silently fails,
the keyboard prompt is right there.

Reboot. Verify:
- Controller PIN unlocks.
- Keyboard passphrase unlocks (boot once with controller unplugged).
- A wrong PIN followed by a correct PIN unlocks.

Repeat reboot at least three times. If any boot fails, remove the agent
unit and reinstall the initrd before rebooting again:
- Arch: remove `luks-controller-unlock` from HOOKS, `mkinitcpio -P`.
- Debian: remove the hook from `/etc/initramfs-tools/hooks/`, `update-initramfs -u`.
- NixOS: set `boot.initrd.luks-controller-unlock.enable = false;`, `nixos-rebuild switch`.
- SteamOS/dracut: remove `/usr/lib/dracut/modules.d/90luks-controller-unlock`, `dracut --force`.

### Stage 5.3 — optional: mask the console agent

Only after stage 5.2 passes a week of daily-driver use without a single
boot failure. This removes the keyboard fallback prompt from the screen
(keyboard passphrases still work — the console agent reply path is still
wired through systemd-cryptsetup, just no second prompt on display).

Apply the drop-in from
`dist/systemd/systemd-ask-password-console.service.d/`. Rebuild initrd.
Reboot.

If anything fails, this is reversible: delete the drop-in, rebuild
initrd. Use the keyboard fallback (it still works, just silently —
type your passphrase blind and press Enter).

---

## Recovery checklist

If you can't unlock at boot:

1. Boot from live USB.
2. `cryptsetup luksDump /dev/sdaX` — confirm the device is still LUKS.
3. If keyslots look intact: `cryptsetup luksOpen /dev/sdaX recover` and use the keyboard passphrase. Mount, fix the initrd, reboot.
4. If the header is damaged:
   ```sh
   cryptsetup luksHeaderRestore /dev/sdaX \
       --header-backup-file /path/to/luks-header-YYYY-MM-DD.img
   ```
5. Boot again. Use the keyboard passphrase.

---

## Per-rung pass/fail matrix

| Rung | Touches LUKS? | Bricks risk | Time |
|---|---|---|---|
| 1 — host unit + dry | No | None | < 5 min |
| 2 — loop image | No (only loop file) | None | < 5 min |
| 3 — fake ask file | No | None | < 5 min |
| 4 — VM | VM disk only | VM only — snapshot | 30 min – 2 h |
| 5.1 — enroll on real disk | Yes (adds slot) | Header damage only | 10 min |
| 5.2 — agent installed, no mask | Yes (initrd) | Boot failure → keyboard fallback works | 30 min |
| 5.3 — mask console agent | Yes (initrd) | Boot failure → blind keyboard typing works | 5 min |
