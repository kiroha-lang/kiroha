//! Property-based verification of exact decimals (spec §3.2–3.3) and strict
//! JSON (§3.4–3.5) against independent oracles, over randomly generated
//! values including coefficients far beyond 64 bits.

use kiri::decimal::{Decimal, DivError};
use kiri::json;
use kiri::value::{json_equal, Record, Value};
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, Zero};
use proptest::prelude::*;
use std::cmp::Ordering;

fn pow10(n: u32) -> BigInt {
    num_traits::pow(BigInt::from(10u8), n as usize)
}

/// Random decimal: up to 60 significant digits, exponent in [-40, 40].
fn decimal() -> impl Strategy<Value = Decimal> {
    (prop::collection::vec(0u8..10, 1..60), -40i64..40, any::<bool>()).prop_map(|(digits, exp, neg)| {
        let s: String = digits.iter().map(|d| char::from(b'0' + d)).collect();
        let mut c: BigInt = s.parse().unwrap();
        if neg {
            c = -c;
        }
        Decimal::normalize(c, exp)
    })
}

/// Random decimal with at most 34 significant digits.
fn decimal34() -> impl Strategy<Value = Decimal> {
    (prop::collection::vec(0u8..10, 1..35), -40i64..40, any::<bool>()).prop_map(|(digits, exp, neg)| {
        let s: String = digits.iter().map(|d| char::from(b'0' + d)).collect();
        let mut c: BigInt = s.parse().unwrap();
        if neg {
            c = -c;
        }
        Decimal::normalize(c, exp)
    })
}

fn nonzero_decimal() -> impl Strategy<Value = Decimal> {
    decimal().prop_filter("nonzero", |d| !d.is_zero())
}

/// Exact integer scaling of two decimals to a common exponent (oracle).
fn common(a: &Decimal, b: &Decimal) -> (BigInt, BigInt, i64) {
    let e = a.exp().min(b.exp());
    let ca = a.coef() * pow10((a.exp() - e) as u32);
    let cb = b.coef() * pow10((b.exp() - e) as u32);
    (ca, cb, e)
}

