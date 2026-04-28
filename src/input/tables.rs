//! evdev event-code → CanonicalButton mapping tables, one per supported
//! controller family. Dispatch is by (vendor, product) read from the
//! evdev device's input_id.

use crate::pin::CanonicalButton;

#[derive(Debug, Clone, Copy)]
pub struct ControllerFamily {
    pub name: &'static str,
    pub vendor: u16,
    pub product: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct ButtonMapping {
    pub evdev_code: u16,
    pub button: CanonicalButton,
}

/// Submit and backspace are out-of-band: not mapped to a CanonicalButton.
#[derive(Debug, Clone, Copy)]
pub struct ControlMapping {
    pub submit_evdev_code: u16,
    pub backspace_evdev_code: u16,
}
