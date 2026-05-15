//! Disable Steam Controller / Steam Deck "lizard mode" (KB+mouse
//! emulation) for the lifetime of the process. Without this the Deck
//! built-in controller and the standalone Steam Controller surface
//! as a keyboard + trackpad mouse and never deliver gamepad evdev
//! events.
//!
//! Drop alone is not enough: SIGINT (Ctrl-C) and SIGTERM default to
//! killing the process without running destructors, which would leave
//! lizard mode disabled forever. We additionally register an atexit
//! callback (so cleanup runs on `process::exit`) and install signal
//! handlers that funnel SIGINT/SIGTERM through `process::exit`.

#![allow(unsafe_code)]

use std::ffi::c_int;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::thread::sleep;
use std::time::Duration;

use tracing::{debug, info, warn};

const PARAM: &str = "/sys/module/hid_steam/parameters/lizard_mode";

/// Stash for the prior `lizard_mode` value. Populated by
/// `LizardGuard::disable` the first time it runs and read by the
/// atexit / signal-driven `restore` path. `None` means we have
/// nothing to restore (module absent, write failed, or already
/// restored).
static PRIOR: OnceLock<Mutex<Option<String>>> = OnceLock::new();

/// RAII guard that toggles the global hid-steam lizard mode off and
/// arranges for the prior value to be restored on drop OR on
/// SIGINT/SIGTERM/`process::exit`.
pub struct LizardGuard {
    /// True once we have armed the global cleanup path; redundant
    /// drops are no-ops.
    armed: bool,
}

impl LizardGuard {
    /// Try to disable lizard mode. No-op when the hid-steam module is
    /// not loaded — non-Steam controllers don't need this.
    pub fn disable() -> Self {
        let path = Path::new(PARAM);
        if !path.exists() {
            debug!("lizard: {PARAM} absent; hid-steam not loaded — skipping");
            return Self { armed: false };
        }
        let prior = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                warn!("lizard: read {PARAM} failed: {e}");
                return Self { armed: false };
            }
        };
        if let Err(e) = fs::write(path, "0\n") {
            warn!("lizard: write {PARAM} failed: {e} (need root?)");
            return Self { armed: false };
        }
        info!("lizard: disabled (was {:?})", prior.trim());
        // hid-steam re-grabs HID inputs and republishes evdev nodes;
        // give udev a tick to settle before enumerate.
        sleep(Duration::from_millis(150));

        // First-time setup of the global stash + signal handlers + atexit.
        let stash = PRIOR.get_or_init(|| Mutex::new(None));
        if let Ok(mut g) = stash.lock() {
            // Don't clobber an earlier prior — first one wins (matches
            // the very first observable lizard mode value).
            if g.is_none() {
                *g = Some(prior);
                install_handlers();
            }
        }
        Self { armed: true }
    }
}

impl Drop for LizardGuard {
    fn drop(&mut self) {
        if self.armed {
            restore();
        }
    }
}

fn restore() {
    let Some(stash) = PRIOR.get() else { return };
    let Ok(mut g) = stash.lock() else { return };
    if let Some(prior) = g.take() {
        if let Err(e) = fs::write(PARAM, &prior) {
            warn!("lizard: restore {PARAM} failed: {e}");
        } else {
            debug!("lizard: restored prior value");
        }
    }
}

fn install_handlers() {
    // atexit fires for normal exit AND `process::exit` (including the
    // path our signal handlers take). Returning from `main` also runs
    // it after destructors.
    // SAFETY: `restore_atexit` is `extern "C"` with no args, the
    // contract for libc::atexit. The function only reads/writes the
    // stash mutex and a small file; both safe in atexit context.
    unsafe {
        libc::atexit(restore_atexit);
    }
    install_signal(libc::SIGINT);
    install_signal(libc::SIGTERM);
    install_signal(libc::SIGHUP);
}

fn install_signal(sig: c_int) {
    // SAFETY: libc::signal takes a function pointer; `signal_handler`
    // matches the required ABI. The handler is async-signal-unsafe
    // (it calls `process::exit`), which is the standard pragmatic
    // tradeoff for ensuring atexit runs — the alternative (deferred
    // flag check from a polling loop) does not cover the case where
    // the loop is blocked in a syscall that cannot be interrupted.
    unsafe {
        libc::signal(sig, signal_handler as *const () as libc::sighandler_t);
    }
}

extern "C" fn signal_handler(sig: c_int) {
    // Convention: 128 + signum. Triggers atexit -> restore().
    std::process::exit(128 + sig);
}

extern "C" fn restore_atexit() {
    restore();
}
