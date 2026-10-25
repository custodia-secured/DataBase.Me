// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fractional sort keys for ordering siblings (ADR-0003, ADR-0013).
//!
//! A key is a string over the 62 ASCII digits `0-9A-Za-z`, compared byte by byte. It has two
//! parts:
//!
//! - an **integer part** whose first character says its length: `a`..`z` start integers of 2 to 27
//!   characters (non-negative, growing), `A`..`Z` start integers of 27 down to 2 characters
//!   (negative). Appending or prepending one item at a time changes only this part, so keys stay
//!   short (logarithmic in the number of items).
//! - an optional **fraction**, used when a key is made between two neighbours. A fraction never
//!   ends in `0`, so there is always room for another key between any two.
//!
//! The first key is `a0`; after it come `a1`, `a2`, …; between `a0` and `a1` comes `a0V`. This is
//! the algorithm of David Greenspan's "Implementing Fractional Indexing" as published in the
//! `fractional-indexing` library (CC0), with the same digits and the same results.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::CoreError;

const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const BASE: usize = 62;
const ZERO: u8 = b'0';
const LARGEST_DIGIT: u8 = b'z';
/// The smallest integer part. It cannot stand alone: nothing could go before it.
const SMALLEST_INTEGER: &str = "A00000000000000000000000000";

/// An order key. Siblings sort by key, then by ID when keys are equal.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(transparent))]
pub struct SortKey(String);

impl SortKey {
    /// The key for the only item in an empty list: `a0`.
    pub fn first() -> Self {
        Self("a0".to_owned())
    }

    /// Checks and wraps a key.
    pub fn parse(text: &str) -> Result<Self, CoreError> {
        validate(text.as_bytes()).map_err(|()| CoreError::InvalidSortKey(text.to_owned()))?;
        Ok(Self(text.to_owned()))
    }

    /// The key text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A key strictly between `before` and `after`. `None` means "no neighbour on that side", so
    /// `between(None, None)` is the first key, `between(Some(last), None)` appends and
    /// `between(None, Some(first))` prepends.
    pub fn between(
        before: Option<&SortKey>,
        after: Option<&SortKey>,
    ) -> Result<SortKey, CoreError> {
        let a = before.map(|k| k.0.as_bytes());
        let b = after.map(|k| k.0.as_bytes());
        if let (Some(a), Some(b)) = (a, b)
            && a >= b
        {
            return Err(order_error(a, b));
        }
        key_between(a, b).map(into_key)
    }

    /// `n` keys in increasing order, all strictly between `before` and `after`, spread so that
    /// none of them is needlessly long. Used for bulk inserts and for rebalancing long keys.
    pub fn n_between(
        before: Option<&SortKey>,
        after: Option<&SortKey>,
        n: usize,
    ) -> Result<Vec<SortKey>, CoreError> {
        let a = before.map(|k| k.0.as_bytes().to_vec());
        let b = after.map(|k| k.0.as_bytes().to_vec());
        if let (Some(a), Some(b)) = (&a, &b)
            && a >= b
        {
            return Err(order_error(a, b));
        }
        let mut out = Vec::with_capacity(n);
        n_keys_between(a, b, n, &mut out)?;
        Ok(out.into_iter().map(into_key).collect())
    }
}

impl fmt::Display for SortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for SortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SortKey({})", self.0)
    }
}

impl Serialize for SortKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SortKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// Keys are built only from [`DIGITS`], so they are always valid UTF-8.
fn into_key(bytes: Vec<u8>) -> SortKey {
    SortKey(bytes.into_iter().map(char::from).collect())
}

fn order_error(a: &[u8], b: &[u8]) -> CoreError {
    CoreError::SortKeyOrder(
        String::from_utf8_lossy(a).into_owned(),
        String::from_utf8_lossy(b).into_owned(),
    )
}

fn exhausted(key: &[u8]) -> CoreError {
    CoreError::SortKeyExhausted(String::from_utf8_lossy(key).into_owned())
}

fn digit_value(c: u8) -> Option<usize> {
    match c {
        b'0'..=b'9' => Some(usize::from(c - b'0')),
        b'A'..=b'Z' => Some(usize::from(c - b'A') + 10),
        b'a'..=b'z' => Some(usize::from(c - b'a') + 36),
        _ => None,
    }
}

/// Value of a byte already known to be a digit.
fn value(c: u8) -> usize {
    digit_value(c).unwrap_or(0)
}

fn integer_length(head: u8) -> Option<usize> {
    match head {
        b'a'..=b'z' => Some(usize::from(head - b'a') + 2),
        b'A'..=b'Z' => Some(usize::from(b'Z' - head) + 2),
        _ => None,
    }
}

