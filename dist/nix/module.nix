{ config, lib, pkgs, ... }:

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

    boot.initrd.kernelModules =
      cfg.extraKernelModules
      # vfat (and the NLS charmaps it pulls in) only needed when the
      # debug log is going to a FAT-formatted ESP. nvme already comes
      # in via the auto-detected hardware-configuration modules.
      ++ lib.optionals (cfg.debugLogToEsp != null) [
        "vfat"
        "nls_cp437"
        "nls_iso8859-1"
      ];

    boot.initrd.systemd = {
      storePaths =
        [ "${cfg.package}/bin/luks-controller-unlock" ]
        ++ lib.optionals (cfg.debugLogToEsp != null) [
          "${pkgs.util-linux}/bin/mount"
          "${pkgs.coreutils}/bin/mkdir"
        ];

      # Separate oneshot unit to mount the ESP. The agent unit
      # Requires+After it so systemd evaluates the StandardOutput=
      # append: path AFTER the mount succeeded — which it does not do
      # if the mount is an ExecStartPre on the agent unit itself
      # (StandardOutput is set up first).
      services.luks-controller-mount-debug-esp = lib.mkIf (cfg.debugLogToEsp != null) {
        description = "Mount ESP at /boot-debug for luks-controller-unlock log";
        wantedBy = [ "luks-controller-unlock.service" ];
        before = [ "luks-controller-unlock.service" ];
        unitConfig.DefaultDependencies = false;
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = [
            "${pkgs.coreutils}/bin/mkdir -p /boot-debug"
            "${pkgs.util-linux}/bin/mount -t vfat -o rw,umask=0077 ${cfg.debugLogToEsp} /boot-debug"
          ];
          # mount fails with EBUSY on retry; ignore so a service
          # restart doesn't bounce off an already-mounted FS.
          SuccessExitStatus = [ "0" "32" ];
        };
      };

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
          StandardOutput = "append:/boot-debug/luks-controller-unlock.log";
          StandardError = "append:/boot-debug/luks-controller-unlock.log";
        };
      } // lib.optionalAttrs (cfg.debugLogToEsp != null) {
        requires = [ "luks-controller-mount-debug-esp.service" ];
        after = [ "luks-controller-mount-debug-esp.service" ];
      };

      services.systemd-ask-password-console = lib.mkIf cfg.maskConsoleAgent {
        enable = false;
      };
    };
  };
}
