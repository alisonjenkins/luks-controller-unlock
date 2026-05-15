{ config, lib, ... }:

let
  cfg = config.boot.initrd.luks-controller-unlock;
in
{
  options.boot.initrd.luks-controller-unlock = {
    enable = lib.mkEnableOption "controller-driven LUKS unlock agent in stage 1";

    package = lib.mkOption {
      type = lib.types.package;
      description = "luks-controller-unlock package to install into the initrd.";
    };

    maskConsoleAgent = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Mask systemd-ask-password-console.service in the initrd so the
        controller UI is the only thing on screen. Keyboard passphrase
        entry continues to work because systemd-cryptsetup itself reads
        the agent's reply on the AF_UNIX socket — but this turns off
        the duplicate kernel-tty prompt.
      '';
    };

    extraKernelModules = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        # GPU drivers that commonly handle the boot console.
        "amdgpu" "i915" "nouveau" "radeon"
        # Input + vendor HID. evdev is automatic on most kernels.
        "xpad" "hid_sony" "hid_playstation" "hid_nintendo" "hid_steam"
      ];
      description = "Kernel modules added to the initrd for DRM and controller HID.";
    };

    debugLogToEsp = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "/dev/disk/by-partlabel/ESP";
      description = ''
        If set, mount this device (assumed FAT) at /boot-debug in the
        initrd before the agent starts and redirect the agent's
        stdout+stderr there. The ESP is unencrypted, so after a failed
        boot you can read /boot/luks-controller-unlock.log from any
        rescue environment without unlocking LUKS first.

        Strictly a debug aid — leave null in production. The log will
        contain the canonical PIN char sequence at trace level if
        verbose flags are passed; do not enable this on a system with
        an enrolled keyslot you care about secrecy for.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = config.boot.initrd.systemd.enable;
        message = ''
          boot.initrd.luks-controller-unlock.enable requires
          boot.initrd.systemd.enable = true. The scripted (non-systemd)
          stage 1 does not implement the ask-password protocol that the
          agent uses to communicate with systemd-cryptsetup.
        '';
      }
    ];

    boot.initrd.kernelModules = cfg.extraKernelModules;

    boot.initrd.systemd = {
      storePaths = [ "${cfg.package}/bin/luks-controller-unlock" ];

      mounts = lib.mkIf (cfg.debugLogToEsp != null) [
        {
          what = cfg.debugLogToEsp;
          where = "/boot-debug";
          type = "vfat";
          options = "rw,relatime,umask=0077";
          unitConfig.DefaultDependencies = false;
          wantedBy = [ "luks-controller-unlock.service" ];
          before = [ "luks-controller-unlock.service" ];
        }
      ];

      services.luks-controller-unlock = {
        description = "Controller-driven LUKS unlock agent";
        wantedBy = [ "cryptsetup.target" ];
        before = [ "cryptsetup.target" ];
        conflicts = [ "plymouth-start.service" ];
        unitConfig.DefaultDependencies = false;
        serviceConfig = {
          Type = "simple";
          ExecStart = "${cfg.package}/bin/luks-controller-unlock -vv agent";
          Restart = "on-failure";
          RestartSec = 2;
          TimeoutStartSec = 0;
        } // lib.optionalAttrs (cfg.debugLogToEsp != null) {
          # Append (not truncate) so the journal across boots and
          # restarts accumulates instead of clobbering itself.
          StandardOutput = "append:/boot-debug/luks-controller-unlock.log";
          StandardError = "append:/boot-debug/luks-controller-unlock.log";
        };
      };

      services.systemd-ask-password-console = lib.mkIf cfg.maskConsoleAgent {
        enable = false;
      };
    };
  };
}
