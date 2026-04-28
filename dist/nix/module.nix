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
      services.luks-controller-unlock = {
        description = "Controller-driven LUKS unlock agent";
        wantedBy = [ "cryptsetup.target" ];
        before = [ "cryptsetup.target" ];
        conflicts = [ "plymouth-start.service" ];
        unitConfig.DefaultDependencies = false;
        serviceConfig = {
          Type = "simple";
          ExecStart = "${cfg.package}/bin/luks-controller-unlock agent";
          Restart = "on-failure";
          RestartSec = 2;
          TimeoutStartSec = 0;
        };
      };

      services.systemd-ask-password-console = lib.mkIf cfg.maskConsoleAgent {
        enable = false;
      };
    };
  };
}
