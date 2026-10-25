// SPDX-License-Identifier: AGPL-3.0-or-later
//! Identifiers and time (ADR-0003).
//!
//! Everything with an identity has a ULID: a 48-bit millisecond timestamp followed by 80 random
//! bits, written as 26 upper-case Crockford base32 characters. IDs come from an [`IdGenerator`],
//! which never hands out the same or a smaller ID twice, and timestamps from a [`Clock`], which
//! tests can control.

use std::fmt;
use std::str::FromStr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::CoreError;

/// A ULID. Sorts by creation time; safe in file names and block anchors.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(transparent))]
pub struct Id(#[cfg_attr(feature = "specta", specta(type = String))] ulid::Ulid);

/// The largest timestamp a ULID can carry (2^48 - 1 milliseconds, in the year 10889).
const MAX_ULID_MS: u64 = (1 << 48) - 1;
/// The 80 random bits of a ULID.
const RANDOM_MASK: u128 = (1 << 80) - 1;

impl Id {
    /// Builds an ID from its parts. `ms` is clamped to what a ULID can hold and `random` is
    /// truncated to 80 bits.
    pub fn from_parts(ms: u64, random: u128) -> Self {
        Self(ulid::Ulid::from_parts(
            ms.min(MAX_ULID_MS),
            random & RANDOM_MASK,
        ))
    }

    /// Parses the 26-character text form. Lower case is accepted; the canonical form is upper
    /// case.
    pub fn parse(text: &str) -> Result<Self, CoreError> {
        // 26 base32 characters carry 130 bits; a first character above `7` would overflow the 128
        // bits of a ULID, and the decoder would silently drop the excess.
        if text.len() != 26 || !matches!(text.as_bytes()[0], b'0'..=b'7') {
            return Err(CoreError::InvalidId(text.to_owned()));
        }
        ulid::Ulid::from_string(text)
            .map(Self)
            .map_err(|_| CoreError::InvalidId(text.to_owned()))
    }

    /// The creation time in milliseconds since the Unix epoch.
    pub fn timestamp_ms(&self) -> u64 {
        self.0.timestamp_ms()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.0)
    }
}

impl FromStr for Id {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// A source of the current time, in milliseconds since the Unix epoch (UTC).
pub trait Clock: Send + Sync + fmt::Debug {
    /// The current time.
    fn now_ms(&self) -> i64;
}

/// The operating system's clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}

/// A clock that only moves when told to. For tests and replays.
#[derive(Debug, Default)]
pub struct ManualClock(AtomicI64);

impl ManualClock {
    /// A clock showing `ms`.
    pub fn new(ms: i64) -> Self {
        Self(AtomicI64::new(ms))
    }

    /// Sets the time.
    pub fn set(&self, ms: i64) {
        self.0.store(ms, Ordering::SeqCst);
    }

    /// Moves the time forward by `ms` and returns the new time.
    pub fn advance(&self, ms: i64) -> i64 {
        self.0.fetch_add(ms, Ordering::SeqCst) + ms
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Hands out strictly increasing IDs.
///
/// Within one millisecond, or if the clock steps backwards, the next ID reuses the last timestamp
/// and increments the random part, so IDs from one generator always sort in the order they were
/// made.
#[derive(Debug, Default)]
pub struct IdGenerator {
    last: Mutex<Option<(u64, u128)>>,
}

impl IdGenerator {
    /// A generator with no history.
    pub fn new() -> Self {
        Self::default()
    }

    /// The next ID for time `now_ms` (negative times count as zero).
    pub fn next(&self, now_ms: i64) -> Result<Id, CoreError> {
        let ms = u64::try_from(now_ms).unwrap_or(0).min(MAX_ULID_MS);
        let mut last = self
            .last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (ms, random) = match *last {
            Some((last_ms, last_random)) if ms <= last_ms => {
                if last_random < RANDOM_MASK {
                    (last_ms, last_random + 1)
                } else if last_ms < MAX_ULID_MS {
                    (last_ms + 1, fresh_random()?)
                } else {
                    return Err(CoreError::InvalidId("ULID space exhausted".to_owned()));
                }
            }
            _ => (ms, fresh_random()?),
        };
        *last = Some((ms, random));
        Ok(Id::from_parts(ms, random))
    }
}

/// 80 random bits, with the top bit clear so that incrementing within a millisecond has room.
fn fresh_random() -> Result<u128, CoreError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes[6..]).map_err(|_| CoreError::Random)?;
    Ok(u128::from_be_bytes(bytes) & (RANDOM_MASK >> 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_form_round_trips_and_accepts_lower_case() {
        let id = Id::from_parts(1_761_380_471_000, 0x0123_4567_89AB_CDEF_0123);
        let text = id.to_string();
        assert_eq!(text.len(), 26);
        assert_eq!(text, text.to_uppercase());
        assert_eq!(Id::parse(&text).unwrap(), id);
        assert_eq!(Id::parse(&text.to_lowercase()).unwrap(), id);
        assert_eq!(text.parse::<Id>().unwrap(), id);
        assert_eq!(id.timestamp_ms(), 1_761_380_471_000);
        assert_eq!(format!("{id:?}"), format!("Id({text})"));
    }

    #[test]
    fn bad_text_is_rejected() {
        for bad in [
            "",
            "01JB3R7Q8Y5K3M2N4P6Q8R0S2",
            "01JB3R7Q8Y5K3M2N4P6Q8R0S2TX",
            "01JB3R7Q8Y5K3M2N4P6Q8R0SUU",
            "81JB3R7Q8Y5K3M2N4P6Q8R0S2T",
        ] {
            assert!(Id::parse(bad).is_err(), "{bad:?} parsed");
        }
    }

    #[test]
    fn serde_uses_the_text_form() {
        let id = Id::from_parts(5, 7);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<Id>(&json).unwrap(), id);
        assert!(serde_json::from_str::<Id>("\"nope\"").is_err());
        assert!(serde_json::from_str::<Id>("3").is_err());
    }

    #[test]
    fn generator_is_strictly_increasing_even_when_time_stands_still_or_goes_back() {
        let ids = IdGenerator::new();
        let mut previous = ids.next(1_000).unwrap();
        for now in [1_000, 1_000, 999, 0, -5, 1_000, 1_001, 1_001] {
            let id = ids.next(now).unwrap();
            assert!(id > previous, "{id:?} <= {previous:?}");
            previous = id;
        }
        assert_eq!(previous.timestamp_ms(), 1_001);
    }

    #[test]
    fn generator_rolls_into_the_next_millisecond_when_the_random_part_is_full() {
        let ids = IdGenerator::new();
        *ids.last.lock().unwrap() = Some((10, RANDOM_MASK));
        let id = ids.next(10).unwrap();
        assert_eq!(id.timestamp_ms(), 11);

        *ids.last.lock().unwrap() = Some((MAX_ULID_MS, RANDOM_MASK));
        assert!(ids.next(0).is_err());
    }

    #[test]
    fn huge_timestamps_are_clamped() {
        assert_eq!(Id::from_parts(u64::MAX, 0).timestamp_ms(), MAX_ULID_MS);
        assert_eq!(
            IdGenerator::new().next(i64::MAX).unwrap().timestamp_ms(),
            MAX_ULID_MS
        );
    }

    #[test]
    fn clocks() {
        let clock = ManualClock::new(100);
        assert_eq!(clock.now_ms(), 100);
        assert_eq!(clock.advance(5), 105);
        clock.set(7);
        assert_eq!(clock.now_ms(), 7);
        assert!(SystemClock.now_ms() > 1_700_000_000_000);
    }
}
