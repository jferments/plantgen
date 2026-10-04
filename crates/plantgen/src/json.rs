//! JSON whose numbers every reader reads back exactly.
//!
//! `serde_json` without its `float_roundtrip` feature reads a number by
//! turning its decimal digits into an integer, converting that to a double
//! and making one multiplication or division by a power of ten. The result
//! is the correctly rounded value of the decimal when the integer is below
//! 2⁵³ and the power is at most 10²², because then both operands are exact.
//! The shortest text for a full-precision double can need 17 digits, and
//! then the read can land one unit in the last place away. The workspace
//! leaves `float_roundtrip` off: Cargo would turn it on for every crate
//! built alongside this one, changing how they parse numbers and splitting
//! the build of everything that depends on `serde_json`, Bevy included.
//!
//! So plant packages write doubles with at most 15 significant digits and a
//! power of ten no further than 10²² from the digits ([`exact_decimal`]).
//! Any reader, fast or exact, reads such a number as the same double, and
//! writing that double again gives the same text:
//!
//! ```text
//! write(x) = d            15 significant digits, |power| ≤ 22
//! read(d)  = x'           the double nearest to d, |x' − x| ≤ 5·10⁻¹⁵ |x|
//! write(x') = d           x' lies far closer to d than to any other
//!                         15-digit decimal
//! ```
//!
//! Single-precision numbers keep their shortest form, which has at most 9
//! digits, whenever reading it as a double, fast or correctly rounded, and
//! rounding that to single precision gives the same number; otherwise they
//! are written like a double ([`exact_decimal_f32`]).

use std::io;

use serde::Serialize;
use serde_json::ser::{CompactFormatter, Formatter, PrettyFormatter};

/// Significant digits written for a double.
pub const DIGITS: i32 = 15;

/// The largest power of ten `serde_json` holds exactly.
const MAX_POWER: i32 = 22;

/// Integers up to this are exact doubles.
const MAX_EXACT: u64 = 1 << 53;

/// `value` as a JSON number that reads back as the double nearest to its
/// [`DIGITS`]-digit rounding, or `None` if no such number has an exact
/// power of ten (magnitudes of about 10³⁸ and more). Magnitudes below
/// 10⁻⁸ keep fewer digits, the last at 10⁻²² or above, and magnitudes
/// below 10⁻²² are written as zero, keeping the sign.
#[must_use]
pub fn exact_decimal(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let magnitude = value.abs();
    // The leading digit's power of ten, after rounding to every digit.
    let (_, leading) = rounded(magnitude, DIGITS)?;
    // Keep the last digit's power at 10⁻²² or above.
    let count = DIGITS.min(leading + MAX_POWER + 1);
    if magnitude == 0.0 || count < 1 {
        return Some(format!("{sign}0.0"));
    }
    let (mut digits, leading) = rounded(magnitude, count)?;
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    // The number is digits × 10^power; move a large power into the digits
    // while they stay exact.
    let mut power = leading - (i32::try_from(digits.len()).ok()? - 1);
    while power > MAX_POWER {
        let widened: u64 = format!("{digits}0").parse().ok()?;
        if widened > MAX_EXACT {
            return None;
        }
        digits.push('0');
        power -= 1;
    }
    Some(format!("{sign}{}", render(&digits, leading)))
}

