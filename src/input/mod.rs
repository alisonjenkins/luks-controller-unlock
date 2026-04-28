//! Controller input. Pure-Rust evdev poll, per-controller mapping tables,
//! hot-plug via udev netlink with inotify on `/dev/input/` as fallback.

use tracing::info;

use crate::error::{Error, Result};

pub mod tables;

/// Dump canonical button events to stderr (`luks-controller-unlock test-input`).
pub fn run_test() -> Result<()> {
    info!("test-input: starting");
    Err(Error::Unsupported("test-input not implemented yet".into()))
}
