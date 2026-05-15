//! Disable Steam Controller / Steam Deck "lizard mode" (KB+mouse
//! emulation) for the lifetime of this guard. Without this the
//! built-in Deck controller and the standalone Steam Controller
//! surface as a keyboard + trackpad mouse and never deliver gamepad
//! evdev events. Restores the prior value on drop.

use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use tracing::{debug, info, warn};

const PARAM: &str = "/sys/module/hid_steam/parameters/lizard_mode";

/// RAII guard that toggles the global hid-steam lizard mode off and
/// restores the prior value when dropped.
pub struct LizardGuard {
    prior: Option<String>,
}

impl LizardGuard {
    /// Try to disable lizard mode. No-op (and silent at debug level)
    /// when the hid-steam module is not loaded — non-Steam controllers
    /// don't need this.
    pub fn disable() -> Self {
        let path = Path::new(PARAM);
        if !path.exists() {
            debug!("lizard: {PARAM} absent; hid-steam not loaded — skipping");
            return Self { prior: None };
        }
        let prior = fs::read_to_string(path).ok();
        match fs::write(path, "0\n") {
            Ok(()) => {
                info!(
                    "lizard: disabled (was {:?})",
                    prior.as_deref().map(str::trim)
                );
                // hid-steam re-grabs HID inputs and republishes evdev
                // nodes — give udev a tick to settle before enumerate.
                sleep(Duration::from_millis(150));
                Self { prior }
            }
            Err(e) => {
                warn!("lizard: write {PARAM} failed: {e} (need root?)");
                Self { prior: None }
            }
        }
    }
}

impl Drop for LizardGuard {
    fn drop(&mut self) {
        if let Some(prior) = self.prior.take() {
            if let Err(e) = fs::write(PARAM, prior) {
                warn!("lizard: restore {PARAM} failed: {e}");
            }
        }
    }
}
