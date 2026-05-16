# Setup — NixOS

> **Status:** boots cleanly on a Steam Deck OLED with NixOS +
> Jovian-NixOS + impermanence root + `linux-*-valve1` kernel. Other
> NixOS configurations (regular laptop, desktop) follow the same
> module wiring but haven't been bench-tested. Generations are your
> friend — do not garbage-collect the previous one until the new one
> has booted cleanly several times.

systemd stage 1 initrd. Agent path uses the systemd ask-password
protocol.

> Requires `boot.initrd.systemd.enable = true`. The legacy scripted
> stage 1 does not implement the ask-password protocol the agent uses.
> The NixOS module asserts this — `nixos-rebuild` will refuse to
> evaluate without it.

## Gotchas surfaced during Steam Deck deployment

These bit us hard during the first end-to-end deployment. If you hit
the same symptoms on another NixOS host, jump straight to the fix:

1. **TPM2 + systemd 258.7 segfault.** If `boot.initrd.luks.devices.<name>.crypttabExtraOpts`
   contains `tpm2-device=auto`, systemd-cryptsetup may segfault during
   the TPM unlock attempt — happens after "Successfully created
   primary key on TPM" and *before* the keyfile/ask-password fallback,
   so the agent never receives a request. Workaround: comment out
   `tpm2-device=auto` until systemd is patched.

2. **Impermanence + journal loss.** Default `services.journald` writes
   to `/var/log/journal/` only if the directory exists. On a fresh
   impermanence root where `/persistence/var/log` is empty, journald
   silently falls back to volatile `/run/log/journal` and you lose
   every diagnostic on reboot. Set:
   ```nix
   services.journald.extraConfig = ''
     Storage=persistent
     SystemMaxUse=200M
   '';
   ```

3. **Mask the stock console agent only after the controller agent is
   proven.** With `maskConsoleAgent = true` and no fallback, an agent
   crash means *no* unlock path at all. Set it to `false` until you
   have a week of clean reboots; once stable, flip on to suppress the
   duplicate kernel-tty prompt.

4. **Enroll environment must match unlock environment for the same
   physical buttons to produce the same encoded chars.** Hid-steam
   reports physical Y as `BTN_WEST` and physical X as `BTN_NORTH` —
   opposite the Xbox-style convention. That's universal across stock
   and Valve kernels, so enroll-on-installer + unlock-in-initrd both
   produce the same encoded form. Do **not** introduce per-driver
   "swap" logic — it breaks the consistency.

5. **The Deck's panel is portrait-native (800x1280), displayed
   landscape.** The agent's `DrmSurface` auto-detects this (connector
   type == `EmbeddedDisplayPort` + height > width) and rotates the
   render 90° CW. Other embedded panels with this geometry will get
   the same treatment.

6. **`boot.initrd.systemd.storePaths` does not auto-follow ExecStart
   closure.** If you point a unit at a `writeShellScript` (e.g. for
   debug logging), you must add both the script and every tool it
   exec's to `storePaths`, or the unit fails 203/EXEC in the initrd.

## 0. Prerequisites and pre-flight

Read [_shared.md](_shared.md) and complete the build, header backup,
and enroll steps. The rest of this page assumes:

* You have a checkout of this repo somewhere on the host (or a flake
  input pointing at it).
* The CLI binary, built once, has been used to enroll a PIN keyslot
  on `/dev/sdaX` and you've verified it with `cryptsetup luksOpen`.
* You have a LUKS header backup off-device.

## 1. Add the module to your NixOS configuration

Two options. Pick one.

### Option A: import the path directly

If your config lives next to a checkout of this repo:

```nix
{ config, pkgs, ... }: {
  imports = [
    /path/to/luks-controller-unlock/dist/nix/module.nix
  ];

  boot.initrd.systemd.enable = true;

  boot.initrd.luks-controller-unlock = {
    enable = true;
    package = pkgs.callPackage /path/to/luks-controller-unlock/dist/nix/default.nix {};
    # Optional: mask the stock console password agent so the controller
    # UI is the only thing on screen. Keyboard passphrase still works
    # — systemd-cryptsetup reads our reply on the same socket — but
    # the duplicate prompt is gone.
    maskConsoleAgent = false;
  };
}
```

### Option B: as a flake input

In your system flake's `inputs`:

```nix
{
  inputs.luks-controller-unlock.url = "github:<you>/luks-controller-unlock";
  outputs = { self, nixpkgs, luks-controller-unlock, ... }: {
    nixosConfigurations.htpc = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        luks-controller-unlock.dist.nix.module    # if exposed
        ({ pkgs, ... }: {
          boot.initrd.systemd.enable = true;
          boot.initrd.luks-controller-unlock = {
            enable = true;
            package = pkgs.callPackage
              "${luks-controller-unlock}/dist/nix/default.nix" {};
          };
        })
      ];
    };
  };
}
```

(The repo's own `flake.nix` exposes a devshell only; expose the
module + package as flake outputs for cleaner consumption — open issue
or PR.)

## 2. Confirm the LUKS device is in `boot.initrd.luks.devices`

The agent only helps if `systemd-cryptsetup` actually opens the device
in stage 1. Make sure your config has something like:

```nix
boot.initrd.luks.devices.cryptroot = {
  device = "/dev/disk/by-uuid/<uuid>";
  preLVM = true;
};
```

`nixos-generate-config` writes this for you on a fresh install.

## 3. Rebuild

```sh
sudo nixos-rebuild switch
```

`nixos-rebuild` will fail with the asserted error if
`boot.initrd.systemd.enable = true` is missing. Add it and retry.

## 4. Reboot test

Follow [TESTING.md rung 5](../../TESTING.md). Stage 5.1 (enroll
only) was already done before this page; stage 5.2 corresponds to
the rebuild above; stage 5.3 corresponds to setting
`maskConsoleAgent = true`.

## NixOS-specific notes

* **Generations as a safety net.** Failed boots are recoverable from
  the bootloader's previous generation. Don't delete generations
  faster than you boot-test new ones.
* **Module location.** Importing from a path in `/etc/nixos` or
  similar is fine for a single host. For multiple hosts, prefer the
  flake input route — it's reproducible and pins a commit.
* **Kernel modules.** The module adds `amdgpu`, `i915`, `nouveau`,
  `radeon`, `xpad`, `hid_sony`, `hid_playstation`, `hid_nintendo`,
  `hid_steam` to `boot.initrd.kernelModules`. Override the list via
  `boot.initrd.luks-controller-unlock.extraKernelModules`.
* **Build the package once, locally.** The `default.nix` uses
  `rustPlatform.buildRustPackage` with `cargoLock.lockFile`, so a
  full source build runs the first time. Subsequent rebuilds are
  cached.
* **Specialisations.** If you want to be extra safe, put the
  `luks-controller-unlock.enable = true;` in a NixOS specialisation
  rather than the default profile — the bootloader gets a sibling
  entry, and the default entry stays controller-free as a fallback.

## Reverting

Either:

```nix
boot.initrd.luks-controller-unlock.enable = false;
```

`nixos-rebuild switch && reboot`. The enrolled keyslot remains; remove
with `cryptsetup luksKillSlot` (find the slot via
`cryptsetup luksDump`). **Do not kill your keyboard keyslot.**

Or roll back to the previous generation from the bootloader if the
new one won't boot.