fn validate(key: &[u8]) -> Result<(), ()> {
    let head = *key.first().ok_or(())?;
    let int_len = integer_length(head).ok_or(())?;
    if key.len() < int_len || key.iter().any(|&c| digit_value(c).is_none()) {
        return Err(());
    }
    if key == SMALLEST_INTEGER.as_bytes() || (key.len() > int_len && key.last() == Some(&ZERO)) {
        return Err(());
    }
    Ok(())
}

/// Splits a valid key into its integer part and fraction.
fn split(key: &[u8]) -> (&[u8], &[u8]) {
    let int_len = integer_length(key[0]).unwrap_or(key.len()).min(key.len());
    key.split_at(int_len)
}

/// A fraction strictly between fractions `a` and `b` (`None`: no upper bound). Neither ends in
/// `0`, and `a < b`.
fn midpoint(a: &[u8], b: Option<&[u8]>) -> Vec<u8> {
    if let Some(b) = b {
        // Skip the common prefix, reading missing digits of `a` as zero.
        let mut n = 0;
        while n < b.len() && a.get(n).copied().unwrap_or(ZERO) == b[n] {
            n += 1;
        }
        if n > 0 {
            let mut out = b[..n].to_vec();
            out.extend(midpoint(a.get(n..).unwrap_or(&[]), Some(&b[n..])));
            return out;
        }
    }
    let digit_a = a.first().map_or(0, |&c| value(c));
    let digit_b = b.and_then(|b| b.first()).map_or(BASE, |&c| value(c));
    if digit_b - digit_a > 1 {
        vec![DIGITS[(digit_a + digit_b).div_ceil(2)]]
    } else if let Some(b) = b.filter(|b| b.len() > 1) {
        vec![b[0]]
    } else {
        let mut out = vec![DIGITS[digit_a]];
        out.extend(midpoint(a.get(1..).unwrap_or(&[]), None));
        out
    }
}

fn increment_integer(x: &[u8]) -> Option<Vec<u8>> {
    let head = x[0];
    let mut digits = x[1..].to_vec();
    let mut carry = true;
    for d in digits.iter_mut().rev() {
        let next = value(*d) + 1;
        if next == BASE {
            *d = ZERO;
        } else {
            *d = DIGITS[next];
            carry = false;
            break;
        }
    }
    if !carry {
        return Some([&[head][..], &digits].concat());
    }
    match head {
        b'Z' => Some(vec![b'a', ZERO]),
        b'z' => None,
        _ => {
            let head = head + 1;
            if head > b'a' {
                digits.push(ZERO);
            } else {
                digits.pop();
            }
            Some([&[head][..], &digits].concat())
        }
    }
}

fn decrement_integer(x: &[u8]) -> Option<Vec<u8>> {
    let head = x[0];
    let mut digits = x[1..].to_vec();
    let mut borrow = true;
    for d in digits.iter_mut().rev() {
        let current = value(*d);
        if current == 0 {
            *d = LARGEST_DIGIT;
        } else {
            *d = DIGITS[current - 1];
            borrow = false;
            break;
        }
    }
    if !borrow {
        return Some([&[head][..], &digits].concat());
    }
    match head {
        b'a' => Some(vec![b'Z', LARGEST_DIGIT]),
        b'A' => None,
        _ => {
            let head = head - 1;
            if head < b'Z' {
                digits.push(LARGEST_DIGIT);
            } else {
                digits.pop();
            }
            Some([&[head][..], &digits].concat())
        }
    }
}

/// Both keys valid, and `a < b` when both are given.
fn key_between(a: Option<&[u8]>, b: Option<&[u8]>) -> Result<Vec<u8>, CoreError> {
    match (a, b) {
        (None, None) => Ok(b"a0".to_vec()),
        (None, Some(b)) => {
            let (int_b, frac_b) = split(b);
            if int_b == SMALLEST_INTEGER.as_bytes() {
                return Ok([int_b, &midpoint(&[], Some(frac_b))].concat());
            }
            if !frac_b.is_empty() {
                return Ok(int_b.to_vec());
            }
            let previous = decrement_integer(int_b).ok_or_else(|| exhausted(b))?;
            if previous == SMALLEST_INTEGER.as_bytes() {
                // The smallest integer cannot stand alone; use a fraction of it instead.
                return Ok([previous.as_slice(), &midpoint(&[], None)].concat());
            }
            Ok(previous)
        }
        (Some(a), None) => {
            let (int_a, frac_a) = split(a);
            Ok(match increment_integer(int_a) {
                Some(next) => next,
                None => [int_a, &midpoint(frac_a, None)].concat(),
            })
        }
        (Some(a), Some(b)) => {
            let (int_a, frac_a) = split(a);
            let (int_b, frac_b) = split(b);
            if int_a == int_b {
                return Ok([int_a, &midpoint(frac_a, Some(frac_b))].concat());
            }
            let next = increment_integer(int_a).ok_or_else(|| exhausted(a))?;
            if next.as_slice() < b {
                Ok(next)
            } else {
                Ok([int_a, &midpoint(frac_a, None)].concat())
            }
        }
    }
}