fn digit_len(n: &BigInt) -> u32 {
    if n.is_zero() {
        1
    } else {
        n.abs().to_string().len() as u32
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2000, .. ProptestConfig::default() })]

    // §3.2: normalized form — coefficient not divisible by ten, zero is 0e0.
    #[test]
    fn normalized(d in decimal()) {
        if d.is_zero() {
            prop_assert_eq!(d.exp(), 0);
        } else {
            prop_assert!(!(d.coef() % BigInt::from(10u8)).is_zero());
        }
    }

    // §3.3: canonical spelling parses back to the identical value.
    #[test]
    fn canonical_roundtrip(d in decimal()) {
        let text = d.canonical();
        let back = Decimal::parse_json(&text).unwrap().unwrap();
        prop_assert_eq!(&back, &d);
        prop_assert!(!text.contains('+') && text != "-0");
        // No trailing fractional zero in the mantissa.
        let mantissa = text.split('e').next().unwrap();
        prop_assert!(!mantissa.contains('.') || !mantissa.ends_with('0'), "{}", text);
    }

    // §3.2: + - * are exact (oracle: integer arithmetic at a common exponent).
    #[test]
    fn add_sub_mul_exact(a in decimal(), b in decimal()) {
        let (ca, cb, e) = common(&a, &b);
        prop_assert_eq!(a.add(&b).unwrap(), Decimal::normalize(&ca + &cb, e));
        prop_assert_eq!(a.sub(&b).unwrap(), Decimal::normalize(&ca - &cb, e));
        prop_assert_eq!(a.mul(&b).unwrap(), Decimal::normalize(a.coef() * b.coef(), a.exp() + b.exp()));
        prop_assert_eq!(a.add(&b).unwrap().sub(&b).unwrap(), a.clone());
    }

    // §3.5: ordering is exact and agrees with the sign of the difference.
    #[test]
    fn comparison_exact(a in decimal(), b in decimal()) {
        let (ca, cb, _) = common(&a, &b);
        prop_assert_eq!(a.cmp_exact(&b), ca.cmp(&cb));
        prop_assert_eq!(a.cmp_exact(&b), b.cmp_exact(&a).reverse());
    }

    // §3.2: division is the exact quotient rounded to 34 significant digits,
    // ties to even. Oracle: cross-multiplied integer error bound.
    #[test]
    fn division_rounding(a in decimal(), b in nonzero_decimal()) {
        let q = a.div(&b).unwrap();
        if a.is_zero() {
            prop_assert!(q.is_zero());
            return Ok(());
        }
        prop_assert!(digit_len(q.coef()) <= 34, "more than 34 digits: {}", q.canonical());
        // Exact quotient a/b = (ca·10^ea)/(cb·10^eb). Compare q·b with a.
        let qb = q.mul(&b).unwrap();
        let (num_err, scale_a, e) = common(&qb.sub(&a).unwrap(), &a);
        let _ = scale_a;
        // ulp of the 34th significant digit of q.
        let lead = q.exp() + digit_len(q.coef()) as i64 - 1;
        let ulp_exp = lead - 33;
        // |q·b - a| <= ulp/2 · |b|  <=>  2·|q·b - a| <= 10^ulp_exp · |b|
        let lhs = Decimal::normalize(num_err.abs() * 2u8, e);
        let rhs = Decimal::normalize(b.coef().abs(), b.exp() + ulp_exp);
        prop_assert!(lhs.cmp_exact(&rhs) != Ordering::Greater,
            "error too large: {} / {} = {}", a.canonical(), b.canonical(), q.canonical());
        // Ties: the 34-digit coefficient is even.
        if lhs.cmp_exact(&rhs) == Ordering::Equal && !num_err.is_zero() {
            let c34 = q.coef() * pow10((q.exp() - ulp_exp) as u32);
            prop_assert!(c34.is_even(), "tie not rounded to even: {}", q.canonical());
        }
    }

    // Ties are rare at random: construct them. A 35-significant-digit value
    // ending in 5, divided by ±10^k, lies exactly halfway at 34 digits.
    #[test]
    fn division_ties_to_even(digits in prop::collection::vec(0u8..10, 33..34), lead in 1u8..10,
                             exp in -20i64..20, k in -20i64..20, neg in any::<bool>(), bneg in any::<bool>()) {
        let s: String = std::iter::once(lead).chain(digits.into_iter()).chain(std::iter::once(5u8))
            .map(|d| char::from(b'0' + d)).collect();
        let mut c: BigInt = s.parse().unwrap();
        if neg { c = -c; }
        let a = Decimal::normalize(c.clone(), exp);
        let b = Decimal::normalize(if bneg { -BigInt::one() } else { BigInt::one() }, k);
        let q = a.div(&b).unwrap();
        // Expected: drop the final 5, round the 34-digit prefix to even.
        let mut prefix = c.abs() / 10u8;
        if prefix.is_odd() { prefix += 1u8; }
        let sign_neg = neg != bneg;
        let expected = Decimal::normalize(if sign_neg { -prefix } else { prefix }, exp + 1 - k);
        prop_assert_eq!(q, expected);
    }

    // A quotient that fits in 34 digits is exact.
    #[test]
    fn exact_quotients_preserved(q in decimal34(), b in nonzero_decimal()) {
        let a = q.mul(&b).unwrap();
        prop_assert_eq!(a.div(&b).unwrap(), q);
    }

    // §3.2: a % b = a - trunc(a/b)·b with the exact quotient; |r| < |b|;
    // the remainder has the sign of a.
    #[test]
    fn remainder_exact(a in decimal(), b in nonzero_decimal()) {
        let r = a.rem(&b).unwrap();
        let (ca, cb, _) = common(&a, &b);
        let (cr, cb2, _) = common(&r, &b);
        prop_assert!(cr.abs() < cb2.abs());
        if !r.is_zero() {
            prop_assert_eq!(r.is_negative(), a.is_negative());
        }
        // (a - r) is an integer multiple of b, and the multiple is trunc(a/b).
        let diff = a.sub(&r).unwrap();
        let (cd, cbb, _) = common(&diff, &b);
        prop_assert!((&cd % &cbb).is_zero());
        let trunc = &ca / &cb; // BigInt division truncates toward zero
        prop_assert_eq!(&cd / &cbb, trunc);
    }

    #[test]
    fn zero_divisor(a in decimal()) {
        prop_assert_eq!(a.div(&Decimal::zero()), Err(DivError::DivisionByZero));
        prop_assert_eq!(a.rem(&Decimal::zero()), Err(DivError::DivisionByZero));
    }
}

