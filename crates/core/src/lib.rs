// SPDX-License-Identifier: AGPL-3.0-or-later
//! `dbme-core` is the DataBase.Me domain layer: objects, types, relations, the op log, the query
//! DSL and the policies that guard them. It has no I/O of its own; storage lives in `dbme-store`.
//!
//! This crate must keep compiling for iOS and Android (ADR-0008), so it may not depend on anything
//! desktop-only.
//!
//! At milestone M0 it only carries the identity and version facts that the desktop app and the
//! `dbme` CLI report. The object model arrives in M1.

/// The product name shown to people. Code identifiers use the short form `dbme` (ADR-0001).
pub const PRODUCT_NAME: &str = "DataBase.Me";

/// The DataBase.Me release this crate was built for.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Version of the on-disk vault format described in `docs/FORMAT.md`.
///
/// `0` is the draft format: nothing reads or writes vaults yet, and there is no compatibility
/// promise until FORMAT.md v0.1 ships with the MVP (M7).
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
