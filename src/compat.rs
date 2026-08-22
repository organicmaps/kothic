//! Reference-compatible number/string formatting helpers.
//!
//! The generated files must stay byte-identical with the previously published
//! output, which was produced by the original Python toolchain and relies on
//! CPython's float formatting semantics. These differ from Rust's
//! `Display`/`{:.*}`, so this module reimplements the exact behaviors needed:
//!
//!   * `repr(float)` / `str(float)` (shortest round-trip digits, `.0` suffix for
//!     integral values, scientific notation outside `1e-4..1e16`),
//!   * `"{:.Pg}".format(value)` (general format used for eval results and text dumps),
//!   * `round(value, 2)` (banker's rounding),
//!   * `repr(str)` (single-quoted with escapes).

/// Splits a Rust `Display` float string into significant decimal digits and a
/// base-10 exponent, such that `value == d0.d1d2... * 10^exp10`.
/// Returns `(digits, exp10)` with leading and trailing zeros stripped.
fn parse_display(s: &str) -> Option<(Vec<u8>, i32)> {
    let s = s.strip_prefix('-').unwrap_or(s);
    if s == "inf" || s == "NaN" || s == "nan" {
        return None;
    }
    let (mantissa, exp) = match s.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse::<i32>().ok()?),
        None => (s, 0i32),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((i, f)) => (i, f),
        None => (mantissa, ""),
    };
    let dot_pos = int_part.len() as i32;
    let mut digits: Vec<u8> = Vec::with_capacity(int_part.len() + frac_part.len());
    digits.extend(int_part.bytes().map(|b| b - b'0'));
    digits.extend(frac_part.bytes().map(|b| b - b'0'));
    if digits.is_empty() {
        return None;
    }
    // Strip leading zeros.
    let mut z = 0usize;
    while z + 1 < digits.len() && digits[z] == 0 {
        z += 1;
    }
    let digits = digits.split_off(z);
    let exp10 = dot_pos - z as i32 - 1 + exp;
    // Strip trailing zeros (not significant).
    let mut d = digits;
    while d.len() > 1 && *d.last().unwrap() == 0 {
        d.pop();
    }
    let exp10 = if d == [0] { 0 } else { exp10 };
    Some((d, exp10))
}

fn strip_trailing_zeros(d: &[u8], exp10: i32) -> (Vec<u8>, i32) {
    let mut v = d.to_vec();
    while v.len() > 1 && *v.last().unwrap() == 0 {
        v.pop();
    }
    if v == [0] { (vec![0], 0) } else { (v, exp10) }
}

fn pow2_u128(e: u32) -> Option<u128> {
    if e < 128 { Some(1u128 << e) } else { None }
}

fn pow5_u128(mut e: u32) -> Option<u128> {
    let mut r: u128 = 1;
    while e > 0 {
        r = r.checked_mul(5)?;
        e -= 1;
    }
    Some(r)
}

/// Rounds `x * 10^k` to the nearest integer with ties-to-even, computed from
/// the exact binary value of `x` (mirrors CPython's `_Py_dg_dtoa` rounding).
/// Returns `None` when the exact arithmetic overflows a `u128` (only reachable
/// for values far outside the MapCSS numeric range).
fn exact_scale_round(x: f64, k: i64) -> Option<i64> {
    // Exact value: x = m * 2^e.
    let bits = x.to_bits();
    let exp_bits = ((bits >> 52) & 0x7FF) as i64;
    let frac = bits & ((1u64 << 52) - 1);
    let (m, e) = if exp_bits == 0 {
        (frac as u128, -1074i64)
    } else {
        ((frac | (1u64 << 52)) as u128, exp_bits - 1075)
    };
    // x * 10^k = m * 2^(e+k) * 5^k
    let e2 = e + k;
    let mut num = m;
    if e2 >= 0 {
        num = num.checked_mul(pow2_u128(e2 as u32)?)?;
    }
    if k >= 0 {
        num = num.checked_mul(pow5_u128(k as u32)?)?;
    }
    let mut den: u128 = 1;
    if e2 < 0 {
        den = den.checked_mul(pow2_u128((-e2) as u32)?)?;
    }
    if k < 0 {
        den = den.checked_mul(pow5_u128((-k) as u32)?)?;
    }
    let q = num / den;
    let r = num % den;
    if r == 0 {
        return Some(q as i64);
    }
    // Round without computing 2*r (avoids u128 overflow when den is large).
    let dist = den - r;
    if r > dist {
        Some(q as i64 + 1)
    } else if r < dist {
        Some(q as i64)
    } else {
        // Exact tie: round to even.
        Some(q as i64 + (q & 1) as i64)
    }
}

