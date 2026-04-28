//! Fullscreen unlock UI. DRM/KMS dumb buffer + tiny-skia software
//! rasterizer + fontdue text. Pretty card layout, dot row showing entered
//! PIN length (no symbol leak), controller-status indicator.

use clap::Parser;
use tracing::info;

use crate::error::{Error, Result};

pub mod drm;
pub mod render;

#[derive(Parser, Debug)]
pub struct TestArgs {
    /// DRM card device.
    #[arg(long, default_value = "/dev/dri/card0")]
    pub card: std::path::PathBuf,
}

pub fn run_test(args: TestArgs) -> Result<()> {
    info!(card = %args.card.display(), "test-ui: starting");
    Err(Error::Unsupported("test-ui not implemented yet".into()))
}
