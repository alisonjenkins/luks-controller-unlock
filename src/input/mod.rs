//! Controller input via evdev.
//!
//! v1: single-device. Opens the first device on `/dev/input/event*` that
//! advertises `BTN_SOUTH`. Multi-device + udev hot-plug is a follow-up.
//!
//! Emits a stream of canonical events to the caller. Implements the
//! B-hold-for-backspace state machine internally so callers do not need
//! to manage timing: a B-tap (release < 500 ms) produces `Press(B)`; a
//! B-hold (≥ 500 ms) produces `Backspace` and the eventual release is
//! suppressed.

use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, poll};
use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::pin::CanonicalButton;

pub mod lizard;
pub mod tables;

use lizard::LizardGuard;
use tables::{
    ABS_HAT0X, ABS_HAT0Y, ABS_RZ, ABS_Z, BTN_DPAD_DOWN, BTN_DPAD_LEFT, BTN_DPAD_RIGHT,
    BTN_DPAD_UP, BTN_EAST, BTN_SELECT, BTN_SOUTH, BTN_START, BTN_TL2, BTN_TR2,
    TRIGGER_PRESS_THRESHOLD, TRIGGER_RELEASE_THRESHOLD, key_to_canonical,
    scale_trigger_thresholds,
};

/// Hold time after which `BTN_EAST` counts as backspace instead of B.
pub const BACKSPACE_HOLD: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    /// A canonical button was committed (B-taps surface here too).
    Press(CanonicalButton),
    /// START was pressed: caller should submit the current PIN.
    Submit,
    /// B was held past the backspace threshold.
    Backspace,
    /// SELECT + START held simultaneously: caller should exit cleanly
    /// so RAII guards (e.g. lizard mode restore) fire. This is the
    /// only safe exit on a Deck where the only inputs are the
    /// controller and the touchscreen.
    Quit,
}

#[derive(Debug, Default)]
struct AxisState {
    lt_held: bool,
    rt_held: bool,
    hat_x: i32, // -1 / 0 / +1
    hat_y: i32,
}

#[derive(Debug, Default)]
struct ChordState {
    select_held: bool,
    start_held: bool,
    quit_emitted: bool,
}

#[derive(Debug, Clone, Copy)]
enum BState {
    Idle,
    Pressed { at: Instant },
    ConsumedAsBackspace,
}

pub struct Controller {
    pub name: String,
    pub path: String,
    pub caps: Capabilities,
    device: evdev::Device,
    axis: AxisState,
    b: BState,
    chord: ChordState,
    // Held to keep hid-steam out of lizard mode for the lifetime of
    // the controller. Dropped restores prior value.
    _lizard: LizardGuard,
}

impl std::fmt::Debug for Controller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Controller")
            .field("name", &self.name)
            .field("path", &self.path)
            .field("caps", &self.caps)
            .finish_non_exhaustive()
    }
}

/// Resolved per-device input dispatch choices.
///
/// Drivers vary in whether they expose a trigger as an analog axis, a
/// digital button, or both. To keep the canonical PIN byte sequence
/// stable across drivers we pick exactly one source per logical input
/// at device open and ignore the other to avoid double-emit.
#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)] // boolean caps describe orthogonal evdev features
pub struct Capabilities {
    /// True if `ABS_Z` is supported and is the chosen LT source.
    pub lt_uses_axis: bool,
    /// True if `ABS_RZ` is supported and is the chosen RT source.
    pub rt_uses_axis: bool,
    /// True if `ABS_HAT0X`/`ABS_HAT0Y` is supported and is the chosen
    /// d-pad source.
    pub dpad_uses_hat: bool,
    /// Press threshold for `ABS_Z`, scaled to the reported axis range.
    pub lt_press_threshold: i32,
    /// Release threshold for `ABS_Z`, scaled to the reported axis range.
    pub lt_release_threshold: i32,
    /// Press threshold for `ABS_RZ`, scaled to the reported axis range.
    pub rt_press_threshold: i32,
    /// Release threshold for `ABS_RZ`, scaled to the reported axis range.
    pub rt_release_threshold: i32,
    /// Swap canonical face-button mapping for `BTN_NORTH` and `BTN_WEST`.
    /// Set when the device is a Steam Deck built-in controller running
    /// under Valve's hid-steam fork (in `linux-*-valve1` kernels), which
    /// reports physical Y as `BTN_WEST` and physical X as `BTN_NORTH` —
    /// the inverse of the upstream Linux gamepad convention.
    pub swap_north_west: bool,
}

