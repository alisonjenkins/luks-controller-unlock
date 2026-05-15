{ config, lib, pkgs, ... }:

let
  cfg = config.boot.initrd.luks-controller-unlock;

  # Bind these once so we can list them in storePaths AND reference
  # them as ExecStart= values. boot.initrd.systemd doesn't follow
  # closure of ExecStart= automatically, so a writeShellScript inline
  # in serviceConfig.ExecStart would land as a dangling reference and
  # systemd would 203/EXEC before the script ever ran.
  agentWrapper = pkgs.writeShellScript "luks-controller-unlock-debug-wrap" ''
    set -u
    MOUNTPOINT=/boot-debug
    LOG=$MOUNTPOINT/luks-controller-unlock.log
    ${pkgs.coreutils}/bin/mkdir -p "$MOUNTPOINT"
    if ! ${pkgs.util-linux}/bin/mount -t vfat -o rw,umask=0077 ${toString cfg.debugLogToEsp} "$MOUNTPOINT" 2>/dev/null; then
      exec ${cfg.package}/bin/luks-controller-unlock -v agent
    fi
    {
      echo "=== boot uptime=$(cat /proc/uptime 2>/dev/null) ==="
      ${pkgs.coreutils}/bin/uname -a
      echo "--- agent stderr+stdout ---"
    } >> "$LOG" 2>&1
    ${pkgs.coreutils}/bin/sync
    ${cfg.package}/bin/luks-controller-unlock -v agent >> "$LOG" 2>&1
    rc=$?
    echo "--- agent exited rc=$rc ---" >> "$LOG"
    ${pkgs.coreutils}/bin/sync
    exit $rc
  '';

  journalDump = pkgs.writeShellScript "dump-initrd-journal" ''
    set -u
    MOUNTPOINT=/boot-debug
    ${pkgs.coreutils}/bin/mkdir -p "$MOUNTPOINT"
    ${pkgs.util-linux}/bin/mount -t vfat -o rw ${toString cfg.debugLogToEsp} "$MOUNTPOINT" 2>/dev/null || true
    ${pkgs.coreutils}/bin/mkdir -p "$MOUNTPOINT/initrd-journal"
    if [ -d /run/log/journal ]; then
      ${pkgs.coreutils}/bin/cp -r /run/log/journal/. "$MOUNTPOINT/initrd-journal/" 2>/dev/null || true
    fi
    ${pkgs.coreutils}/bin/sync
  '';
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
        Wrap the agent in a shell script that mounts this device (assumed
        FAT) at /boot-debug and tees agent output to
        /boot-debug/luks-controller-unlock.log. Also installs an
        emergency-time hook that dumps /run/log/journal to the ESP so
        the full initrd journal survives a failed boot.

        Read after failure from any rescue env:
            mount /dev/<ESP> /mnt
            cat /mnt/luks-controller-unlock.log
            journalctl --file=/mnt/initrd-journal/*/system.journal

        Strictly debug only. The log includes verbose tracing output
        from the agent and the journal may include the canonical PIN
        char sequence — do not leave on for a system with an enrolled
        keyslot you care about secrecy for.
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
          # The wrappers themselves — without these, ExecStart= points
          # at store paths that aren't in the initrd and systemd exits
          # 203/EXEC before any redirected output exists.
          agentWrapper
          journalDump
          # And the tools the wrappers exec. writeShellScript hardcodes
          # the full /nix/store paths to coreutils + util-linux, so we
          # need those packages' bin trees too. Bash for the shebang.
          "${pkgs.bash}/bin/bash"
          "${pkgs.util-linux}/bin/mount"
          "${pkgs.util-linux}/bin/umount"
          "${pkgs.coreutils}/bin/mkdir"
          "${pkgs.coreutils}/bin/cp"
          "${pkgs.coreutils}/bin/sync"
          "${pkgs.coreutils}/bin/uname"
        ];

      services.luks-controller-unlock = {
        description = "Controller-driven LUKS unlock agent";
        wantedBy = [ "cryptsetup.target" ];
        before = [ "cryptsetup.target" ];
        conflicts = [ "plymouth-start.service" ];
        unitConfig.DefaultDependencies = false;
        serviceConfig =
          if cfg.debugLogToEsp == null then {
            Type = "simple";
            ExecStart = "${cfg.package}/bin/luks-controller-unlock agent";
            Restart = "on-failure";
            RestartSec = 2;
            TimeoutStartSec = 0;
          } else {
            Type = "simple";
            ExecStart = "${agentWrapper}";
            Restart = "on-failure";
            RestartSec = 2;
            TimeoutStartSec = 0;
          };
      };

      # On emergency entry (e.g. cryptsetup target failed) copy the
      # in-memory journald output to the ESP so the full initrd
      # journal survives the reboot.
      services.luks-controller-emergency-dump-journal = lib.mkIf (cfg.debugLogToEsp != null) {
        description = "Dump initrd journal to ESP on emergency entry";
        wantedBy = [ "emergency.target" "emergency.service" ];
        before = [ "emergency.service" ];
        unitConfig.DefaultDependencies = false;
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "${journalDump}";
        };
      };

      services.systemd-ask-password-console = lib.mkIf cfg.maskConsoleAgent {
        enable = false;
      };
    };
  };
}
