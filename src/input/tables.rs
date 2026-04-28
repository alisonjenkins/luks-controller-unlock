//! evdev event-code constants and mapping to canonical buttons.
//!
//! Codes follow `linux/input-event-codes.h`. The mapping is generic for
//! all controllers that present themselves as a standard gamepad through
//! the kernel (Xbox via xpad, DualShock 4 via hid-sony, DualSense via
//! hid-playstation, Switch Pro via hid-nintendo, 8BitDo Pro 2 via xpad,
//! Steam Controller via hid-steam). Per-controller overrides can be
//! added later without changing the Pin alphabet.
//!
//! Submit and backspace are intentionally NOT canonical buttons — they
//! are control inputs and live outside the alphabet.

use crate::pin::CanonicalButton;

// Key codes (linux/input-event-codes.h).
pub const BTN_SOUTH: u16 = 0x130; // A
pub const BTN_EAST: u16 = 0x131; // B (also = backspace on hold)
pub const BTN_NORTH: u16 = 0x133; // Y (gamepad north = Y)
pub const BTN_WEST: u16 = 0x134; // X
pub const BTN_TL: u16 = 0x136; // LB
pub const BTN_TR: u16 = 0x137; // RB
pub const BTN_TL2: u16 = 0x138; // LT (some controllers report as button)
pub const BTN_TR2: u16 = 0x139; // RT (some controllers report as button)
pub const BTN_SELECT: u16 = 0x13a;
pub const BTN_START: u16 = 0x13b; // SUBMIT
pub const BTN_MODE: u16 = 0x13c;
pub const BTN_THUMBL: u16 = 0x13d;
pub const BTN_THUMBR: u16 = 0x13e;
pub const BTN_DPAD_UP: u16 = 0x220;
pub const BTN_DPAD_DOWN: u16 = 0x221;
pub const BTN_DPAD_LEFT: u16 = 0x222;
pub const BTN_DPAD_RIGHT: u16 = 0x223;

// Absolute axis codes.
pub const ABS_Z: u16 = 0x02; // LT analog
pub const ABS_RZ: u16 = 0x05; // RT analog
pub const ABS_HAT0X: u16 = 0x10; // dpad horizontal: -1 = left, +1 = right
pub const ABS_HAT0Y: u16 = 0x11; // dpad vertical:   -1 = up,   +1 = down

/// Trigger-axis press threshold. Triggers report 0..=255 on most controllers.
pub const TRIGGER_PRESS_THRESHOLD: i32 = 64;
/// Hysteresis for trigger release so a held trigger doesn't chatter.
pub const TRIGGER_RELEASE_THRESHOLD: i32 = 32;

/// Map a KEY_* code to a canonical button, if any.
///
/// Returns `None` for keys that are control inputs (START, BTN_EAST when
/// the caller is implementing the B-hold-for-backspace state machine), or
/// for keys that are not part of the canonical alphabet.
pub fn key_to_canonical(code: u16) -> Option<CanonicalButton> {
    match code {
        BTN_SOUTH => Some(CanonicalButton::A),
        BTN_EAST => Some(CanonicalButton::B),
        BTN_WEST => Some(CanonicalButton::X),
        BTN_NORTH => Some(CanonicalButton::Y),
        BTN_TL => Some(CanonicalButton::Lb),
        BTN_TR => Some(CanonicalButton::Rb),
        BTN_TL2 => Some(CanonicalButton::Lt),
        BTN_TR2 => Some(CanonicalButton::Rt),
        BTN_DPAD_UP => Some(CanonicalButton::DpadN),
        BTN_DPAD_DOWN => Some(CanonicalButton::DpadS),
        BTN_DPAD_LEFT => Some(CanonicalButton::DpadW),
        BTN_DPAD_RIGHT => Some(CanonicalButton::DpadE),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn standard_face_buttons_map() {
        assert_eq!(key_to_canonical(BTN_SOUTH), Some(CanonicalButton::A));
        assert_eq!(key_to_canonical(BTN_EAST), Some(CanonicalButton::B));
        assert_eq!(key_to_canonical(BTN_WEST), Some(CanonicalButton::X));
        assert_eq!(key_to_canonical(BTN_NORTH), Some(CanonicalButton::Y));
    }

    #[test]
    fn shoulders_and_triggers_map() {
        assert_eq!(key_to_canonical(BTN_TL), Some(CanonicalButton::Lb));
        assert_eq!(key_to_canonical(BTN_TR), Some(CanonicalButton::Rb));
        assert_eq!(key_to_canonical(BTN_TL2), Some(CanonicalButton::Lt));
        assert_eq!(key_to_canonical(BTN_TR2), Some(CanonicalButton::Rt));
    }

    #[test]
    fn dpad_buttons_map() {
        assert_eq!(key_to_canonical(BTN_DPAD_UP), Some(CanonicalButton::DpadN));
        assert_eq!(key_to_canonical(BTN_DPAD_DOWN), Some(CanonicalButton::DpadS));
        assert_eq!(key_to_canonical(BTN_DPAD_LEFT), Some(CanonicalButton::DpadW));
        assert_eq!(key_to_canonical(BTN_DPAD_RIGHT), Some(CanonicalButton::DpadE));
    }

    #[test]
    fn submit_is_not_canonical() {
        assert_eq!(key_to_canonical(BTN_START), None);
    }

    #[test]
    fn unknown_keys_unmapped() {
        for code in [0u16, 1, 0x100, 0x140, 0xfff] {
            assert_eq!(key_to_canonical(code), None);
        }
    }
}
