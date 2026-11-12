//! State shared between commands.

use diskscape_core::scan::CancelToken;
use diskscape_core::tree::Tree;
use std::sync::atomic::AtomicU64;
use std::sync::{Mutex, RwLock};

#[derive(Default)]
pub struct AppState {
    /// The tree currently shown, if a scan or import has finished.
    pub tree: RwLock<Option<Tree>>,
    /// Token of the scan in flight.
    pub cancel: Mutex<CancelToken>,
    /// Incremented by every `start_scan`; a scan only reports back while its
    /// number is still the current one, so a superseded scan stays silent.
    pub generation: AtomicU64,
    /// How long the scan that produced `tree` took.
    pub elapsed_ms: AtomicU64,
}

/// Message used when a command needs a tree and there is none.
pub const NO_TREE: &str = "nothing has been scanned yet";
