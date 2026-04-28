//! systemd ask-password agent.
//!
//! Watches a directory (default `/run/systemd/ask-password/`) for `ask.*`
//! request files written by `systemd-cryptsetup`. For each request:
//!   1. parse the INI to extract the reply socket path, message, and
//!      optional `NotAfter` timeout;
//!   2. open the controller (retrying with a "PLUG IN A CONTROLLER"
//!      banner if none is connected);
//!   3. open a DRM/KMS surface and drive the unlock UI;
//!   4. when START is pressed, encode the PIN to a passphrase and
//!      reply over the AF_UNIX socket;
//!   5. tear down DRM and re-arm.
//!
//! The first agent to reply wins, which means our agent and the stock
//! `systemd-ask-password-console` agent coexist — keyboard passphrase
//! entry continues to work for any LUKS keyslot the user enrolled with
//! a keyboard. Distros that ship with both agents enabled should mask
//! the console agent in the initrd via the drop-in shipped under
//! `dist/systemd/` if a "split-screen" effect with the controller UI
//! becomes annoying.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use inotify::{Inotify, WatchMask};
use tracing::{debug, error, info, warn};

use crate::error::{Error, Result};
use crate::input::{Controller, InputEvent};
use crate::pin::Pin;
use crate::ui::drm::DrmSurface;
use crate::ui::render::{Message, UiState};
use crate::ui::render;

const POLL_TIMEOUT: Duration = Duration::from_millis(500);
/// Initial back-off after a wrong PIN, doubled up to LOCKOUT_MAX.
const LOCKOUT_INITIAL: Duration = Duration::from_secs(1);
const LOCKOUT_MAX: Duration = Duration::from_secs(30);

#[derive(Parser, Debug)]
pub struct Args {
    /// Directory to watch for ask.* request files.
    #[arg(long, default_value = "/run/systemd/ask-password")]
    pub watch_dir: PathBuf,

    /// DRM card device to use for the UI.
    #[arg(long, default_value = "/dev/dri/card0")]
    pub card: PathBuf,
}

pub fn run(args: Args) -> Result<()> {
    info!(watch_dir = %args.watch_dir.display(), card = %args.card.display(), "agent: starting");

    if !args.watch_dir.exists() {
        return Err(Error::Unsupported(format!(
            "watch dir {} does not exist (systemd ask-password not initialised in this initrd?)",
            args.watch_dir.display()
        )));
    }

    let mut inot = Inotify::init().map_err(|e| Error::Io(e))?;
    inot.watches()
        .add(
            &args.watch_dir,
            WatchMask::CREATE | WatchMask::MOVED_TO,
        )
        .map_err(|e| Error::Io(e))?;

    let mut handled = std::collections::HashSet::<PathBuf>::new();
    let mut lockout = LOCKOUT_INITIAL;

    // Process any requests that already exist.
    process_pending(&args.watch_dir, &args.card, &mut handled, &mut lockout)?;

    let mut buffer = [0u8; 4096];
    loop {
        let events = match inot.read_events_blocking(&mut buffer) {
            Ok(it) => it,
            Err(e) => {
                warn!("agent: inotify read failed: {e}; retrying");
                std::thread::sleep(POLL_TIMEOUT);
                continue;
            }
        };
        let mut new_files = Vec::new();
        for ev in events {
            if let Some(name) = ev.name {
                let path = args.watch_dir.join(name);
                if is_ask_file(&path) && !handled.contains(&path) {
                    new_files.push(path);
                }
            }
        }
        for path in new_files {
            if let Err(e) = process_request(&path, &args.card, &mut lockout) {
                warn!(?path, "agent: failed to process request: {e}");
            }
            handled.insert(path);
        }
        // Re-scan: inotify can miss a file created between watch setup and
        // a previously-handled batch; cheap to re-poll.
        process_pending(&args.watch_dir, &args.card, &mut handled, &mut lockout)?;
    }
}

fn process_pending(
    dir: &Path,
    card: &Path,
    handled: &mut std::collections::HashSet<PathBuf>,
    lockout: &mut Duration,
) -> Result<()> {
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) => return Err(Error::Io(e)),
    };
    for entry in read.flatten() {
        let path = entry.path();
        if !is_ask_file(&path) || handled.contains(&path) {
            continue;
        }
        if let Err(e) = process_request(&path, card, lockout) {
            warn!(?path, "agent: failed to process request: {e}");
        }
        handled.insert(path);
    }
    Ok(())
}

