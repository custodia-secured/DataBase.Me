// SPDX-License-Identifier: AGPL-3.0-or-later
//! Content hashes (ADR-0003): `<algorithm>:<lowercase hex>`, today always BLAKE3-256.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::CoreError;

/// The algorithm prefix this version writes.
pub const ALGORITHM: &str = "blake3";
const HEX_LEN: usize = 64;

/// A hash with its algorithm, for example `blake3:af13…` (64 hex digits).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(transparent))]
pub struct ContentHash(String);

impl ContentHash {
    /// The BLAKE3 hash of `bytes`.
    pub fn of(bytes: &[u8]) -> Self {
        Self::from_blake3(&blake3::hash(bytes))
    }

    fn from_blake3(hash: &blake3::Hash) -> Self {
        Self(format!("{ALGORITHM}:{}", hash.to_hex()))
    }

    /// Parses `<algorithm>:<hex>`. Upper-case hex is accepted and normalised to lower case. An
    /// algorithm other than BLAKE3 is reported as unknown, never guessed.
    pub fn parse(text: &str) -> Result<Self, CoreError> {
        let Some((algorithm, hex)) = text.split_once(':') else {
            return Err(CoreError::InvalidHash(text.to_owned()));
        };
        if algorithm != ALGORITHM {
            return Err(
                if algorithm.is_empty() || !algorithm.bytes().all(|b| b.is_ascii_alphanumeric()) {
                    CoreError::InvalidHash(text.to_owned())
                } else {
                    CoreError::UnknownHashAlgorithm(algorithm.to_owned())
                },
            );
        }
        if hex.len() != HEX_LEN || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CoreError::InvalidHash(text.to_owned()));
        }
        Ok(Self(format!("{ALGORITHM}:{}", hex.to_ascii_lowercase())))
    }

    /// The whole text, `blake3:<hex>`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The hex digits alone.
    pub fn hex(&self) -> &str {
        &self.0[ALGORITHM.len() + 1..]
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentHash({})", self.0)
    }
}

impl Serialize for ContentHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// Hashes data that arrives in pieces, such as a file being copied into the blob store.
#[derive(Debug, Default, Clone)]
pub struct ContentHasher(blake3::Hasher);

impl ContentHasher {
    /// A hasher that has seen nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds more bytes.
    pub fn update(&mut self, bytes: &[u8]) -> &mut Self {
        self.0.update(bytes);
        self
    }

    /// The hash of everything fed so far.
    pub fn finalize(&self) -> ContentHash {
        ContentHash::from_blake3(&self.0.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // BLAKE3 of the empty input, from the BLAKE3 test vectors.
    const EMPTY: &str = "blake3:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";

    #[test]
    fn hashes_match_the_reference_and_the_streaming_form() {
        assert_eq!(ContentHash::of(b"").as_str(), EMPTY);
        let mut hasher = ContentHasher::new();
        hasher.update(b"hello ").update(b"world");
        assert_eq!(hasher.finalize(), ContentHash::of(b"hello world"));
        assert_eq!(ContentHash::of(b"").hex().len(), 64);
    }

    #[test]
    fn parsing() {
        let hash = ContentHash::parse(EMPTY).unwrap();
        assert_eq!(hash.to_string(), EMPTY);
        assert_eq!(
            ContentHash::parse(&EMPTY.to_uppercase().replacen("BLAKE3", "blake3", 1)).unwrap(),
            hash
        );
        assert!(
            matches!(ContentHash::parse("sha256:abcd"), Err(CoreError::UnknownHashAlgorithm(a)) if a == "sha256")
        );
        for bad in [
            "",
            "blake3",
            "blake3:",
            "blake3:xyz",
            ":abcd",
            "bla ke3:00",
            &EMPTY[..EMPTY.len() - 1],
        ] {
            assert!(
                matches!(ContentHash::parse(bad), Err(CoreError::InvalidHash(_))),
                "{bad:?}"
            );
        }
        assert_eq!(format!("{hash:?}"), format!("ContentHash({EMPTY})"));
        let json = serde_json::to_string(&hash).unwrap();
        assert_eq!(serde_json::from_str::<ContentHash>(&json).unwrap(), hash);
        assert!(serde_json::from_str::<ContentHash>("\"md5:00\"").is_err());
    }
}
