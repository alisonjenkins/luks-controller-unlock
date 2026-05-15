//! `luks-controller-unlock gen`: pwgen-style candidate generator for
//! controller PINs. Each candidate is a length-N sequence drawn
//! uniformly from the 12-button canonical alphabet, printed both as
//! the encoded char form and as the button-label sequence the user
//! would actually press at the unlock prompt. Pick one you can
//! remember, feed it to `enroll`, done.
//!
//! Entropy: log2(12) ≈ 3.585 bits per button. A 12-button PIN is
//! ~43 bits, 16-button ~57, 20-button ~72. The header line in the
//! output reports it.

use std::io::{Read, Write, stdout};

use clap::Args;
use tracing::warn;

use crate::error::{Error, Result};
use crate::pin::{CanonicalButton, MAX_PIN_LEN};

const ALPHABET_SIZE: u8 = 12;
// Largest multiple of ALPHABET_SIZE that fits in a u8: 252 = 21 * 12.
// Bytes >= REJECT are discarded so the modulo is unbiased.
const REJECT: u8 = 252;

#[derive(Args, Debug)]
pub struct GenArgs {
    /// Number of buttons in each candidate PIN.
    #[arg(short = 'l', long, default_value_t = 12)]
    pub length: usize,

    /// Number of candidates to print.
    #[arg(short = 'n', long, default_value_t = 20)]
    pub count: usize,

    /// Suppress the entropy / hint header.
    #[arg(long)]
    pub quiet: bool,
}

pub fn run(args: &GenArgs) -> Result<()> {
    if args.length == 0 || args.length > MAX_PIN_LEN {
        return Err(Error::Config(format!(
            "length must be between 1 and {MAX_PIN_LEN}",
        )));
    }
    if args.count == 0 {
        return Err(Error::Config("count must be >= 1".into()));
    }

    let bits_per = (f64::from(ALPHABET_SIZE)).log2();
    // Length is bounded by MAX_PIN_LEN (256), well inside f64 mantissa.
    #[allow(clippy::cast_precision_loss)]
    let total_bits = bits_per * args.length as f64;

    let mut urandom = std::fs::File::open("/dev/urandom")
        .map_err(|e| Error::Io(std::io::Error::new(e.kind(), format!("/dev/urandom: {e}"))))?;
    let mut out = stdout().lock();

    if !args.quiet {
        writeln!(
            out,
            "# {} candidates x {} buttons (~{:.1} bits entropy each)",
            args.count, args.length, total_bits,
        )?;
        writeln!(
            out,
            "# format: encoded-pin                    button sequence",
        )?;
        writeln!(out)?;
    }

    let pin_field_width = encoded_field_width(args.length);
    for _ in 0..args.count {
        let buttons = sample(&mut urandom, args.length)?;
        let encoded: String = buttons
            .iter()
            .map(|b| char::from(b.as_char()))
            .collect();
        let labels: Vec<&'static str> = buttons.iter().map(|b| b.label()).collect();
        writeln!(
            out,
            "{:width$}  {}",
            chunked(&encoded, 4),
            labels.join(" "),
            width = pin_field_width,
        )?;
    }
    Ok(())
}

const fn encoded_field_width(length: usize) -> usize {
    // chunked() inserts a '-' every 4 chars (length-1)/4 separators.
    if length == 0 {
        0
    } else {
        length + (length - 1) / 4
    }
}

fn chunked(s: &str, group: usize) -> String {
    if group == 0 {
        return s.to_owned();
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(bytes.len() + bytes.len() / group);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 && i % group == 0 {
            out.push('-');
        }
        out.push(char::from(b));
    }
    out
}

fn sample<R: Read>(rng: &mut R, n: usize) -> Result<Vec<CanonicalButton>> {
    let mut out: Vec<CanonicalButton> = Vec::with_capacity(n);
    let mut buf = [0u8; 64];
    while out.len() < n {
        rng.read_exact(&mut buf)
            .map_err(|e| Error::Io(std::io::Error::new(e.kind(), format!("/dev/urandom: {e}"))))?;
        for &b in &buf {
            if out.len() >= n {
                break;
            }
            if b >= REJECT {
                continue;
            }
            // Map 0..12 to 'a'..'l' and convert via the canonical
            // mapping so we share the encoding with PIN parsing.
            let idx = b % ALPHABET_SIZE;
            let c = b'a' + idx;
            if let Ok(btn) = CanonicalButton::try_from_char(c) {
                out.push(btn);
            } else {
                warn!("gen: unexpected unmapped char {c}");
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn rejects_zero_length() {
        let args = GenArgs {
            length: 0,
            count: 1,
            quiet: true,
        };
        assert!(run(&args).is_err());
    }

    #[test]
    fn rejects_zero_count() {
        let args = GenArgs {
            length: 4,
            count: 0,
            quiet: true,
        };
        assert!(run(&args).is_err());
    }

    #[test]
    fn sample_yields_requested_length() {
        // 64 zero bytes -> 64 'a's; we ask for 5 -> first 5 'a's.
        let mut rng = Cursor::new(vec![0u8; 64]);
        let v = sample(&mut rng, 5).unwrap();
        assert_eq!(v.len(), 5);
        assert!(v.iter().all(|b| *b == CanonicalButton::A));
    }

    #[test]
    fn sample_skips_rejected_bytes() {
        // First 64 bytes all = 252 (REJECT) -> all rejected.
        // Second 64 bytes all = 1 -> 'b' = CanonicalButton::B.
        let mut bytes = vec![REJECT; 64];
        bytes.extend(std::iter::repeat_n(1u8, 64));
        let mut rng = Cursor::new(bytes);
        let v = sample(&mut rng, 3).unwrap();
        assert_eq!(v.len(), 3);
        assert!(v.iter().all(|b| *b == CanonicalButton::B));
    }

    #[test]
    fn chunked_groups_of_four() {
        assert_eq!(chunked("abcdefgh", 4), "abcd-efgh");
        assert_eq!(chunked("abc", 4), "abc");
        assert_eq!(chunked("", 4), "");
        assert_eq!(chunked("abcdefghi", 4), "abcd-efgh-i");
    }

    #[test]
    fn encoded_field_width_matches_chunked() {
        for len in [1usize, 4, 5, 8, 9, 12, 16, 17, 20] {
            let s: String = std::iter::repeat_n('a', len).collect();
            assert_eq!(chunked(&s, 4).len(), encoded_field_width(len), "len={len}");
        }
    }
}
