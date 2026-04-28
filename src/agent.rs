//! systemd ask-password agent. Watches `/run/systemd/ask-password/`,
//! drives the controller UI, replies via the AF_UNIX socket named in the
//! request file. Coexists with the stock console agent (whichever responds
//! first wins, so keyboard fallback continues to work).

use clap::Parser;
use std::path::PathBuf;
use tracing::info;

use crate::error::{Error, Result};

#[derive(Parser, Debug)]
pub struct Args {
    /// Directory to watch for ask.* request files.
    #[arg(long, default_value = "/run/systemd/ask-password")]
    pub watch_dir: PathBuf,
}

pub fn run(args: Args) -> Result<()> {
    info!(watch_dir = %args.watch_dir.display(), "agent: starting");
    Err(Error::Unsupported("agent loop not implemented yet".into()))
}