/// Renders fixed-point notation (`%f`-like) given significant digits and exponent.
fn render_fixed(digits: &[u8], exp10: i32) -> String {
    let n = digits.len() as i32;
    let point = exp10 + 1; // number of digits before the decimal point
    let mut s = String::new();
    if point >= n {
        // Integer: digits followed by zeros.
        for &d in digits {
            s.push((b'0' + d) as char);
        }
        for _ in n..point {
            s.push('0');
        }
    } else if point <= 0 {
        s.push_str("0.");
        for _ in 0..(-point) {
            s.push('0');
        }
        for &d in digits {
            s.push((b'0' + d) as char);
        }
    } else {
        for i in 0..point {
            s.push((b'0' + digits[i as usize]) as char);
        }
        s.push('.');
        for i in point..n {
            s.push((b'0' + digits[i as usize]) as char);
        }
    }
    s
}

fn render_sci(digits: &[u8], exp10: i32) -> String {
    let mut s = String::new();
    s.push((b'0' + digits[0]) as char);
    if digits.len() > 1 {
        s.push('.');
        for &d in &digits[1..] {
            s.push((b'0' + d) as char);
        }
    }
    let sign = if exp10 < 0 { '-' } else { '+' };
    s.push('e');
    s.push(sign);
    let e = exp10.unsigned_abs();
    s.push_str(&format!("{:02}", e));
    s
}

pub fn float_str(x: f64) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        };
    }
    if x.is_infinite() || x.is_nan() {
        let d = format!("{}", x);
        return if d == "NaN" { "nan".to_string() } else { d };
    }
    let neg = x.is_sign_negative();
    let (digits, exp10) = shortest(x);
    let body = if (-4..16).contains(&exp10) {
        let mut s = render_fixed(&digits, exp10);
        if !s.contains('.') {
            s.push_str(".0");
        }
        s
    } else {
        render_sci(&digits, exp10)
    };
    if neg { format!("-{}", body) } else { body }
}

pub fn g_format(x: f64, precision: usize) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() {
            "-0".to_string()
        } else {
            "0".to_string()
        };
    }
    if x.is_infinite() || x.is_nan() {
        return format!("{}", x);
    }
    let neg = x.is_sign_negative();
    let ax = x.abs();
    // exp10 = floor(log10(ax)); corrected for log10 rounding error.
    let mut exp10 = ax.log10().floor() as i32;
    while 10f64.powi(exp10 + 1) <= ax {
        exp10 += 1;
    }
    while 10f64.powi(exp10) > ax {
        exp10 -= 1;
    }
    let p = precision as i64;
    let k = (p - 1) - exp10 as i64;
    let mut q = exact_scale_round(ax, k)
        .unwrap_or_else(|| (ax * 10f64.powi(k as i32)).round_ties_even() as i64);
    // Rounding may have produced p+1 digits (e.g. 99 -> 100 at p=1).
    while q >= 10i64.pow(p as u32) {
        q /= 10;
        exp10 += 1;
    }
    if q == 0 {
        q = 1;
        exp10 = 0;
    }
    let (digits, _) = strip_trailing_zeros(
        &q.to_string().bytes().map(|b| b - b'0').collect::<Vec<u8>>(),
        exp10,
    );
    let body = if exp10 < -4 || exp10 >= precision as i32 {
        render_sci(&digits, exp10)
    } else {
        render_fixed(&digits, exp10)
    };
    if neg { format!("-{}", body) } else { body }
}

