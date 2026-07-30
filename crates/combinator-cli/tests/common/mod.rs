//! Shared helpers for the black-box CLI test binaries.
//!
//! Not every test binary uses every helper, so unused items are expected here.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A freshly created temporary directory, removed when the guard drops.
///
/// Naming scratch files after the process ID alone is not enough. Every test in
/// a binary shares one PID, so it distinguishes runs rather than tests, and the
/// OS recycles PIDs — a run that panicked before cleaning up can leave a file
/// that a later run collides with. `combinator` refuses to write over an
/// existing `--output` path without `--overwrite`, so such a leftover turns
/// into an `OUTPUT_EXISTS` failure unrelated to the behavior under test.
///
/// Combining the PID with the clock and a process-wide counter keeps paths
/// distinct across tests, threads and runs, and creating a directory that did
/// not previously exist guarantees nothing stale is inside it. Cleanup runs in
/// `Drop`, so it happens even when a test panics — which in turn lets tests
/// assert in their natural order instead of cleaning up before asserting.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Creates a uniquely named directory, with `label` retained to keep the
    /// path recognizable while debugging a failure.
    pub fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "combinator_{label}_{}_{nanos}_{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&path)
            .unwrap_or_else(|e| panic!("failed creating temp dir {}: {e}", path.display()));
        Self { path }
    }

    /// Path of a file inside this directory. The file is not created.
    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The guard's contract: a directory that exists, is empty, and is never handed
/// out twice — the three properties that keep leftovers from one run out of the
/// next.
#[test]
fn temp_dir_is_fresh_empty_and_unique() {
    let first = TempDir::new("selftest");
    let second = TempDir::new("selftest");
    assert_ne!(first.path(), second.path());
    for dir in [&first, &second] {
        assert!(
            dir.path().is_dir(),
            "{} is not a directory",
            dir.path().display()
        );
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "{} was not empty",
            dir.path().display()
        );
    }

    let leaked = {
        let temporary = TempDir::new("selftest");
        std::fs::write(temporary.join("scratch.txt"), "x").unwrap();
        temporary.path().to_path_buf()
    };
    assert!(!leaked.exists(), "{} outlived its guard", leaked.display());
}

/// Cleanup must survive a failing test, or the leftovers it leaves behind are
/// exactly what the next run trips over.
#[test]
fn temp_dir_is_removed_when_a_test_panics() {
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(PathBuf::new()));
    let captured = std::sync::Arc::clone(&recorded);

    let result = std::panic::catch_unwind(move || {
        let temporary = TempDir::new("selftest_panic");
        std::fs::write(temporary.join("scratch.txt"), "x").unwrap();
        *captured.lock().unwrap() = temporary.path().to_path_buf();
        panic!("simulated test failure");
    });

    assert!(result.is_err(), "the closure was expected to panic");
    let path = recorded.lock().unwrap().clone();
    assert!(!path.as_os_str().is_empty(), "no path was recorded");
    assert!(
        !path.exists(),
        "{} survived a panicking test",
        path.display()
    );
}
