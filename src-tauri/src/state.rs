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
    /// The Quick Look window currently open, if any (macOS).
    pub preview: Mutex<Option<Preview>>,
}

/// A running system preview and the node it shows.
pub struct Preview {
    pub id: diskscape_core::tree::NodeId,
    pub child: std::process::Child,
}

impl AppState {
    /// Closes the preview window, if one is open.
    pub fn close_preview(&self) {
        if let Ok(mut slot) = self.preview.lock() {
            if let Some(mut preview) = slot.take() {
                let _ = preview.child.kill();
                let _ = preview.child.wait();
            }
        }
    }
}

/// Message used when a command needs a tree and there is none.
pub const NO_TREE: &str = "nothing has been scanned yet";