impl Controller {
    /// Open the first connected device that looks like a gamepad.
    pub fn open_first() -> Result<Self> {
        // Disable hid-steam lizard mode FIRST so the Steam Controller /
        // Deck built-in controller starts emitting gamepad events before
        // we enumerate. The guard is moved into the returned Controller
        // and restores the prior value on drop.
        let lizard = LizardGuard::disable();
        for (path, dev) in evdev::enumerate() {
            if !is_gamepad(&dev) {
                debug!(path = %path.display(), "input: skipping non-gamepad device");
                continue;
            }
            let name = dev.name().unwrap_or("(unnamed)").to_owned();
            let path_s = path.display().to_string();
            let mut caps = probe_capabilities(&dev);
            // Valve's `linux-*-valve1` hid-steam fork swaps BTN_NORTH
            // and BTN_WEST for the Deck's built-in controller relative
            // to the upstream Linux convention. The device name is
            // "Steam Deck" under BOTH kernels (so name alone isn't a
            // valid discriminator), but the kernel release string
            // contains "valve" only on Valve's kernel. Flip the
            // canonical mapping only there so a PIN encoded on a stock
            // kernel and a PIN encoded on a Valve kernel produce the
            // same canonical char sequence for the same physical buttons.
            if name == "Steam Deck" && kernel_is_valve() {
                caps.swap_north_west = true;
            }
            info!(
                path = %path_s, name = %name,
                lt_axis = caps.lt_uses_axis,
                rt_axis = caps.rt_uses_axis,
                dpad_hat = caps.dpad_uses_hat,
                lt_press = caps.lt_press_threshold,
                rt_press = caps.rt_press_threshold,
                swap_nw = caps.swap_north_west,
                "input: opened controller"
            );
            return Ok(Self {
                name,
                path: path_s,
                caps,
                device: dev,
                axis: AxisState::default(),
                b: BState::Idle,
                chord: ChordState::default(),
                _lizard: lizard,
            });
        }
        warn!("input: no gamepad device found");
        Err(Error::NoController)
    }

    /// Block until the next canonical event or the optional timeout elapses.
    /// Returns `Ok(None)` on timeout.
    pub fn next_event(&mut self, timeout: Option<Duration>) -> Result<Option<InputEvent>> {
        loop {
            // If B is held past threshold, surface backspace immediately.
            if let BState::Pressed { at } = self.b {
                if at.elapsed() >= BACKSPACE_HOLD {
                    self.b = BState::ConsumedAsBackspace;
                    return Ok(Some(InputEvent::Backspace));
                }
            }

            let wait_ms = compute_wait(&self.b, timeout);

            let fd = self.device.as_fd();
            let mut pfds = [PollFd::new(&fd, PollFlags::IN)];
            let n = poll(&mut pfds, wait_ms).map_err(|e| Error::Evdev(format!("poll: {e}")))?;
            if n == 0 {
                // Either real timeout, or the B-hold timer fired -> loop and re-check.
                if matches!(self.b, BState::Pressed { .. }) {
                    continue;
                }
                return Ok(None);
            }

            let events: Vec<_> = self
                .device
                .fetch_events()
                .map_err(|e| Error::Evdev(format!("fetch_events: {e}")))?
                .collect();
            for ev in &events {
                // trace level (not debug): the Steam Deck IMU on the
                // built-in controller fires ABS_X/Y/RX/RY at ~250 Hz
                // so this would flood the journal under -vv. Show
                // with RUST_LOG=trace or -vvv when actually debugging
                // a controller mapping.
                tracing::trace!(
                    type_ = ?ev.event_type(),
                    code = ev.code(),
                    value = ev.value(),
                    "input: raw event",
                );
                if let Some(ie) = self.handle_event(ev) {
                    return Ok(Some(ie));
                }
            }
            // No event of interest; loop and wait again.
        }
    }

    fn handle_event(&mut self, ev: &evdev::InputEvent) -> Option<InputEvent> {
        match ev.event_type() {
            evdev::EventType::KEY => self.handle_key(ev.code(), ev.value()),
            evdev::EventType::ABSOLUTE => self.handle_abs(ev.code(), ev.value()),
            _ => None,
        }
    }