/// Shortest round-trip digits of `x` (relies on Rust `Display` which uses the
/// same shortest-round-trip algorithm family as CPython's `repr`).
fn shortest(x: f64) -> (Vec<u8>, i32) {
    parse_display(&format!("{}", x)).unwrap_or((vec![0], 0))
}

pub fn round2(x: f64) -> f64 {
    let neg = x.is_sign_negative();
    let q =
        exact_scale_round(x.abs(), 2).unwrap_or_else(|| (x.abs() * 100.0).round_ties_even() as i64);
    let r = q as f64 / 100.0;
    if neg { -r } else { r }
}

pub fn repr_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repr_float(v: f64) -> String {
        float_str(v)
    }

    #[test]
    fn test_repr_basics() {
        assert_eq!(repr_float(5.0), "5.0");
        assert_eq!(repr_float(200.0), "200.0");
        assert_eq!(repr_float(0.3), "0.3");
        assert_eq!(repr_float(0.5), "0.5");
        assert_eq!(repr_float(2.5), "2.5");
        assert_eq!(repr_float(2048.0), "2048.0");
        assert_eq!(repr_float(0.0), "0.0");
        assert_eq!(repr_float(-0.0), "-0.0");
        assert_eq!(repr_float(1e15), "1000000000000000.0");
        assert_eq!(repr_float(1e16), "1e+16");
        assert_eq!(repr_float(1e-4), "0.0001");
        assert_eq!(repr_float(1e-5), "1e-05");
        assert_eq!(repr_float(100.0), "100.0");
        assert_eq!(repr_float(0.1), "0.1");
    }

    #[test]
    fn test_g_basics() {
        assert_eq!(g_format(5.0, 4), "5");
        assert_eq!(g_format(0.3, 4), "0.3");
        assert_eq!(g_format(2048.0, 4), "2048");
        assert_eq!(g_format(20480.0, 4), "2.048e+04");
        assert_eq!(g_format(0.0001, 4), "0.0001");
        assert_eq!(g_format(0.00001, 4), "1e-05");
        assert_eq!(g_format(2.5, 6), "2.5");
        assert_eq!(g_format(11.2, 6), "11.2");
        assert_eq!(g_format(1.2, 6), "1.2");
        assert_eq!(g_format(9.0, 4), "9");
        assert_eq!(g_format(512.0, 4), "512");
        assert_eq!(g_format(30.0, 4), "30");
        assert_eq!(g_format(60.0, 4), "60");
        assert_eq!(g_format(256.0, 4), "256");
        assert_eq!(g_format(0.0, 6), "0");
    }

    #[test]
    fn test_g_rounding() {
        // 2.5 rounded to 1 sig digit -> 2 (half ties away)
        assert_eq!(g_format(2.5, 1), "2");
        assert_eq!(g_format(1.05, 2), "1.1");
        assert_eq!(g_format(99.0, 1), "1e+02");
        assert_eq!(g_format(0.0003, 1), "0.0003");
    }

    #[test]
    fn test_repr_str() {
        assert_eq!(repr_str("name"), "'name'");
        assert_eq!(repr_str("a'b"), "'a\\'b'");
        assert_eq!(repr_str("a\\b"), "'a\\\\b'");
    }

    #[test]
    fn test_round2() {
        assert_eq!(round2(2.5), 2.5);
        assert_eq!(round2(2.675), 2.67);
        // 2.685 is stored as a double slightly above 2.685, so it rounds up.
        assert_eq!(round2(2.685), 2.69);
        assert_eq!(round2(-2.675), -2.67);
    }
}
