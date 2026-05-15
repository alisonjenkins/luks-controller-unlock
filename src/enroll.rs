//! Booted-system enrollment.
//!
//! Reads the user's existing LUKS passphrase from the controlling tty
//! (echo disabled), drives a two-pass controller-PIN entry on stderr,
//! then shells out to `cryptsetup luksAddKey` with `--pbkdf-memory`
//! capped to a value that won't OOM the initrd at boot. Both
//! passphrases are passed to cryptsetup over stdin to keep them out of
//! the process-table and any tempfile.
//!
//! v1: tty progress only (a row of dots). The DRM/KMS UI is reserved
//! for the unlock paths (agent + keyscript) where the system has no
//! login session contending for the screen. Enroll runs from a logged-in
//! shell and leaning on the same DRM surface would force the user onto
//! a free VT first; tty progress is friendlier.

use std::io::{self, BufRead, IsTerminal, Write};
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use clap::Parser;
use rustix::termios::{LocalModes, OptionalActions, tcgetattr, tcsetattr};
use tracing::{info, warn};

use crate::error::{Error, Result};
use crate::input::{Controller, InputEvent};
use crate::pin::{MAX_PIN_LEN, Pin};

/// Memory cost (KiB) passed to `cryptsetup luksAddKey --pbkdf-memory`.
/// Must fit comfortably in initrd RAM. 256 MiB is a safe ceiling for
/// the Steam Deck's 16 GB and small enough not to OOM smaller HTPCs.
pub const PBKDF_MEMORY_KIB: u32 = 256 * 1024;

#[derive(Parser, Debug)]
pub struct Args {
    /// LUKS2 device to enroll the controller PIN on (e.g. /dev/sda3).
    #[arg(long)]
    pub device: PathBuf,

    /// Skip the second confirmation pass. NOT recommended — a typo'd
    /// PIN locks the volume on next boot.
    #[arg(long)]
    pub no_confirm: bool,

    /// Override the Argon2 memory cost in KiB. Defaults to 256 MiB.
    #[arg(long, default_value_t = PBKDF_MEMORY_KIB)]
    pub pbkdf_memory_kib: u32,
}

pub fn run(args: Args) -> Result<()> {
    let device = args.device;
    if !device.exists() {
        return Err(Error::Config(format!(
            "device {} does not exist",
            device.display()
        )));
    }
    info!(device = %device.display(), "enroll: starting");

    let existing = read_existing_passphrase()?;
    if existing.is_empty() {
        return Err(Error::Config("existing passphrase is empty".into()));
    }

    let pin = collect_pin_with_confirm(args.no_confirm)?;
    let new = pin.to_passphrase();
    drop(pin);

    add_keyslot(&device, &existing, &new, args.pbkdf_memory_kib)?;
    info!("enroll: keyslot added");
    eprintln!("Enrollment succeeded.");
    Ok(())
}

fn read_existing_passphrase() -> Result<Vec<u8>> {
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        // Allow piping for scripted setup — read first line.
        let mut buf = String::new();
        stdin.lock().read_line(&mut buf)?;
        return Ok(buf.trim_end_matches('\n').as_bytes().to_vec());
    }

    eprint!("Existing LUKS passphrase: ");
    io::stderr().flush()?;

    let fd = stdin.as_fd();
    let saved = tcgetattr(fd).map_err(|e| Error::Subprocess(format!("tcgetattr: {e}")))?;
    let mut tio = saved.clone();
    tio.local_modes.remove(LocalModes::ECHO);
    tcsetattr(fd, OptionalActions::Now, &tio)
        .map_err(|e| Error::Subprocess(format!("tcsetattr (no-echo): {e}")))?;

    let mut buf = String::new();
    let read_result = stdin.lock().read_line(&mut buf);

    // Always try to restore the tty regardless of read outcome.
    if let Err(e) = tcsetattr(fd, OptionalActions::Now, &saved) {
        warn!("tcsetattr (restore): {e}");
    }
    eprintln!();
    read_result?;
    Ok(buf.trim_end_matches('\n').as_bytes().to_vec())
}

