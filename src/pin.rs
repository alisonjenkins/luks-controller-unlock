//! Canonical controller button alphabet and PIN encoding.
//!
//! A PIN is a sequence of canonical buttons. The canonical alphabet is the
//! set of buttons present on every supported controller (face, shoulders,
//! triggers as digital, dpad). Each button maps to a single ASCII byte; a
//! PIN encodes deterministically to a passphrase that is registered with
//! `cryptsetup luksAddKey`. The same sequence on any controller yields the
//! same passphrase.

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CanonicalButton {
    A = b'a',
    B = b'b',
    X = b'c',
    Y = b'd',
    Lb = b'e',
    Rb = b'f',
    Lt = b'g',
    Rt = b'h',
    DpadN = b'i',
    DpadS = b'j',
    DpadE = b'k',
    DpadW = b'l',
}

impl CanonicalButton {
    #[allow(dead_code)]
    pub const ALL: [Self; 12] = [
        Self::A,
        Self::B,
        Self::X,
        Self::Y,
        Self::Lb,
        Self::Rb,
        Self::Lt,
        Self::Rt,
        Self::DpadN,
        Self::DpadS,
        Self::DpadE,
        Self::DpadW,
    ];

    #[inline]
    pub const fn as_char(self) -> u8 {
        self as u8
    }

    #[allow(dead_code)]
    pub const fn try_from_char(c: u8) -> Result<Self> {
        match c {
            b'a' => Ok(Self::A),
            b'b' => Ok(Self::B),
            b'c' => Ok(Self::X),
            b'd' => Ok(Self::Y),
            b'e' => Ok(Self::Lb),
            b'f' => Ok(Self::Rb),
            b'g' => Ok(Self::Lt),
            b'h' => Ok(Self::Rt),
            b'i' => Ok(Self::DpadN),
            b'j' => Ok(Self::DpadS),
            b'k' => Ok(Self::DpadE),
            b'l' => Ok(Self::DpadW),
            _ => Err(Error::UnknownButton),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::X => "X",
            Self::Y => "Y",
            Self::Lb => "LB",
            Self::Rb => "RB",
            Self::Lt => "LT",
            Self::Rt => "RT",
            Self::DpadN => "Up",
            Self::DpadS => "Down",
            Self::DpadE => "Right",
            Self::DpadW => "Left",
        }
    }
}

pub const MAX_PIN_LEN: usize = 256;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Pin {
    buttons: Vec<CanonicalButton>,
}

impl Pin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, button: CanonicalButton) -> Result<()> {
        if self.buttons.len() >= MAX_PIN_LEN {
            return Err(Error::PinTooLong { max: MAX_PIN_LEN });
        }
        self.buttons.push(button);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<CanonicalButton> {
        self.buttons.pop()
    }

    pub fn clear(&mut self) {
        self.buttons.clear();
    }

    pub fn len(&self) -> usize {
        self.buttons.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buttons.is_empty()
    }

    #[allow(dead_code)]
    pub fn buttons(&self) -> &[CanonicalButton] {
        &self.buttons
    }

    /// Encode to LUKS passphrase bytes. Deterministic across controllers.
    pub fn to_passphrase(&self) -> Vec<u8> {
        self.buttons.iter().map(|b| b.as_char()).collect()
    }

    #[allow(dead_code)]
    pub fn from_passphrase(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_PIN_LEN {
            return Err(Error::PinTooLong { max: MAX_PIN_LEN });
        }
        let mut buttons = Vec::with_capacity(bytes.len());
        for &c in bytes {
            buttons.push(CanonicalButton::try_from_char(c)?);
        }
        Ok(Self { buttons })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn canonical_chars_are_distinct() {
        let mut chars: Vec<u8> = CanonicalButton::ALL.iter().map(|b| b.as_char()).collect();
        chars.sort_unstable();
        chars.dedup();
        assert_eq!(chars.len(), CanonicalButton::ALL.len());
    }

    #[test]
    fn canonical_chars_lowercase_ascii() {
        for b in CanonicalButton::ALL {
            assert!(b.as_char().is_ascii_lowercase());
        }
    }

    #[test]
    fn try_from_char_round_trip() {
        for b in CanonicalButton::ALL {
            assert_eq!(CanonicalButton::try_from_char(b.as_char()).unwrap(), b);
        }
    }

    #[test]
    fn try_from_char_rejects_unknown() {
        for c in [b'm', b'A', b'!', 0u8, 255u8] {
            assert!(CanonicalButton::try_from_char(c).is_err());
        }
    }

    #[test]
    fn pin_push_pop_len() {
        let mut p = Pin::new();
        assert!(p.is_empty());
        p.push(CanonicalButton::A).unwrap();
        p.push(CanonicalButton::B).unwrap();
        p.push(CanonicalButton::X).unwrap();
        assert_eq!(p.len(), 3);
        assert_eq!(p.pop(), Some(CanonicalButton::X));
        assert_eq!(p.len(), 2);
        p.clear();
        assert!(p.is_empty());
    }

    #[test]
    fn pin_length_cap_enforced() {
        let mut p = Pin::new();
        for _ in 0..MAX_PIN_LEN {
            p.push(CanonicalButton::A).unwrap();
        }
        assert_eq!(p.len(), MAX_PIN_LEN);
        let err = p.push(CanonicalButton::A).unwrap_err();
        assert!(matches!(err, Error::PinTooLong { max } if max == MAX_PIN_LEN));
    }

    #[test]
    fn passphrase_round_trip() {
        let mut p = Pin::new();
        for b in [
            CanonicalButton::DpadN,
            CanonicalButton::Lt,
            CanonicalButton::A,
            CanonicalButton::B,
            CanonicalButton::Y,
            CanonicalButton::Rb,
        ] {
            p.push(b).unwrap();
        }
        let pass = p.to_passphrase();
        assert_eq!(pass.len(), 6);
        let restored = Pin::from_passphrase(&pass).unwrap();
        assert_eq!(restored, p);
    }

    #[test]
    fn from_passphrase_rejects_oversized() {
        let big = vec![b'a'; MAX_PIN_LEN + 1];
        let err = Pin::from_passphrase(&big).unwrap_err();
        assert!(matches!(err, Error::PinTooLong { .. }));
    }

    #[test]
    fn from_passphrase_rejects_invalid_char() {
        let bad = b"abcm";
        assert!(Pin::from_passphrase(bad).is_err());
    }

    #[test]
    fn passphrase_is_deterministic() {
        let mut p1 = Pin::new();
        let mut p2 = Pin::new();
        let seq = [
            CanonicalButton::DpadE,
            CanonicalButton::DpadE,
            CanonicalButton::A,
            CanonicalButton::Rt,
        ];
        for b in seq {
            p1.push(b).unwrap();
            p2.push(b).unwrap();
        }
        assert_eq!(p1.to_passphrase(), p2.to_passphrase());
    }
}