    fn handle_key(&mut self, code: u16, value: i32) -> Option<InputEvent> {
        // value: 1 = press, 0 = release, 2 = key-repeat (ignored).
        if value == 2 {
            return None;
        }

        // SELECT + START chord = clean exit. Track both buttons'
        // state and emit Quit once both are held; suppress the
        // normal START=Submit semantics for the actuation that
        // completed the chord so callers don't get a stale Submit
        // followed by Quit.
        if code == BTN_SELECT {
            self.chord.select_held = value == 1;
            if value == 0 {
                self.chord.quit_emitted = false;
            }
            if let Some(q) = self.maybe_quit() {
                return Some(q);
            }
            return None;
        }
        if code == BTN_START {
            self.chord.start_held = value == 1;
            if value == 0 {
                self.chord.quit_emitted = false;
            }
            if let Some(q) = self.maybe_quit() {
                return Some(q);
            }
            // Only surface Submit on press, and only when SELECT
            // isn't part of an in-progress chord.
            if value == 1 && !self.chord.select_held {
                return Some(InputEvent::Submit);
            }
            return None;
        }

        if code == BTN_EAST {
            return self.handle_b_event(value);
        }

        // Suppress digital trigger / d-pad sources when the analog/HAT
        // source is the chosen one for this device. Without this, a
        // driver that emits both would produce two `Press` events per
        // actuation and the canonical PIN byte sequence would diverge
        // from a single-source driver.
        if (code == BTN_TL2 && self.caps.lt_uses_axis)
            || (code == BTN_TR2 && self.caps.rt_uses_axis)
        {
            return None;
        }
        if self.caps.dpad_uses_hat
            && matches!(
                code,
                BTN_DPAD_UP | BTN_DPAD_DOWN | BTN_DPAD_LEFT | BTN_DPAD_RIGHT
            )
        {
            return None;
        }

        if value == 1 {
            return key_to_canonical(code, self.caps.swap_north_west).map(InputEvent::Press);
        }
        None
    }

    fn maybe_quit(&mut self) -> Option<InputEvent> {
        if self.chord.select_held && self.chord.start_held && !self.chord.quit_emitted {
            self.chord.quit_emitted = true;
            return Some(InputEvent::Quit);
        }
        None
    }

    fn handle_b_event(&mut self, value: i32) -> Option<InputEvent> {
        match (value, self.b) {
            (1, _) => {
                self.b = BState::Pressed { at: Instant::now() };
                None
            }
            (0, BState::Pressed { at }) if at.elapsed() < BACKSPACE_HOLD => {
                self.b = BState::Idle;
                Some(InputEvent::Press(CanonicalButton::B))
            }
            (0, _) => {
                self.b = BState::Idle;
                None
            }
            _ => None,
        }
    }

    fn handle_abs(&mut self, code: u16, value: i32) -> Option<InputEvent> {
        match code {
            ABS_Z if self.caps.lt_uses_axis => self.handle_trigger(value, /* left */ true),
            ABS_RZ if self.caps.rt_uses_axis => self.handle_trigger(value, /* left */ false),
            ABS_HAT0X if self.caps.dpad_uses_hat => self.handle_hat_x(value),
            ABS_HAT0Y if self.caps.dpad_uses_hat => self.handle_hat_y(value),
            _ => None,
        }
    }

    fn handle_trigger(&mut self, value: i32, left: bool) -> Option<InputEvent> {
        let (held, button, press, release) = if left {
            (
                &mut self.axis.lt_held,
                CanonicalButton::Lt,
                self.caps.lt_press_threshold,
                self.caps.lt_release_threshold,
            )
        } else {
            (
                &mut self.axis.rt_held,
                CanonicalButton::Rt,
                self.caps.rt_press_threshold,
                self.caps.rt_release_threshold,
            )
        };
        if !*held && value >= press {
            *held = true;
            return Some(InputEvent::Press(button));
        }
        if *held && value <= release {
            *held = false;
        }
        None
    }

    fn handle_hat_x(&mut self, value: i32) -> Option<InputEvent> {
        let prev = self.axis.hat_x;
        let next = value.signum();
        self.axis.hat_x = next;
        match (prev, next) {
            (0, -1) => Some(InputEvent::Press(CanonicalButton::DpadW)),
            (0, 1) => Some(InputEvent::Press(CanonicalButton::DpadE)),
            _ => None,
        }
    }

    fn handle_hat_y(&mut self, value: i32) -> Option<InputEvent> {
        let prev = self.axis.hat_y;
        let next = value.signum();
        self.axis.hat_y = next;
        match (prev, next) {
            (0, -1) => Some(InputEvent::Press(CanonicalButton::DpadN)),
            (0, 1) => Some(InputEvent::Press(CanonicalButton::DpadS)),
            _ => None,
        }
    }
}