fn collect_pin_with_confirm(skip_confirm: bool) -> Result<Pin> {
    let mut ctrl = Controller::open_first()?;
    eprintln!(
        "Controller: {} ({})\n\
         Press buttons to enter your PIN. B (hold ≥500ms) erases. START submits.",
        ctrl.name, ctrl.path
    );

    let first = collect_pin(&mut ctrl, "Enter PIN")?;
    if first.is_empty() {
        return Err(Error::PinEmpty);
    }
    if skip_confirm {
        warn!("enroll: --no-confirm; skipping second pass");
        return Ok(first);
    }
    let second = collect_pin(&mut ctrl, "Confirm PIN")?;
    if first.to_passphrase() != second.to_passphrase() {
        return Err(Error::PinMismatch);
    }
    Ok(first)
}

fn collect_pin(ctrl: &mut Controller, prompt: &str) -> Result<Pin> {
    eprint!("{prompt}: ");
    io::stderr().flush()?;
    let mut pin = Pin::new();
    let mut last_render_len = usize::MAX;
    redraw(&mut last_render_len, pin.len())?;
    loop {
        match ctrl.next_event(None)? {
            Some(InputEvent::Press(b)) => {
                if pin.push(b).is_err() {
                    eprintln!("\nPIN reached maximum length of {MAX_PIN_LEN}.");
                    eprint!("{prompt}: ");
                    last_render_len = usize::MAX;
                }
                redraw(&mut last_render_len, pin.len())?;
            }
            Some(InputEvent::Backspace) => {
                pin.pop();
                redraw(&mut last_render_len, pin.len())?;
            }
            Some(InputEvent::Submit) => {
                eprintln!();
                return Ok(pin);
            }
            Some(InputEvent::Quit) => {
                eprintln!();
                return Err(Error::Cancelled);
            }
            None => {}
        }
    }
}

fn redraw(last: &mut usize, current: usize) -> Result<()> {
    if *last == current {
        return Ok(());
    }
    *last = current;
    let mut stderr = io::stderr().lock();
    write!(stderr, "\r")?;
    write!(stderr, "Enter PIN: ")?;
    for _ in 0..current {
        write!(stderr, "● ")?;
    }
    write!(stderr, "       ")?; // overwrite trailing chars from previous longer line
    write!(stderr, "\rEnter PIN: ")?;
    for _ in 0..current {
        write!(stderr, "● ")?;
    }
    stderr.flush()?;
    Ok(())
}

fn add_keyslot(device: &std::path::Path, existing: &[u8], new: &[u8], memory_kib: u32) -> Result<()> {
    let device_str = device
        .to_str()
        .ok_or_else(|| Error::Config(format!("device path is not UTF-8: {}", device.display())))?;
    let memory_arg = memory_kib.to_string();
    let mut child = Command::new("cryptsetup")
        .args([
            "luksAddKey",
            "--batch-mode",
            "--pbkdf-memory",
            &memory_arg,
            "--key-file=-",
            device_str,
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| Error::Subprocess(format!("spawn cryptsetup: {e}")))?;

    {
        let stdin = child.stdin.take().ok_or_else(|| {
            Error::Subprocess("cryptsetup stdin pipe was not captured".into())
        })?;
        let mut stdin = stdin;
        stdin.write_all(existing)?;
        stdin.write_all(b"\n")?;
        stdin.write_all(new)?;
        stdin.write_all(b"\n")?;
        // closing here releases the pipe so cryptsetup reads EOF.
    }

    let status = child
        .wait()
        .map_err(|e| Error::Subprocess(format!("wait cryptsetup: {e}")))?;
    if !status.success() {
        return Err(Error::Cryptsetup(format!(
            "cryptsetup exited with {}",
            status
                .code()
                .map_or_else(|| "signal".to_owned(), |c| c.to_string())
        )));
    }
    Ok(())
}