/// `value` as a JSON number that every reader turns back into the same
/// `f32`: its shortest form when reading that as a double and rounding to
/// single precision gives `value` again, else the double's
/// [`exact_decimal`].
#[must_use]
pub fn exact_decimal_f32(value: f32) -> Option<String> {
    let double = f64::from(value);
    if !value.is_finite() || value == 0.0 {
        return exact_decimal(double);
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    // `{:e}` gives the shortest digits that identify the f32.
    let (digits, leading) = split(&format!("{:e}", value.abs()))?;
    let text = format!("{sign}{}", render(&digits, leading));
    if reads_as(&text, value) {
        Some(text)
    } else {
        exact_decimal(double)
    }
}

/// Whether both ways of reading `text` as a double and rounding that to
/// single precision give `value`: `serde_json`'s fast reading
/// ([`fast_read`]) and the correctly rounded one. A reader that rounds the
/// decimal straight to single precision gets `value` from its shortest
/// form anyway.
fn reads_as(text: &str, value: f32) -> bool {
    let (Some(fast), Ok(correct)) = (fast_read(text), text.parse::<f64>()) else {
        return false;
    };
    // Rounding to single precision is the point.
    #[allow(clippy::cast_possible_truncation)]
    let same = |double: f64| (double as f32).to_bits() == value.to_bits();
    same(fast) && same(correct)
}

/// How `serde_json` without `float_roundtrip` reads a number we wrote: its
/// digits as one integer converted to a double, then multiplied or divided
/// by the correctly rounded power of ten.
fn fast_read(text: &str) -> Option<f64> {
    let (sign, text) = match text.strip_prefix('-') {
        Some(magnitude) => (-1.0, magnitude),
        None => (1.0, text),
    };
    let (mantissa, exponent) = text.split_once('e').unwrap_or((text, "0"));
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits: u64 = format!("{whole}{fraction}").parse().ok()?;
    let power = exponent.parse::<i32>().ok()? - i32::try_from(fraction.len()).ok()?;
    let scale: f64 = format!("1e{}", power.unsigned_abs()).parse().ok()?;
    // Converting the digits is the first of the reader's two roundings.
    #[allow(clippy::cast_precision_loss)]
    let significand = digits as f64;
    let magnitude = if power >= 0 {
        significand * scale
    } else {
        significand / scale
    };
    Some(sign * magnitude)
}

/// The digits of `magnitude` rounded to `count` significant digits, and
/// the power of ten of the first.
fn rounded(magnitude: f64, count: i32) -> Option<(String, i32)> {
    let decimals = usize::try_from(count.max(1) - 1).ok()?;
    split(&format!("{magnitude:.decimals$e}"))
}

/// The digits and leading power of ten of `{:e}` output.
fn split(text: &str) -> Option<(String, i32)> {
    let (mantissa, exponent) = text.split_once('e')?;
    let digits = mantissa.chars().filter(char::is_ascii_digit).collect();
    Some((digits, exponent.parse().ok()?))
}

/// The number whose significant digits are `digits`, the first of them at
/// `10^leading`: positional for moderate magnitudes, scientific beyond
/// them. A reader takes every written digit, trailing zeros included, as
/// one integer.
fn render(digits: &str, leading: i32) -> String {
    let length = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    if (-7..15).contains(&leading) {
        return plain(digits, leading, leading - (length - 1));
    }
    let (first, rest) = digits.split_at(1);
    if rest.is_empty() {
        format!("{first}e{leading}")
    } else {
        format!("{first}.{rest}e{leading}")
    }
}

/// Positional notation: `0.00012`, `3.5`, `120.0`.
fn plain(digits: &str, leading: i32, power: i32) -> String {
    if power >= 0 {
        let zeros = usize::try_from(power).unwrap_or(0);
        return format!("{digits}{}.0", "0".repeat(zeros));
    }
    if leading < 0 {
        let zeros = usize::try_from(-leading - 1).unwrap_or(0);
        return format!("0.{}{digits}", "0".repeat(zeros));
    }
    let point = usize::try_from(leading + 1).unwrap_or(0);
    let (whole, fraction) = digits.split_at(point.min(digits.len()));
    format!("{whole}.{fraction}")
}

/// A `serde_json` formatter that writes numbers as [`exact_decimal`]s and
/// [`exact_decimal_f32`]s, and everything else as `F` does.
struct Exact<F>(F);

macro_rules! delegate {
    ($($name:ident($($argument:ident: $kind:ty),*);)*) => {
        $(
            fn $name<W>(&mut self, writer: &mut W $(, $argument: $kind)*) -> io::Result<()>
            where
                W: ?Sized + io::Write,
            {
                self.0.$name(writer $(, $argument)*)
            }
        )*
    };
}

impl<F: Formatter> Formatter for Exact<F> {
    fn write_f64<W>(&mut self, writer: &mut W, value: f64) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        let text = exact_decimal(value).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{value:e} is too large to write exactly"),
            )
        })?;
        writer.write_all(text.as_bytes())
    }

    fn write_f32<W>(&mut self, writer: &mut W, value: f32) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        let text = exact_decimal_f32(value).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{value:e} is too large to write exactly"),
            )
        })?;
        writer.write_all(text.as_bytes())
    }

    delegate! {
        begin_array();
        end_array();
        begin_array_value(first: bool);
        end_array_value();
        begin_object();
        end_object();
        begin_object_key(first: bool);
        end_object_key();
        begin_object_value();
        end_object_value();
    }
}

fn write<T, F>(value: &T, formatter: F) -> Result<Vec<u8>, String>
where
    T: Serialize + ?Sized,
    F: Formatter,
{
    let mut bytes = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, Exact(formatter));
    value
        .serialize(&mut serializer)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

/// Compact JSON with exact numbers.
///
/// # Errors
///
/// Fails if the value cannot be serialised or holds a number too large to
/// write exactly.
pub fn to_vec<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, String> {
    write(value, CompactFormatter)
}