fn n_keys_between(
    a: Option<Vec<u8>>,
    b: Option<Vec<u8>>,
    n: usize,
    out: &mut Vec<Vec<u8>>,
) -> Result<(), CoreError> {
    match n {
        0 => Ok(()),
        1 => {
            out.push(key_between(a.as_deref(), b.as_deref())?);
            Ok(())
        }
        _ if b.is_none() => {
            let mut c = key_between(a.as_deref(), None)?;
            for _ in 1..n {
                let next = key_between(Some(&c), None)?;
                out.push(std::mem::replace(&mut c, next));
            }
            out.push(c);
            Ok(())
        }
        _ if a.is_none() => {
            let mut keys = Vec::with_capacity(n);
            let mut c = key_between(None, b.as_deref())?;
            for _ in 1..n {
                let next = key_between(None, Some(&c))?;
                keys.push(std::mem::replace(&mut c, next));
            }
            keys.push(c);
            keys.reverse();
            out.extend(keys);
            Ok(())
        }
        _ => {
            let mid = n / 2;
            let c = key_between(a.as_deref(), b.as_deref())?;
            n_keys_between(a, Some(c.clone()), mid, out)?;
            out.push(c.clone());
            n_keys_between(Some(c), b, n - mid - 1, out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> SortKey {
        SortKey::parse(s).unwrap()
    }

    fn between(a: Option<&str>, b: Option<&str>) -> Result<String, CoreError> {
        let a = a.map(key);
        let b = b.map(key);
        SortKey::between(a.as_ref(), b.as_ref()).map(|k| k.0)
    }

    #[test]
    fn known_keys() {
        let cases: &[(Option<&str>, Option<&str>, &str)] = &[
            (None, None, "a0"),
            (Some("a0"), None, "a1"),
            (Some("a1"), None, "a2"),
            (Some("a0"), Some("a1"), "a0V"),
            (Some("a1"), Some("a2"), "a1V"),
            (Some("a0V"), Some("a1"), "a0l"),
            (Some("Zz"), Some("a0"), "ZzV"),
            (Some("Zz"), Some("a1"), "a0"),
            (None, Some("Y00"), "Xzzz"),
            (Some("bzz"), None, "c000"),
            (Some("a0"), Some("a0V"), "a0G"),
            (Some("a0"), Some("a0G"), "a08"),
            (Some("b125"), Some("b129"), "b127"),
            (Some("a0"), Some("a1V"), "a1"),
            (Some("Zz"), Some("a01"), "a0"),
            (None, Some("a0V"), "a0"),
            (None, Some("b999"), "b99"),
            (None, Some("a0"), "Zz"),
            (
                None,
                Some("A000000000000000000000000001"),
                "A000000000000000000000000000V",
            ),
            (
                Some("zzzzzzzzzzzzzzzzzzzzzzzzzzy"),
                None,
                "zzzzzzzzzzzzzzzzzzzzzzzzzzz",
            ),
            (
                Some("zzzzzzzzzzzzzzzzzzzzzzzzzzz"),
                None,
                "zzzzzzzzzzzzzzzzzzzzzzzzzzzV",
            ),
        ];
        for &(a, b, want) in cases {
            assert_eq!(between(a, b).unwrap(), want, "between({a:?}, {b:?})");
        }
        assert_eq!(SortKey::first().as_str(), "a0");
    }

    #[test]
    fn invalid_keys_are_rejected() {
        for bad in [
            "",
            "0",
            "a",
            "a00",
            "a0V0",
            "A00000000000000000000000000",
            "b1",
            "a0!",
            "é0",
            "Zz0",
        ] {
            assert!(SortKey::parse(bad).is_err(), "{bad:?} accepted");
        }
        assert!(serde_json::from_str::<SortKey>("\"a00\"").is_err());
        let ok: SortKey = serde_json::from_str("\"a0V\"").unwrap();
        assert_eq!(serde_json::to_string(&ok).unwrap(), "\"a0V\"");
        assert_eq!(format!("{ok} {ok:?}"), "a0V SortKey(a0V)");
    }

    #[test]
    fn wrong_order_and_exhaustion_are_errors() {
        assert!(matches!(
            between(Some("a1"), Some("a0")),
            Err(CoreError::SortKeyOrder(..))
        ));
        assert!(matches!(
            between(Some("a1"), Some("a1")),
            Err(CoreError::SortKeyOrder(..))
        ));
        let smallest_whole = "Azzzzzzzzzzzzzzzzzzzzzzzzzz";
        // Below the smallest integer there is still room, as a fraction of it.
        assert!(between(None, Some(smallest_whole)).unwrap().as_str() < smallest_whole);
        assert!(matches!(
            SortKey::n_between(Some(&key("a1")), Some(&key("a0")), 3),
            Err(CoreError::SortKeyOrder(..))
        ));
        // Just above the smallest integer, the key before it is a fraction of the smallest.
        assert_eq!(
            between(None, Some("A00000000000000000000000001")).unwrap(),
            "A00000000000000000000000000V"
        );
        let largest = "zzzzzzzzzzzzzzzzzzzzzzzzzzz";
        // Above the largest integer, keys continue as fractions of it.
        assert_eq!(
            between(Some(largest), Some("zzzzzzzzzzzzzzzzzzzzzzzzzzzV")).unwrap(),
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzG"
        );
        assert_eq!(
            between(Some(largest), Some("zzzzzzzzzzzzzzzzzzzzzzzzzzz1")).unwrap(),
            "zzzzzzzzzzzzzzzzzzzzzzzzzzz0V"
        );
    }

    #[test]
    fn integer_parts_roll_over() {
        assert_eq!(increment_integer(b"az").as_deref(), Some(&b"b00"[..]));
        assert_eq!(increment_integer(b"Zz").as_deref(), Some(&b"a0"[..]));
        assert_eq!(increment_integer(b"Yzz").as_deref(), Some(&b"Z0"[..]));
        assert_eq!(decrement_integer(b"b00").as_deref(), Some(&b"az"[..]));
        assert_eq!(decrement_integer(b"b01").as_deref(), Some(&b"b00"[..]));
        assert_eq!(decrement_integer(b"a0").as_deref(), Some(&b"Zz"[..]));
        assert_eq!(decrement_integer(b"Z0").as_deref(), Some(&b"Yzz"[..]));
        assert_eq!(decrement_integer(SMALLEST_INTEGER.as_bytes()), None);
        assert_eq!(increment_integer(b"zzzzzzzzzzzzzzzzzzzzzzzzzzz"), None);
    }

    #[test]
    fn n_between_spreads_keys_evenly() {
        for (a, b) in [
            (None, None),
            (Some("a0"), None),
            (None, Some("a0")),
            (Some("a0"), Some("a1")),
        ] {
            let a = a.map(key);
            let b = b.map(key);
            for n in [0, 1, 2, 3, 10, 100] {
                let keys = SortKey::n_between(a.as_ref(), b.as_ref(), n).unwrap();
                assert_eq!(keys.len(), n);
                let all: Vec<&SortKey> = a.iter().chain(&keys).chain(b.iter()).collect();
                assert!(
                    all.windows(2).all(|w| w[0] < w[1]),
                    "n={n} not strictly increasing: {all:?}"
                );
                assert!(keys.iter().all(|k| k.as_str().len() <= 5), "{keys:?}");
            }
        }
    }

    proptest::proptest! {
        #[test]
        fn random_inserts_stay_valid_and_ordered(
            positions in proptest::collection::vec(proptest::prelude::any::<proptest::sample::Index>(), 1..400)
        ) {
            let mut keys: Vec<SortKey> = Vec::new();
            for position in positions {
                let i = position.index(keys.len() + 1);
                let new = SortKey::between(i.checked_sub(1).and_then(|j| keys.get(j)), keys.get(i)).unwrap();
                proptest::prop_assert!(SortKey::parse(new.as_str()).is_ok());
                keys.insert(i, new);
                proptest::prop_assert!(keys.windows(2).all(|w| w[0] < w[1]));
            }
        }
    }

    #[test]
    fn appending_ten_thousand_keys_keeps_them_short() {
        let mut last: Option<SortKey> = None;
        for _ in 0..10_000 {
            last = Some(SortKey::between(last.as_ref(), None).unwrap());
        }
        assert!(last.unwrap().as_str().len() <= 4);
        let mut first: Option<SortKey> = None;
        for _ in 0..10_000 {
            first = Some(SortKey::between(None, first.as_ref()).unwrap());
        }
        assert!(first.unwrap().as_str().len() <= 4);
    }
}
