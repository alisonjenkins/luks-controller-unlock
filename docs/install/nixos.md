# Setup — NixOS

systemd stage 1 initrd. Agent path uses the systemd ask-password
protocol.

> Requires `boot.initrd.systemd.enable = true`. The legacy scripted
> stage 1 does not implement the ask-password protocol the agent uses.
> The NixOS module asserts this — `nixos-rebuild` will refuse to
> evaluate without it.

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
