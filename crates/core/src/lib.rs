// SPDX-License-Identifier: AGPL-3.0-or-later
//! `dbme-core` is the DataBase.Me domain layer: identifiers, sort keys, hashes, the rows of the
//! data model and the rules they obey, and the ops that change them. It has no I/O of its own;
//! storage, and all SQL, live in `dbme-store`.
//!
//! This crate must keep compiling for iOS and Android (ADR-0008), so it may not depend on anything
//! desktop-only.

pub mod block;
pub mod chain;
mod error;
pub mod hash;
pub mod id;
pub mod model;
pub mod op;
pub mod seed;
pub mod sort_key;
pub mod value;

pub use error::CoreError;
pub use hash::{ContentHash, ContentHasher};
pub use id::{Clock, Id, IdGenerator, ManualClock, SystemClock};
pub use sort_key::SortKey;

/// The product name shown to people. Code identifiers use the short form `dbme` (ADR-0001).
pub const PRODUCT_NAME: &str = "DataBase.Me";

/// The DataBase.Me release this crate was built for.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Version of the on-disk vault format described in `docs/FORMAT.md`.
///
/// `0` is the draft format: there is no compatibility promise until FORMAT.md v0.1 ships with the
/// MVP (M7). Within version 0 the schema still only changes through numbered migrations.
pub const FORMAT_VERSION: u32 = 0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_semver() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "{VERSION} is not MAJOR.MINOR.PATCH");
        assert!(parts.iter().all(|p| p.parse::<u64>().is_ok()));
    }
}
