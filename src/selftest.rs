//! Install-time host check. Verifies a DRM connector exists, an evdev
//! gamepad is present, `cryptsetup` >= 2.6 is on PATH, and (where the
//! systemd agent path applies) `/run/systemd` is mounted. Also verifies
//! `hid-steam` is loadable so the Steam Controller can be used.

use tracing::info;

use crate::error::{Error, Result};

pub fn run() -> Result<()> {
    info!("selftest: starting");
    Err(Error::Unsupported("selftest not implemented yet".into()))
}
