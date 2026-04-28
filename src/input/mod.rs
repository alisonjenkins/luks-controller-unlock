//! Controller input via evdev.
//!
//! v1: single-device. Opens the first device on `/dev/input/event*` that
//! advertises BTN_SOUTH. Multi-device + udev hot-plug is a follow-up.
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

pub mod tables;

use tables::{
    ABS_HAT0X, ABS_HAT0Y, ABS_RZ, ABS_Z, BTN_EAST, BTN_SOUTH, BTN_START,
    TRIGGER_PRESS_THRESHOLD, TRIGGER_RELEASE_THRESHOLD, key_to_canonical,
};

/// Hold time after which BTN_EAST counts as backspace instead of B.
pub const BACKSPACE_HOLD: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    /// A canonical button was committed (B-taps surface here too).
    Press(CanonicalButton),
    /// START was pressed: caller should submit the current PIN.
    Submit,
    /// B was held past the backspace threshold.
    Backspace,
}

#[derive(Debug, Default)]
struct AxisState {
    lt_held: bool,
    rt_held: bool,
    hat_x: i32, // -1 / 0 / +1
    hat_y: i32,
}

#[derive(Debug, Clone, Copy)]
enum BState {
    Idle,
    Pressed { at: Instant },
    ConsumedAsBackspace,
}

#[derive(Debug)]
pub struct Controller {
    pub name: String,
    pub path: String,
    device: evdev::Device,
    axis: AxisState,
    b: BState,
}

impl Controller {
    /// Open the first connected device that looks like a gamepad.
    pub fn open_first() -> Result<Self> {
        for (path, dev) in evdev::enumerate() {
            if !is_gamepad(&dev) {
                debug!(path = %path.display(), "input: skipping non-gamepad device");
                continue;
            }
            let name = dev.name().unwrap_or("(unnamed)").to_owned();
            let path_s = path.display().to_string();
            info!(path = %path_s, name = %name, "input: opened controller");
            return Ok(Self {
                name,
                path: path_s,
                device: dev,
                axis: AxisState::default(),
                b: BState::Idle,
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

        if code == BTN_START {
            return (value == 1).then_some(InputEvent::Submit);
        }

        if code == BTN_EAST {
            return self.handle_b_event(value);
        }

        if value == 1 {
            return key_to_canonical(code).map(InputEvent::Press);
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
            ABS_Z => self.handle_trigger(value, /* left */ true),
            ABS_RZ => self.handle_trigger(value, /* left */ false),
            ABS_HAT0X => self.handle_hat_x(value),
            ABS_HAT0Y => self.handle_hat_y(value),
            _ => None,
        }
    }

    fn handle_trigger(&mut self, value: i32, left: bool) -> Option<InputEvent> {
        let (held, button) = if left {
            (&mut self.axis.lt_held, CanonicalButton::Lt)
        } else {
            (&mut self.axis.rt_held, CanonicalButton::Rt)
        };
        if !*held && value >= TRIGGER_PRESS_THRESHOLD {
            *held = true;
            return Some(InputEvent::Press(button));
        }
        if *held && value <= TRIGGER_RELEASE_THRESHOLD {
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

fn is_gamepad(dev: &evdev::Device) -> bool {
    dev.supported_keys()
        .is_some_and(|keys| keys.contains(evdev::KeyCode::new(BTN_SOUTH)))
}

/// `luks-controller-unlock test-input`: print canonical events as they arrive.
pub fn run_test() -> Result<()> {
    let mut ctrl = Controller::open_first()?;
    info!(
        "test-input: listening on '{}' ({}). Press Ctrl-C to exit.",
        ctrl.name, ctrl.path
    );
    loop {
        match ctrl.next_event(None)? {
            Some(InputEvent::Press(b)) => info!("press: {} ('{}')", b.label(), b.as_char() as char),
            Some(InputEvent::Submit) => info!("submit (START)"),
            Some(InputEvent::Backspace) => info!("backspace (B held)"),
            None => {}
        }
    }
}