fn compute_wait(b: &BState, timeout: Option<Duration>) -> i32 {
    let b_remaining = match b {
        BState::Pressed { at } => Some(BACKSPACE_HOLD.saturating_sub(at.elapsed())),
        _ => None,
    };
    match (b_remaining, timeout) {
        (Some(b), Some(t)) => clamp_ms(b.min(t)),
        (Some(b), None) => clamp_ms(b),
        (None, Some(t)) => clamp_ms(t),
        (None, None) => -1,
    }
}

fn clamp_ms(d: Duration) -> i32 {
    i32::try_from(d.as_millis()).unwrap_or(i32::MAX).max(0)
}

fn kernel_is_valve() -> bool {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .is_ok_and(|s| s.to_ascii_lowercase().contains("valve"))
}

fn is_gamepad(dev: &evdev::Device) -> bool {
    dev.supported_keys()
        .is_some_and(|keys| keys.contains(evdev::KeyCode::new(BTN_SOUTH)))
}

/// Resolve which input source the device exposes for each logical
/// trigger and the d-pad, and scale trigger thresholds to the reported
/// axis range. Falls back to the historical 0..=255 constants when the
/// kernel does not provide an `AbsInfo` (rare; should not happen for
/// standard HID gamepads).
fn probe_capabilities(dev: &evdev::Device) -> Capabilities {
    let abs_axes = dev.supported_absolute_axes();
    let supports_abs = |code: u16| {
        abs_axes.is_some_and(|set| set.contains(evdev::AbsoluteAxisCode(code)))
    };
    let keys = dev.supported_keys();
    let supports_key = |code: u16| {
        keys.is_some_and(|set| set.contains(evdev::KeyCode::new(code)))
    };
    let lt_uses_axis = supports_abs(ABS_Z);
    let rt_uses_axis = supports_abs(ABS_RZ);
    // Prefer digital BTN_DPAD_* over the ABS_HAT0X/Y axes when the
    // device advertises both. hid-steam (Steam Deck built-in
    // controller, Steam Controller) and hid-playstation (DualSense)
    // advertise hat axis support but only ever emit BTN_DPAD_*.
    // Picking the hat in that case suppresses the buttons and the
    // d-pad goes silent. Older Switch Pro / generic HID gamepads
    // without BTN_DPAD_* still get the hat fallback.
    let dpad_buttons_present = supports_key(BTN_DPAD_UP)
        || supports_key(BTN_DPAD_DOWN)
        || supports_key(BTN_DPAD_LEFT)
        || supports_key(BTN_DPAD_RIGHT);
    let dpad_uses_hat = !dpad_buttons_present
        && (supports_abs(ABS_HAT0X) || supports_abs(ABS_HAT0Y));

    let abs_info: Vec<(evdev::AbsoluteAxisCode, evdev::AbsInfo)> = dev
        .get_absinfo()
        .map(Iterator::collect)
        .unwrap_or_default();
    let range_for = |code: u16| -> Option<(i32, i32)> {
        abs_info
            .iter()
            .find(|(c, _)| c.0 == code)
            .map(|(_, info)| (info.minimum(), info.maximum()))
    };
    let fallback = (TRIGGER_PRESS_THRESHOLD, TRIGGER_RELEASE_THRESHOLD);
    let (lt_press, lt_release) = range_for(ABS_Z)
        .map_or(fallback, |(lo, hi)| scale_trigger_thresholds(lo, hi));
    let (rt_press, rt_release) = range_for(ABS_RZ)
        .map_or(fallback, |(lo, hi)| scale_trigger_thresholds(lo, hi));

    Capabilities {
        lt_uses_axis,
        rt_uses_axis,
        dpad_uses_hat,
        lt_press_threshold: lt_press,
        lt_release_threshold: lt_release,
        rt_press_threshold: rt_press,
        rt_release_threshold: rt_release,
        // Caller (Controller::open_first) flips this on for "Steam Deck"
        // devices; probe_capabilities has no name context.
        swap_north_west: false,
    }
}

/// `luks-controller-unlock test-input`: print canonical events as they arrive.
pub fn run_test() -> Result<()> {
    let mut ctrl = Controller::open_first()?;
    info!(
        "test-input: listening on '{}' ({}). SELECT+START to exit (Ctrl-C also works).",
        ctrl.name, ctrl.path
    );
    loop {
        match ctrl.next_event(None)? {
            Some(InputEvent::Press(b)) => info!("press: {} ('{}')", b.label(), b.as_char() as char),
            Some(InputEvent::Submit) => info!("submit (START)"),
            Some(InputEvent::Backspace) => info!("backspace (B held)"),
            Some(InputEvent::Quit) => {
                info!("quit (SELECT+START)");
                return Ok(());
            }
            None => {}
        }
    }
}
