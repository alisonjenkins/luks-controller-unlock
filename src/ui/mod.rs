//! Fullscreen unlock UI. DRM/KMS dumb buffer + tiny-skia software
//! rasterizer + embedded 5x7 bitmap font. Centered card with a dot row
//! that shows entered PIN length without leaking which buttons were
//! pressed, plus a controller-connection status pip.

use std::time::{Duration, Instant};

use clap::Parser;
use tracing::info;

use crate::error::Result;
use crate::input::{Controller, InputEvent};
use crate::pin::Pin;

pub mod drm;
pub mod render;

pub use render::{Message, UiState};

#[derive(Parser, Debug)]
pub struct TestArgs {
    /// DRM card device.
    #[arg(long, default_value = "/dev/dri/card0")]
    pub card: std::path::PathBuf,

    /// Render the UI for this many seconds then exit. If a controller
    /// is connected, button events drive the dot row interactively for
    /// the same duration. Set to 0 to run until START is pressed.
    #[arg(long, default_value_t = 0)]
    pub seconds: u64,

    /// Skip controller open and render a static frame only.
    #[arg(long)]
    pub no_input: bool,
}

pub fn run_test(args: &TestArgs) -> Result<()> {
    info!(card = %args.card.display(), "test-ui: starting");
    let mut surface = drm::DrmSurface::open(&args.card)?;
    info!(width = surface.width(), height = surface.height(), "test-ui: surface ready");

    let controller = if args.no_input {
        None
    } else {
        match Controller::open_first() {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::warn!("test-ui: no controller ({e}); rendering static frame");
                None
            }
        }
    };

    let mut state = UiState::new(format!("UNLOCK ROOT  {}x{}", surface.width(), surface.height()));
    state.controller_ok = controller.is_some();
    state.message = if controller.is_some() {
        Some(Message::EnterPin)
    } else {
        Some(Message::PluginController)
    };
    let mut pin = Pin::new();

    let deadline = (args.seconds > 0).then(|| Instant::now() + Duration::from_secs(args.seconds));

    {
        let mut frame = surface.frame()?;
        render::render(&mut frame, &state)?;
    }

    let Some(mut ctrl) = controller else {
        // Static-only path: hold the frame for `seconds` (or 5s default).
        let hold = Duration::from_secs(if args.seconds == 0 { 5 } else { args.seconds });
        std::thread::sleep(hold);
        return Ok(());
    };

    loop {
        let timeout = deadline.map(|d| d.saturating_duration_since(Instant::now()));
        if matches!(timeout, Some(t) if t.is_zero()) {
            break;
        }
        let event = ctrl.next_event(timeout.or(Some(Duration::from_millis(500))))?;
        let dirty = match event {
            Some(InputEvent::Press(b)) if pin.push(b).is_ok() => {
                state.pin_len = pin.len();
                true
            }
            Some(InputEvent::Backspace) if pin.pop().is_some() => {
                state.pin_len = pin.len();
                true
            }
            Some(InputEvent::Submit) => {
                info!("test-ui: submit pressed (len={})", pin.len());
                if args.seconds == 0 {
                    break;
                }
                false
            }
            Some(InputEvent::Quit) => {
                info!("test-ui: quit (SELECT+START)");
                break;
            }
            _ => false,
        };
        if dirty {
            let mut frame = surface.frame()?;
            render::render(&mut frame, &state)?;
        }
    }

    Ok(())
}
