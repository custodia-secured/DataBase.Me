// SPDX-License-Identifier: AGPL-3.0-or-later
//! MEASUREMENT ONLY, on a scratch branch that is not for merging. The `pdf` target with libfuzzer-sys's
//! aborting panic hook replaced, so a panic the importer's `catch_unwind` contains stays contained and
//! the run goes on. Each new panic site is printed once. A panic past the containment still aborts:
//! libfuzzer-sys's own `catch_unwind` around the target ends in `process::abort`.

#![no_main]

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::Duration;

use libfuzzer_sys::fuzz_target;

static SEEN: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

fuzz_target!(init: {
    std::panic::set_hook(Box::new(|info| {
        let place = info.location().map_or_else(String::new, ToString::to_string);
        if let Ok(mut seen) = SEEN.lock()
            && seen.insert(place.clone())
        {
            let what = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_default();
            eprintln!("CONTAINED-PANIC site {}: {place}: {what}", seen.len());
        }
    }));
}, |data: &[u8]| {
    let extract = dbme_ingest::probe::pdf("fuzz.pdf", data, Duration::from_secs(2));
    assert_eq!(extract.objects.len(), 1);
    assert_eq!(extract.objects[0].attachments.len(), 1);
});
