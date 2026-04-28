//! crypttab `keyscript=` entry for non-systemd initrds (Debian/Ubuntu's
//! initramfs-tools). The script reads `CRYPTTAB_NAME` / `CRYPTTAB_SOURCE`
//! from the environment, drives the UI, and prints the derived passphrase
//! to stdout (no trailing newline).

use clap::Parser;
use tracing::info;

use crate::error::{Error, Result};

#[derive(Parser, Debug)]
pub struct Args {
    /// Override the device name shown on the prompt (defaults to $CRYPTTAB_NAME).
    #[arg(long)]
    pub name: Option<String>,
}

pub fn run(args: Args) -> Result<()> {
    info!(name = ?args.name, "keyscript: starting");
    Err(Error::Unsupported("keyscript flow not implemented yet".into()))
}
