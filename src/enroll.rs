//! Booted-system enrollment. Two-pass PIN entry, then `cryptsetup luksAddKey`
//! with `--pbkdf-memory` capped to a value that will not OOM the initrd.

use clap::Parser;
use std::path::PathBuf;
use tracing::info;

use crate::error::{Error, Result};

/// Memory cost (KiB) passed to `cryptsetup luksAddKey --pbkdf-memory=`.
/// Must fit comfortably in initrd RAM. 256 MiB is a safe ceiling.
pub const PBKDF_MEMORY_KIB: u32 = 256 * 1024;

#[derive(Parser, Debug)]
pub struct Args {
    /// LUKS2 device to enroll the controller PIN on (e.g. /dev/sda3).
    #[arg(long)]
    pub device: PathBuf,

    /// Skip the confirmation pass. Not recommended.
    #[arg(long)]
    pub no_confirm: bool,
}

pub fn run(args: Args) -> Result<()> {
    info!(device = %args.device.display(), "enroll: starting");
    Err(Error::Unsupported("enroll flow not implemented yet".into()))
}
