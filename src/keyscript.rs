//! crypttab `keyscript=` entry for non-systemd initrds (Debian/Ubuntu's
//! initramfs-tools).
//!
//! initramfs-tools' `cryptroot` script invokes the keyscript with the
//! crypttab entry's third field as a single argument (often `none`)
//! and these env vars set:
//!   * `CRYPTTAB_NAME` — the mapped name (e.g. `cryptroot`)
//!   * `CRYPTTAB_SOURCE` — the underlying device
//!   * `CRYPTTAB_KEY` — passed-through key field
//!   * `CRYPTTAB_TRIED` — number of previous attempts on this device
//!
//! The script's job is to print the passphrase to stdout (no trailing
//! newline) and exit 0; cryptsetup pipes the output as the key. Errors
//! to stderr.

use std::io::{self, Write};
use std::path::PathBuf;

use clap::Parser;
use tracing::{info, warn};

use crate::error::{Error, Result};
use crate::input::{Controller, InputEvent};
use crate::pin::Pin;
use crate::ui::drm::DrmSurface;
use crate::ui::render::{self, Message, UiState};

#[derive(Parser, Debug)]
pub struct Args {
    /// Override the device name shown on the prompt (defaults to $CRYPTTAB_NAME).
    #[arg(long)]
    pub name: Option<String>,

    /// DRM card device.
    #[arg(long, default_value = "/dev/dri/card0")]
    pub card: PathBuf,

    /// Re-prompt up to N times if the user submits an empty PIN. After
    /// that, exit non-zero so cryptsetup falls back to the next agent.
    #[arg(long, default_value_t = 3)]
    pub max_attempts: u32,
}

pub fn run(args: Args) -> Result<()> {
    let name = args
        .name
        .or_else(|| std::env::var("CRYPTTAB_NAME").ok())
        .unwrap_or_else(|| "root".to_owned());
    let tried: u32 = std::env::var("CRYPTTAB_TRIED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    info!(%name, tried, "keyscript: starting");

    let mut surface = DrmSurface::open(&args.card)?;
    let mut state = UiState::new(format!("UNLOCK {}", name.to_ascii_uppercase()));
    state.message = if tried > 0 {
        Some(Message::WrongPin)
    } else {
        Some(Message::EnterPin)
    };

    let mut controller = open_controller_with_banner(&mut surface, &mut state)?;
    state.controller_ok = true;
    state.message = if tried > 0 {
        Some(Message::WrongPin)
    } else {
        Some(Message::EnterPin)
    };
    redraw(&mut surface, &state)?;

    let mut attempts = 0;
    let pin = loop {
        let pin = collect_pin(&mut surface, &mut state, &mut controller)?;
        if !pin.is_empty() {
            break pin;
        }
        attempts += 1;
        if attempts >= args.max_attempts {
            warn!("keyscript: empty PIN submitted {attempts} times; giving up");
            return Err(Error::PinEmpty);
        }
    };

    state.message = Some(Message::Verifying);
    redraw(&mut surface, &state)?;

    // Print without trailing newline so cryptsetup uses exactly our bytes.
    let mut stdout = io::stdout().lock();
    stdout.write_all(&pin.to_passphrase())?;
    stdout.flush()?;
    Ok(())
}

fn collect_pin(
    surface: &mut DrmSurface,
    state: &mut UiState,
    controller: &mut Controller,
) -> Result<Pin> {
    let mut pin = Pin::new();
    state.pin_len = 0;
    redraw(surface, state)?;
    loop {
        let event = controller.next_event(None)?;
        let mut dirty = false;
        match event {
            Some(InputEvent::Press(b)) => {
                if pin.push(b).is_ok() {
                    state.pin_len = pin.len();
                    dirty = true;
                }
            }
            Some(InputEvent::Backspace) => {
                if pin.pop().is_some() {
                    state.pin_len = pin.len();
                    dirty = true;
                }
            }
            Some(InputEvent::Submit) => return Ok(pin),
            None => {}
        }
        if dirty {
            redraw(surface, state)?;
        }
    }
}

fn open_controller_with_banner(
    surface: &mut DrmSurface,
    state: &mut UiState,
) -> Result<Controller> {
    use std::time::Duration;
    loop {
        match Controller::open_first() {
            Ok(c) => return Ok(c),
            Err(_) => {
                state.controller_ok = false;
                state.message = Some(Message::PluginController);
                redraw(surface, state)?;
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

fn redraw(surface: &mut DrmSurface, state: &UiState) -> Result<()> {
    let mut frame = surface.frame()?;
    render::render(&mut frame, state)
}
