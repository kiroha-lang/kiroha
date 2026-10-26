//! Exact arbitrary-precision decimal numbers (spec §3.2–3.3).
//!
//! A nonzero value is `coef × 10^exp` with `coef` not divisible by ten; zero is
//! `0 × 10^0`. Addition, subtraction, multiplication and comparison are exact.
//! Division rounds to 34 significant digits, ties to even. Remainder uses the
//! exact rational quotient truncated toward zero.

use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::cmp::Ordering;
use std::fmt;

/// Host coefficient quota (spec §3.2 permits coefficient/exponent limits only
/// as host resource termination). Any operand, scaling or result coefficient
/// beyond this many decimal digits aborts with `resource` instead of
/// computing. It bounds the cost of a single arithmetic operation, which
/// arbitrary-precision libraries cannot interrupt, to tens of milliseconds.
pub const MAX_DIGITS: u64 = 100_000;

/// Largest power-of-ten scaling performed by a single operation.
pub const MAX_SCALE_DIGITS: u64 = MAX_DIGITS;

/// Upper bound on bits for a MAX_DIGITS coefficient (log2(10) ≈ 3.3219).
const MAX_BITS: u64 = MAX_DIGITS * 33220 / 10000 + 1;

fn check_bits(n: &BigInt) -> Result<(), Overflow> {
    if n.bits() > MAX_BITS {
        Err(Overflow)
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Decimal {
    coef: BigInt,
    exp: i64,
}

/// A numeric operation would exceed host resources (exponent or digit range).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overflow;

/// Arithmetic failure: either a Kiri-level division by zero or a resource abort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DivError {
    DivisionByZero,
    Overflow,
}

impl From<Overflow> for DivError {
    fn from(_: Overflow) -> Self {
        DivError::Overflow
    }
}

fn pow10(n: u64) -> Result<BigInt, Overflow> {
    pow10_capped(n, MAX_SCALE_DIGITS)
}

fn pow10_capped(n: u64, cap: u64) -> Result<BigInt, Overflow> {
    if n > cap {
        return Err(Overflow);
    }
    Ok(num_traits::pow(BigInt::from(10u8), n as usize))
}

fn digit_count(n: &BigInt) -> u64 {
    if n.is_zero() {
        return 1;
    }
    // bits * log10(2) gives a lower bound within one; refine by comparison.
    let bits = n.bits();
    let est = ((bits as f64 - 1.0) * std::f64::consts::LOG10_2).floor() as u64 + 1;
    let mag = n.abs();
    // est is either the exact count or one less.
    match pow10(est) {
        Ok(p) if mag >= p => est + 1,
        _ => est,
    }
}

impl Decimal {
    pub fn zero() -> Decimal {
        Decimal { coef: BigInt::zero(), exp: 0 }
    }

    pub fn from_i64(v: i64) -> Decimal {
        Decimal::normalize(BigInt::from(v), 0)
    }

    pub fn from_u64(v: u64) -> Decimal {
        Decimal::normalize(BigInt::from(v), 0)
    }

    pub fn from_bigint(v: BigInt) -> Decimal {
        Decimal::normalize(v, 0)
    }

    /// Build a normalized decimal from an arbitrary coefficient/exponent pair.
    pub fn normalize(mut coef: BigInt, mut exp: i64) -> Decimal {
        if coef.is_zero() {
            return Decimal::zero();
        }
        let big = BigInt::from(10_000_000_000_000_000u64);
        loop {
            let (q, r) = coef.div_rem(&big);
            if r.is_zero() {
                coef = q;
                exp += 16;
            } else {
                break;
            }
        }
        let ten = BigInt::from(10u8);
        loop {
            let (q, r) = coef.div_rem(&ten);
            if r.is_zero() {
                coef = q;
                exp += 1;
            } else {
                break;
            }
        }
        Decimal { coef, exp }
    }

    pub fn coef(&self) -> &BigInt {
        &self.coef
    }

    pub fn exp(&self) -> i64 {
        self.exp
    }

    pub fn is_zero(&self) -> bool {
        self.coef.is_zero()
    }

    pub fn is_negative(&self) -> bool {
        self.coef.sign() == Sign::Minus
    }

    pub fn is_integer(&self) -> bool {
        self.exp >= 0
    }

    /// Parse a complete JSON-number spelling, optionally signed. Returns
    /// `Ok(None)` for invalid syntax and `Err(Overflow)` for an exponent beyond
    /// the host range.
    pub fn parse_json(s: &str) -> Result<Option<Decimal>, Overflow> {
        let b = s.as_bytes();
        let mut i = 0;
        let neg = if b.first() == Some(&b'-') {
            i += 1;
            true
        } else {
            false
        };
        let int_start = i;
        if i >= b.len() {
            return Ok(None);
        }
        if b[i] == b'0' {
            i += 1;
        } else if b[i].is_ascii_digit() {
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        } else {
            return Ok(None);
        }
        let int_part = &s[int_start..i];
        let mut frac_part = "";
        if i < b.len() && b[i] == b'.' {
            i += 1;
            let fs = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i == fs {
                return Ok(None);
            }
            frac_part = &s[fs..i];
        }
        let mut exp_val = BigInt::zero();
        if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
            i += 1;
            let mut eneg = false;
            if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
                eneg = b[i] == b'-';
                i += 1;
            }
            let es = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i == es {
                return Ok(None);
            }
            let digits = s[es..i].trim_start_matches('0');
            if digits.len() > 19 {
                return Err(Overflow); // beyond the exponent quota; never parse it
            }
            exp_val = if digits.is_empty() { BigInt::zero() } else { digits.parse::<BigInt>().map_err(|_| Overflow)? };
            if eneg {
                exp_val = -exp_val;
            }
        }
        if i != b.len() {
            return Ok(None);
        }
        let significant = format!("{int_part}{frac_part}");
        let significant = significant.trim_start_matches('0').trim_end_matches('0');
        if significant.len() as u64 > MAX_DIGITS {
            return Err(Overflow);
        }
        // Only the significant digits form the coefficient; leading/trailing
        // zeros of the spelling shift the exponent instead.
        let trailing = (int_part.len() + frac_part.len()) - format!("{int_part}{frac_part}").trim_end_matches('0').len();
        let mut coef: BigInt = if significant.is_empty() { BigInt::zero() } else { significant.parse().map_err(|_| Overflow)? };
        if neg {
            coef = -coef;
        }
        let exp = exp_val - BigInt::from(frac_part.len()) + BigInt::from(trailing);
        let exp = exp.to_i64().ok_or(Overflow)?;
        if exp.unsigned_abs() > (i64::MAX as u64) / 4 {
            return Err(Overflow);
        }
        Ok(Some(Decimal::normalize(coef, exp)))
    }

    pub fn neg(&self) -> Decimal {
        Decimal { coef: -self.coef.clone(), exp: self.exp }
    }

    fn check_exp(e: i128) -> Result<i64, Overflow> {
        if e.unsigned_abs() > (i64::MAX as u128) / 4 {
            Err(Overflow)
        } else {
            Ok(e as i64)
        }
    }

    fn aligned(a: &Decimal, b: &Decimal) -> Result<(BigInt, BigInt, i64), Overflow> {
        let e = a.exp.min(b.exp);
        let sa = (a.exp - e) as u64;
        let sb = (b.exp - e) as u64;
        let ca = if sa == 0 { a.coef.clone() } else { &a.coef * pow10(sa)? };
        let cb = if sb == 0 { b.coef.clone() } else { &b.coef * pow10(sb)? };
        Ok((ca, cb, e))
    }

    pub fn add(&self, o: &Decimal) -> Result<Decimal, Overflow> {
        if self.is_zero() {
            return Ok(o.clone());
        }
        if o.is_zero() {
            return Ok(self.clone());
        }
        let (a, b, e) = Decimal::aligned(self, o)?;
        let sum = a + b;
        check_bits(&sum)?;
        Ok(Decimal::normalize(sum, e))
    }

    pub fn sub(&self, o: &Decimal) -> Result<Decimal, Overflow> {
        self.add(&o.neg())
    }

    pub fn mul(&self, o: &Decimal) -> Result<Decimal, Overflow> {
        if self.is_zero() || o.is_zero() {
            return Ok(Decimal::zero());
        }
        let e = Decimal::check_exp(self.exp as i128 + o.exp as i128)?;
        if self.coef.bits() + o.coef.bits() > MAX_BITS + 1 {
            return Err(Overflow);
        }
        Ok(Decimal::normalize(&self.coef * &o.coef, e))
    }

    /// Division rounded to 34 significant digits, ties to even (§3.2).
    pub fn div(&self, o: &Decimal) -> Result<Decimal, DivError> {
        if o.is_zero() {
            return Err(DivError::DivisionByZero);
        }
        if self.is_zero() {
            return Ok(Decimal::zero());
        }
        let negative = self.is_negative() != o.is_negative();
        let n = self.coef.abs();
        let d = o.coef.abs();
        // value = n/d * 10^(ea-eb); find k with 10^k <= n/d < 10^(k+1)
        let k0 = digit_count(&n) as i64 - digit_count(&d) as i64;
        let ge = if k0 >= 0 {
            n >= &d * pow10(k0 as u64)?
        } else {
            &n * pow10((-k0) as u64)? >= d
        };
        let k = if ge { k0 } else { k0 - 1 };
        let s = 33 - k;
        // Internal scaling for the 34-digit quotient is bounded by the operand
        // sizes (each within the coefficient quota), not by the quota itself.
        let cap = 2 * MAX_DIGITS + 64;
        let (num, den) = if s >= 0 {
            (&n * pow10_capped(s as u64, cap)?, d)
        } else {
            (n, &d * pow10_capped((-s) as u64, cap)?)
        };
        let (mut q, r) = num.div_rem(&den);
        let twice = &r * 2u8;
        if twice > den || (twice == den && q.is_odd()) {
            q += BigInt::one();
        }
        if negative {
            q = -q;
        }
        let e = Decimal::check_exp(self.exp as i128 - o.exp as i128 - s as i128)?;
        Ok(Decimal::normalize(q, e))
    }

    /// `a - trunc(a/b)*b` using the exact rational quotient (§3.2).
    pub fn rem(&self, o: &Decimal) -> Result<Decimal, DivError> {
        if o.is_zero() {
            return Err(DivError::DivisionByZero);
        }
        if self.is_zero() {
            return Ok(Decimal::zero());
        }
        // |a| < |b|: the truncated quotient is 0, so the remainder is a.
        if self.abs_cmp(o) == Ordering::Less {
            return Ok(self.clone());
        }
        if self.exp >= o.exp {
            // a/b = ca·10^(ea-eb)/cb exactly; the remainder is
            // (ca · (10^(ea-eb) mod cb)) mod cb, scaled by 10^eb, with the sign
            // of a. Modular exponentiation keeps this cheap for any exponent gap.
            let cb = o.coef.abs();
            let gap = BigInt::from(self.exp as i128 - o.exp as i128);
            let p = BigInt::from(10u8).modpow(&gap, &cb);
            let mut r = (self.coef.abs() * p) % &cb;
            if self.is_negative() {
                r = -r;
            }
            return Ok(Decimal::normalize(r, o.exp));
        }
        // ea < eb and |a| >= |b|: the scaling of b is bounded by a's digits.
        let (a, b, e) = Decimal::aligned(self, o)?;
        let r = &a % &b; // num-bigint `%` truncates toward zero, sign of a
        Ok(Decimal::normalize(r, e))
    }

    fn abs_cmp(&self, o: &Decimal) -> Ordering {
        let a = Decimal { coef: self.coef.abs(), exp: self.exp };
        let b = Decimal { coef: o.coef.abs(), exp: o.exp };
        a.cmp_exact(&b)
    }

    pub fn cmp_exact(&self, o: &Decimal) -> Ordering {
        let sa = self.coef.sign();
        let sb = o.coef.sign();
        let rank = |s: Sign| match s {
            Sign::Minus => 0,
            Sign::NoSign => 1,
            Sign::Plus => 2,
        };
        if rank(sa) != rank(sb) {
            return rank(sa).cmp(&rank(sb));
        }
        if sa == Sign::NoSign {
            return Ordering::Equal;
        }
        // Compare adjusted exponents (position of leading digit) first.
        let aa = self.exp as i128 + digit_count(&self.coef) as i128;
        let ab = o.exp as i128 + digit_count(&o.coef) as i128;
        let mag = if aa != ab {
            aa.cmp(&ab)
        } else {
            match Decimal::aligned(self, o) {
                Ok((x, y, _)) => x.abs().cmp(&y.abs()),
                Err(_) => Ordering::Equal,
            }
        };
        if sa == Sign::Minus {
            mag.reverse()
        } else {
            mag
        }
    }

    /// Integer value as i128 when exactly integral and in range.
    pub fn to_i128(&self) -> Option<i128> {
        if !self.is_integer() {
            return None;
        }
        if self.exp > 38 {
            return None;
        }
        let v = &self.coef * pow10(self.exp as u64).ok()?;
        v.to_i128()
    }

    pub fn to_u64(&self) -> Option<u64> {
        let v = self.to_i128()?;
        u64::try_from(v).ok()
    }

    /// Integral value as a BigInt (None when fractional).
    pub fn to_bigint(&self) -> Option<BigInt> {
        if !self.is_integer() {
            return None;
        }
        Some(&self.coef * pow10(self.exp as u64).ok()?)
    }

    /// Canonical spelling (§3.3).
    pub fn canonical(&self) -> String {
        if self.coef.is_zero() {
            return "0".to_string();
        }
        let digits = self.coef.abs().to_string();
        let n = digits.len() as i64;
        let e = self.exp;
        let a = e as i128 + n as i128 - 1;
        let mut out = String::new();
        if self.is_negative() {
            out.push('-');
        }
        if (-6..21).contains(&a) {
            if e >= 0 {
                out.push_str(&digits);
                for _ in 0..e {
                    out.push('0');
                }
            } else {
                let point = n + e;
                if point > 0 {
                    out.push_str(&digits[..point as usize]);
                    out.push('.');
                    out.push_str(&digits[point as usize..]);
                } else {
                    out.push_str("0.");
                    for _ in 0..(-point) {
                        out.push('0');
                    }
                    out.push_str(&digits);
                }
            }
        } else {
            out.push_str(&digits[..1]);
            if n > 1 {
                out.push('.');
                out.push_str(&digits[1..]);
            }
            out.push('e');
            out.push_str(&a.to_string());
        }
        out
    }

    /// Approximate storage size in bytes, for memory accounting.
    pub fn approx_bytes(&self) -> usize {
        32 + (self.coef.bits() as usize).div_ceil(8)
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::parse_json(s).unwrap().unwrap()
    }

    #[test]
    fn canonical_spelling() {
        assert_eq!(d("1.2300").canonical(), "1.23");
        assert_eq!(d("1e20").canonical(), "100000000000000000000");
        assert_eq!(d("1e21").canonical(), "1e21");
        assert_eq!(d("1e-6").canonical(), "0.000001");
        assert_eq!(d("1e-7").canonical(), "1e-7");
        assert_eq!(d("-0.0").canonical(), "0");
        assert_eq!(d("-1e21").canonical(), "-1e21");
        assert_eq!(d("123.456e30").canonical(), "1.23456e32");
        assert_eq!(d("0.0001234").canonical(), "0.0001234");
    }

    #[test]
    fn exact_arithmetic() {
        assert_eq!(d("0.1").add(&d("0.2")).unwrap().canonical(), "0.3");
        assert_eq!(d("9007199254740993").add(&d("1")).unwrap().canonical(), "9007199254740994");
        assert_eq!(d("1e100").mul(&d("1e-100")).unwrap().canonical(), "1");
    }

    #[test]
    fn division_rounding() {
        assert_eq!(d("1").div(&d("3")).unwrap().canonical(), "0.3333333333333333333333333333333333");
        assert_eq!(d("2").div(&d("3")).unwrap().canonical(), "0.6666666666666666666666666666666667");
        assert_eq!(d("1").div(&d("8")).unwrap().canonical(), "0.125");
        assert_eq!(
            d("12345678901234567890123456789012345").div(&d("1")).unwrap().canonical(),
            "1.234567890123456789012345678901234e34"
        );
        assert_eq!(
            d("-12345678901234567890123456789012355").div(&d("1")).unwrap().canonical(),
            "-1.234567890123456789012345678901236e34"
        );
        assert_eq!(d("9.9999999999999999999999999999999999").div(&d("1")).unwrap().canonical(), "10");
        assert_eq!(d("20").div(&d("5")).unwrap().div(&d("2")).unwrap().canonical(), "2");
        assert_eq!(d("1").div(&d("0")), Err(DivError::DivisionByZero));
    }

    #[test]
    fn remainder() {
        assert_eq!(d("5").rem(&d("2")).unwrap().canonical(), "1");
        assert_eq!(d("-5").rem(&d("2")).unwrap().canonical(), "-1");
        assert_eq!(d("5").rem(&d("-2")).unwrap().canonical(), "1");
        assert_eq!(d("29999999999999999999999999999999999").rem(&d("3")).unwrap().canonical(), "2");
        assert_eq!(d("5.5").rem(&d("2")).unwrap().canonical(), "1.5");
    }

    #[test]
    fn comparison() {
        assert_eq!(d("1").cmp_exact(&d("1.0")), Ordering::Equal);
        assert_eq!(d("-2").cmp_exact(&d("1")), Ordering::Less);
        assert_eq!(d("0.001").cmp_exact(&d("0.01")), Ordering::Less);
        assert_eq!(d("-0.001").cmp_exact(&d("-0.01")), Ordering::Greater);
        assert_eq!(d("1e30").cmp_exact(&d("999")), Ordering::Greater);
    }

    #[test]
    fn coefficient_quota() {
        let big = "9".repeat(60_000);
        let x = d(&big);
        assert!(x.mul(&x).is_err());
        assert!(Decimal::parse_json(&"1".repeat(100_001)).is_err());
        assert_eq!(d(&format!("1{}", "0".repeat(150_000))).canonical(), "1e150000");
        assert_eq!(d("7").rem(&d("1e1000000000")).unwrap().canonical(), "7");
        assert_eq!(d("1e100001").rem(&d("7")).unwrap().canonical(), "5");
        assert_eq!(d("1e60000").rem(&d("1e-60000")).unwrap().canonical(), "0");
        assert_eq!(d("-1e100001").rem(&d("7")).unwrap().canonical(), "-5");
        assert_eq!(d("12.5").rem(&d("0.3")).unwrap().canonical(), "0.2");
        let big = d("1e99998").add(&d("1")).unwrap();
        assert_eq!(d("1").div(&big).unwrap().canonical(), "1e-99998");
        assert_eq!(d("0.000").canonical(), "0");
        assert_eq!(d("120.50").canonical(), "120.5");
        assert!(d("1e20000000").add(&d("1")).is_err());
        assert_eq!(d("1e20000000").mul(&d("1e5")).unwrap().canonical(), "1e20000005");
    }

    #[test]
    fn json_syntax() {
        assert!(Decimal::parse_json("01").unwrap().is_none());
        assert!(Decimal::parse_json("+1").unwrap().is_none());
        assert!(Decimal::parse_json("1.").unwrap().is_none());
        assert!(Decimal::parse_json(" 1").unwrap().is_none());
        assert_eq!(d("1.25e2").canonical(), "125");
        assert_eq!(d("-0").canonical(), "0");
    }
}

