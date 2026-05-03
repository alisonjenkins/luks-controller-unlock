//! Install-time host check.
//!
//! Verifies the local system is suitable to enroll on. Returns a typed
//! error on the first failure; logs a `pass:`/`fail:` line for each
//! check so the operator can see what's missing.

use std::path::Path;
use std::process::Command;

use tracing::{info, warn};

use crate::error::{Error, Result};
use crate::input::Controller;

pub fn run() -> Result<()> {
    info!("selftest: starting");

    check_drm_card("/dev/dri/card0").or_else(|e| {
        warn!("fail: drm card: {e}");
        check_any_drm_card()
    })?;

    check_controller()?;
    check_cryptsetup_version()?;
    check_systemd_run_dir();
    check_hid_steam_module();

    info!("selftest: all required checks passed");
    Ok(())
}

fn check_drm_card(path: &str) -> Result<()> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(Error::NoDrmConnector);
    }
    info!("pass: drm card present at {path}");
    Ok(())
}

fn check_any_drm_card() -> Result<()> {
    for entry in std::fs::read_dir("/dev/dri")? {
        let entry = entry?;
        let name = entry.file_name();
        let s = name.to_string_lossy();
        if s.starts_with("card") {
            info!("pass: drm card present at /dev/dri/{s}");
            return Ok(());
        }
    }
    Err(Error::NoDrmConnector)
}

fn check_controller() -> Result<()> {
    match Controller::open_first() {
        Ok(c) => {
            info!("pass: controller detected: {} ({})", c.name, c.path);
            info!(
                "    sources: lt={} rt={} dpad={}",
                trigger_source(c.caps.lt_uses_axis),
                trigger_source(c.caps.rt_uses_axis),
                if c.caps.dpad_uses_hat { "hat" } else { "buttons" },
            );
            info!(
                "    thresholds: lt press/release = {}/{}, rt press/release = {}/{}",
                c.caps.lt_press_threshold,
                c.caps.lt_release_threshold,
                c.caps.rt_press_threshold,
                c.caps.rt_release_threshold,
            );
            Ok(())
        }
        Err(e) => {
            warn!("fail: no controller: {e}");
            warn!("    hint: plug in a wired controller and re-run.");
            Err(e)
        }
    }
}

const fn trigger_source(uses_axis: bool) -> &'static str {
    if uses_axis { "axis" } else { "button" }
}

fn check_cryptsetup_version() -> Result<()> {
    let out = Command::new("cryptsetup")
        .arg("--version")
        .output()
        .map_err(|e| Error::Subprocess(format!("spawn cryptsetup: {e}")))?;
    if !out.status.success() {
        return Err(Error::Cryptsetup(format!(
            "cryptsetup --version failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().next().unwrap_or("(unknown)");
    let version = line.trim().split_whitespace().last().unwrap_or("?");
    if let Some((major, minor)) = parse_major_minor(version) {
        if (major, minor) < (2, 6) {
            return Err(Error::Cryptsetup(format!(
                "cryptsetup {version} is too old (need >= 2.6)"
            )));
        }
        info!("pass: cryptsetup {version}");
        Ok(())
    } else {
        warn!(
            "fail: cryptsetup version line did not parse: {line:?}; assuming OK"
        );
        Ok(())
    }
}

fn parse_major_minor(s: &str) -> Option<(u32, u32)> {
    let mut parts = s.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Informational only — initramfs-tools targets won't have this on the
/// running host, that's fine. The agent runs inside the initrd.
fn check_systemd_run_dir() {
    let p = Path::new("/run/systemd/ask-password");
    if p.exists() {
        info!("info: /run/systemd/ask-password present (systemd ask-password agent path is wired)");
    } else {
        info!(
            "info: /run/systemd/ask-password missing; this is expected outside an initrd context"
        );
    }
}

/// Informational. Required at boot for Steam Controller; warn if missing.
fn check_hid_steam_module() {
    let modules = std::fs::read_to_string("/proc/modules").unwrap_or_default();
    let loaded = modules.lines().any(|l| l.starts_with("hid_steam "));
    if loaded {
        info!("pass: hid_steam module loaded");
        return;
    }
    let path = Path::new("/sys/module/hid_steam");
    if path.exists() {
        info!("pass: hid_steam module available");
        return;
    }
    info!("info: hid_steam not loaded (Steam Controller users should ensure the module is in their initrd)");
}