// ---------------------------------------------------------------- JSON

fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        decimal().prop_map(Value::num),
        "\\PC{0,12}".prop_map(|s| Value::str(&s)),
        prop::collection::vec(any::<char>(), 0..6).prop_map(|cs| Value::str(&cs.into_iter().collect::<String>())),
    ];
    leaf.prop_recursive(4, 48, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::array),
            prop::collection::vec(("[a-z\\u{0}-\\u{1f}\"\\\\é😀]{0,6}", inner), 0..6).prop_map(|kvs| {
                let mut r = Record::new();
                for (k, v) in kvs {
                    if !r.contains(&k) {
                        r.push(&k, v);
                    }
                }
                Value::record(r)
            }),
        ]
    })
}

/// Reverse the key order of every record (JSON equality ignores order).
fn reorder(v: &Value) -> Value {
    match v {
        Value::Array(a) => Value::array(a.iter().map(reorder).collect()),
        Value::Record(r) => {
            let mut out = Record::new();
            for (k, x) in r.entries().iter().rev() {
                out.push(k, reorder(x));
            }
            Value::record(out)
        }
        other => other.clone(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 1500, .. ProptestConfig::default() })]

    // §3.4: parse(dump(v)) == v, and dump is canonical (idempotent, order kept).
    #[test]
    fn json_roundtrip(v in json_value()) {
        let text = json::dump(&v);
        let back = json::parse(&text).unwrap();
        prop_assert!(json_equal(&back, &v));
        prop_assert_eq!(json::dump(&back), text.clone());
        // Only the specified escapes appear: no raw controls, no \u for >= 0x20.
        prop_assert!(!text.chars().any(|c| (c as u32) < 0x20));
    }

    // §3.5: equality ignores record order but not array order; it is
    // reflexive and symmetric.
    #[test]
    fn json_equality(v in json_value(), w in json_value()) {
        let r = reorder(&v);
        prop_assert!(json_equal(&v, &v));
        prop_assert!(json_equal(&v, &r));
        prop_assert_eq!(json_equal(&v, &w), json_equal(&w, &v));
        if json_equal(&v, &w) {
            prop_assert_eq!(kiri::value::json_hash(&v), kiri::value::json_hash(&w));
        }
        prop_assert_eq!(kiri::value::json_hash(&v), kiri::value::json_hash(&r));
    }

    // Strict parser rejects every truncation of a valid document.
    #[test]
    fn json_truncations_rejected(v in json_value()) {
        let text = json::dump(&v);
        for cut in 1..text.len() {
            if !text.is_char_boundary(cut) {
                continue;
            }
            let prefix = &text[..cut];
            if let Ok(p) = json::parse(prefix) {
                // A prefix can only parse if it is itself a complete number
                // that the document started with.
                prop_assert!(matches!(p, Value::Num(_)) && matches!(v, Value::Num(_)), "prefix parsed: {prefix}");
            }
        }
    }
}

#[test]
fn one_is_one() {
    assert!(BigInt::one().is_one());
}
