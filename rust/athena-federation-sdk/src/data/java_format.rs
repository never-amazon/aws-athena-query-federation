use bigdecimal::BigDecimal;
use num_bigint::BigInt;

fn java_fp_to_string(digits: &str, exp: i32, negative: bool) -> String {
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    let point = exp + 1;
    if (-3..=6).contains(&exp) {
        match usize::try_from(point) {
            Err(_) | Ok(0) => {
                out.push_str("0.");
                for _ in 0..(-point) {
                    out.push('0');
                }
                out.push_str(digits);
            }
            Ok(p) if p >= digits.len() => {
                out.push_str(digits);
                for _ in 0..(p - digits.len()) {
                    out.push('0');
                }
                out.push_str(".0");
            }
            Ok(p) => {
                out.push_str(&digits[..p]);
                out.push('.');
                out.push_str(&digits[p..]);
            }
        }
    } else {
        out.push_str(&digits[..1]);
        out.push('.');
        if digits.len() > 1 {
            out.push_str(&digits[1..]);
        } else {
            out.push('0');
        }
        out.push('E');
        out.push_str(&exp.to_string());
    }
    out
}

fn split_sci(sci: &str) -> (String, i32, bool) {
    let negative = sci.starts_with('-');
    let body = sci.trim_start_matches('-');
    let (mantissa, exp) = body.split_once('e').unwrap_or((body, "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let exp = exp.parse::<i32>().unwrap_or(0);
    (digits, exp, negative)
}

pub fn double_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    let (digits, exp, negative) = split_sci(&format!("{value:e}"));
    java_fp_to_string(&digits, exp, negative)
}

pub fn float_to_string(value: f32) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    let (digits, exp, negative) = split_sci(&format!("{value:e}"));
    java_fp_to_string(&digits, exp, negative)
}

pub fn big_decimal_to_string(value: &BigDecimal) -> String {
    let (unscaled, scale) = value.as_bigint_and_exponent();
    big_decimal_parts_to_string(&unscaled, scale)
}

pub fn big_decimal_parts_to_string(unscaled: &BigInt, scale: i64) -> String {
    let negative = unscaled.sign() == num_bigint::Sign::Minus;
    let coeff = unscaled.magnitude().to_string();
    let adjusted = -scale + (i64::try_from(coeff.len()).unwrap_or(i64::MAX) - 1);
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if scale >= 0 && adjusted >= -6 {
        if scale == 0 {
            out.push_str(&coeff);
        } else {
            push_positive_scale(&mut out, &coeff, scale);
        }
    } else {
        out.push_str(&coeff[..1]);
        if coeff.len() > 1 {
            out.push('.');
            out.push_str(&coeff[1..]);
        }
        out.push('E');
        if adjusted >= 0 {
            out.push('+');
        }
        out.push_str(&adjusted.to_string());
    }
    out
}

pub fn big_decimal_to_plain_string(value: &BigDecimal) -> String {
    let (unscaled, scale) = value.as_bigint_and_exponent();
    let negative = unscaled.sign() == num_bigint::Sign::Minus;
    let coeff = unscaled.magnitude().to_string();
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if scale <= 0 {
        out.push_str(&coeff);
        for _ in 0..(-scale) {
            out.push('0');
        }
    } else {
        push_positive_scale(&mut out, &coeff, scale);
    }
    out
}

fn push_positive_scale(out: &mut String, coeff: &str, scale: i64) {
    let scale = usize::try_from(scale).unwrap_or(usize::MAX);
    if coeff.len() > scale {
        let int_len = coeff.len() - scale;
        out.push_str(&coeff[..int_len]);
        out.push('.');
        out.push_str(&coeff[int_len..]);
    } else {
        out.push_str("0.");
        for _ in 0..(scale - coeff.len()) {
            out.push('0');
        }
        out.push_str(coeff);
    }
}

pub fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for unit in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    h
}

pub fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0F) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn double_formatting_matches_java() {
        assert_eq!(double_to_string(1.0), "1.0");
        assert_eq!(double_to_string(1.1), "1.1");
        assert_eq!(double_to_string(100.0), "100.0");
        assert_eq!(double_to_string(1.0e7), "1.0E7");
        assert_eq!(double_to_string(1_234_567.0), "1234567.0");
        assert_eq!(double_to_string(0.001), "0.001");
        assert_eq!(double_to_string(0.0001), "1.0E-4");
        assert_eq!(double_to_string(-2.5), "-2.5");
        assert_eq!(float_to_string(1.5), "1.5");
    }

    #[test]
    fn big_decimal_formatting_matches_java() {
        assert_eq!(
            big_decimal_to_string(&BigDecimal::from_str("1.00").unwrap()),
            "1.00"
        );
        assert_eq!(
            big_decimal_to_string(&BigDecimal::from_str("-0.5").unwrap()),
            "-0.5"
        );
        assert_eq!(
            big_decimal_to_string(&BigDecimal::new(BigInt::from(123), -2)),
            "1.23E+4"
        );
        assert_eq!(
            big_decimal_to_string(&BigDecimal::new(BigInt::from(1), 8)),
            "1E-8"
        );
        assert_eq!(
            big_decimal_to_string(&BigDecimal::new(BigInt::from(0), 2)),
            "0.00"
        );
    }
}