fn process_request(path: &Path, card: &Path, lockout: &mut Duration) -> Result<()> {
    let req = AskRequest::parse(path)?;
    if req.expired() {
        info!(?path, "agent: skipping expired ask file");
        return Ok(());
    }
    info!(socket = %req.socket.display(), id = ?req.id, msg = %req.message, "agent: processing request");

    let mut surface = DrmSurface::open(card)?;
    let mut state = UiState::new(req.message.clone());
    state.message = Some(Message::EnterPin);

    let mut controller = open_controller_with_banner(&mut surface, &mut state)?;
    state.controller_ok = true;
    state.message = Some(Message::EnterPin);
    redraw(&mut surface, &state)?;

    let mut pin = Pin::new();
    loop {
        let event = controller
            .next_event(Some(POLL_TIMEOUT))
            .unwrap_or(None);
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
            Some(InputEvent::Submit) => {
                if pin.is_empty() {
                    continue;
                }
                state.message = Some(Message::Verifying);
                redraw(&mut surface, &state)?;
                let pass = pin.to_passphrase();
                send_reply(&req.socket, true, &pass)?;
                info!("agent: replied {} bytes", pass.len());
                // systemd will remove the ask file on success and create a
                // new one on failure. Apply our local back-off either way:
                std::thread::sleep(*lockout);
                if request_resolved(path) {
                    *lockout = LOCKOUT_INITIAL;
                    return Ok(());
                }
                // wrong PIN: bump lockout, reset state, continue
                *lockout = (*lockout * 2).min(LOCKOUT_MAX);
                state.message = Some(Message::WrongPin);
                pin.clear();
                state.pin_len = 0;
                redraw(&mut surface, &state)?;
                std::thread::sleep(Duration::from_secs(1));
                state.message = Some(Message::EnterPin);
                dirty = true;
            }
            None => {}
        }
        if dirty {
            redraw(&mut surface, &state)?;
        }
        if request_resolved(path) {
            // Some other agent answered — abandon our prompt.
            info!("agent: request resolved by another agent; releasing UI");
            return Ok(());
        }
    }
}

fn open_controller_with_banner(
    surface: &mut DrmSurface,
    state: &mut UiState,
) -> Result<Controller> {
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

fn send_reply(socket_path: &Path, ok: bool, password: &[u8]) -> Result<()> {
    let sock = UnixDatagram::unbound()
        .map_err(|e| Error::AskPasswordReply(format!("unbound: {e}")))?;
    let mut payload = Vec::with_capacity(password.len() + 1);
    payload.push(if ok { b'+' } else { b'-' });
    payload.extend_from_slice(password);
    sock.send_to(&payload, socket_path)
        .map_err(|e| Error::AskPasswordReply(format!("send_to {}: {e}", socket_path.display())))?;
    Ok(())
}

fn request_resolved(path: &Path) -> bool {
    !path.exists()
}

fn is_ask_file(p: &Path) -> bool {
    p.file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.starts_with("ask."))
}

#[derive(Debug)]
struct AskRequest {
    socket: PathBuf,
    message: String,
    id: Option<String>,
    not_after: Option<Duration>,
}

impl AskRequest {
    fn parse(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let kv = parse_ini_section(&content, "Ask")
            .ok_or_else(|| Error::AskPasswordParse(format!("no [Ask] section in {}", path.display())))?;
        let socket = kv
            .get("Socket")
            .ok_or_else(|| Error::AskPasswordParse("missing Socket=".into()))?
            .clone();
        let message = kv
            .get("Message")
            .cloned()
            .unwrap_or_else(|| "Enter passphrase".to_owned());
        let id = kv.get("Id").cloned();
        let not_after = kv
            .get("NotAfter")
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|us| *us > 0)
            .map(Duration::from_micros);
        Ok(Self {
            socket: PathBuf::from(socket),
            message,
            id,
            not_after,
        })
    }

    fn expired(&self) -> bool {
        let Some(not_after) = self.not_after else {
            return false;
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        now >= not_after
    }
}

/// Minimal INI parser tailored to systemd's ask-password file format.
/// Returns the values from the named section. Keys with `=` in values
/// are preserved verbatim. Comments (`#`/`;`) and blank lines ignored.
fn parse_ini_section(content: &str, section: &str) -> Option<HashMap<String, String>> {
    let mut current: Option<&str> = None;
    let mut out = HashMap::new();
    let target = format!("[{section}]");
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current = Some(if line == target { section } else { "" });
            continue;
        }
        if current == Some(section) {
            if let Some((k, v)) = line.split_once('=') {
                out.insert(k.trim().to_owned(), v.trim().to_owned());
            }
        }
    }
    if out.is_empty() && !content.contains(&target) {
        return None;
    }
    Some(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_systemd_ask_file() {
        let content = "\
[Ask]
PID=42
Socket=/run/systemd/ask-password/sck.123
AcceptCached=0
Echo=0
NotAfter=0
Message=Please enter passphrase for disk root:
Id=cryptsetup:/dev/sda3
";
        let kv = parse_ini_section(content, "Ask").unwrap();
        assert_eq!(kv.get("Socket").unwrap(), "/run/systemd/ask-password/sck.123");
        assert_eq!(kv.get("Id").unwrap(), "cryptsetup:/dev/sda3");
        assert_eq!(kv.get("NotAfter").unwrap(), "0");
        assert!(kv.get("Message").unwrap().starts_with("Please"));
    }

    #[test]
    fn ignores_other_sections() {
        let content = "\
[Other]
Socket=/wrong
[Ask]
Socket=/right
";
        let kv = parse_ini_section(content, "Ask").unwrap();
        assert_eq!(kv.get("Socket").unwrap(), "/right");
    }

    #[test]
    fn missing_section_returns_none() {
        let content = "[Foo]\nKey=value\n";
        assert!(parse_ini_section(content, "Ask").is_none());
    }

    #[test]
    fn ask_file_path_detection() {
        assert!(is_ask_file(Path::new("/run/systemd/ask-password/ask.abc")));
        assert!(!is_ask_file(Path::new("/run/systemd/ask-password/sck.abc")));
        assert!(!is_ask_file(Path::new("/run/systemd/ask-password/")));
    }
}
