#![no_main]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use libfuzzer_sys::fuzz_target;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fuzz_target!(|data: &[u8]| {
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "docxlite-fuzz-parse-{}-{counter}.docx",
        std::process::id()
    ));
    if std::fs::write(&path, data).is_err() {
        return;
    }
    let _ = docxlite::load_document(&path, Arc::new(AtomicBool::new(false)));
    let _ = std::fs::remove_file(&path);
});