/// Pretty JSON, laid out as `serde_json::to_vec_pretty` lays it out, with
/// exact numbers.
///
/// # Errors
///
/// As [`to_vec`].
pub fn to_vec_pretty<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, String> {
    write(value, PrettyFormatter::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::{hash_words, unit};

    /// `serde_json`'s reading of `text`, which is the fast one unless
    /// another crate in the build turns on `float_roundtrip`.
    fn read(text: &str) -> f64 {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn short_decimals_are_written_as_people_write_them() {
        let cases = [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (0.1, "0.1"),
            (-2.5, "-2.5"),
            (120.0, "120.0"),
            (0.000_12, "0.00012"),
            (1.5e-8, "1.5e-8"),
            (2.0e20, "2e20"),
            (123_456_789_012_345.0, "123456789012345.0"),
            (1.0 / 3.0, "0.333333333333333"),
            (1e-23, "0.0"),
            (-1e-23, "-0.0"),
        ];
        for (value, text) in cases {
            assert_eq!(exact_decimal(value).as_deref(), Some(text), "{value:e}");
        }
        assert_eq!(exact_decimal(f64::NAN), None);
        assert_eq!(exact_decimal(f64::INFINITY), None);
    }

    #[test]
    fn every_reader_reads_the_same_double_and_writing_it_again_gives_the_same_text() {
        let mut checked = 0;
        for power in -24_i32..40 {
            for sample in 0..400 {
                let draw = |part| {
                    let power = u64::from((power + 100).unsigned_abs());
                    unit(hash_words(&[0x5eed, power, sample, part]))
                };
                let value = (draw(0) * 9.0 + 1.0) * libm::pow(10.0, f64::from(power));
                let value = if draw(1) < 0.5 { -value } else { value };
                let Some(text) = exact_decimal(value) else {
                    assert!(value.abs() >= 1e37, "{value:e} has no exact form");
                    continue;
                };
                // The fast reading is the correctly rounded one, and it
                // writes back as the same text.
                let fast = read(&text);
                let correct: f64 = text.parse().unwrap();
                assert_eq!(fast.to_bits(), correct.to_bits(), "{text}");
                assert_eq!(exact_decimal(fast).as_deref(), Some(text.as_str()));
                let magnitude = value.abs();
                if magnitude >= 1e-8 {
                    // All 15 digits.
                    assert!(
                        (fast - value).abs() <= magnitude * 5e-15,
                        "{value:e} → {text}"
                    );
                } else if magnitude >= 1e-22 {
                    // Digits down to 10⁻²².
                    assert!((fast - value).abs() <= 1e-22, "{value:e} → {text}");
                } else {
                    assert_eq!(text.trim_start_matches('-'), "0.0");
                }
                checked += 1;
            }
        }
        assert!(checked > 20_000);
    }

    #[test]
    fn single_precision_numbers_keep_their_shortest_form() {
        for (value, text) in [
            (0.13_f32, "0.13"),
            (-0.085, "-0.085"),
            (1.0, "1.0"),
            (0.0, "0.0"),
            (3.0e-12, "3e-12"),
            (16_777_216.0, "16777216.0"),
        ] {
            assert_eq!(exact_decimal_f32(value).as_deref(), Some(text), "{value:e}");
        }
        assert_eq!(exact_decimal_f32(f32::NAN), None);
        // Finite f32s sampled across their bit patterns read back as
        // themselves, and writing what was read gives the same text. Only a
        // magnitude below 10⁻⁸ whose shortest form some reader would round
        // differently may lose digits, as a double does.
        let (mut checked, mut tiny, mut tiny_exact) = (0, 0, 0);
        for sample in 0..200_000_u64 {
            #[allow(clippy::cast_possible_truncation)]
            let bits = hash_words(&[0xf32, sample]) as u32;
            let value = f32::from_bits(bits);
            if !value.is_finite() {
                continue;
            }
            let text = exact_decimal_f32(value).unwrap();
            let read: f32 = serde_json::from_str(&text).unwrap();
            assert_eq!(exact_decimal_f32(read).as_deref(), Some(text.as_str()));
            if value.abs() >= 1e-8 {
                assert_eq!(read.to_bits(), value.to_bits(), "{value:e} → {text}");
                checked += 1;
            } else {
                tiny += 1;
                tiny_exact += usize::from(read.to_bits() == value.to_bits());
            }
        }
        assert!(checked > 50_000 && tiny > 50_000);
        assert!(tiny_exact * 100 >= tiny * 99, "{tiny_exact} of {tiny}");
    }

    #[test]
    fn pretty_output_matches_serde_json_apart_from_numbers() {
        let value = serde_json::json!({
            "name": "fir",
            "ages": [4.0, 8.5],
            "nested": { "flag": true, "nothing": null, "count": 3 }
        });
        assert_eq!(
            to_vec_pretty(&value).unwrap(),
            serde_json::to_vec_pretty(&value).unwrap()
        );
        assert_eq!(to_vec(&value).unwrap(), serde_json::to_vec(&value).unwrap());
        assert_eq!(to_vec(&[1.0 / 3.0]).unwrap(), b"[0.333333333333333]");
        assert!(to_vec(&[1e300]).is_err());
    }
}
