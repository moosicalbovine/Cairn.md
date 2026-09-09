use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::domain::library::LibraryError;

#[derive(Clone, Default)]
pub struct WatcherHints {
    scan_requested: Arc<AtomicBool>,
}

impl WatcherHints {
    pub fn request_scan(&self) {
        self.scan_requested.store(true, Ordering::Release);
    }

    pub fn take_scan_request(&self) -> bool {
        self.scan_requested.swap(false, Ordering::AcqRel)
    }
}

pub struct LibraryWatcher {
    _watcher: RecommendedWatcher,
}

impl LibraryWatcher {
    pub fn start(root: &Path, hints: WatcherHints) -> Result<Self, LibraryError> {
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.is_ok() {
                    // Events deliberately carry no mutation semantics. They only request an
                    // authoritative scan, which is debounced by the command caller.
                    hints.request_scan();
                }
            })
            .map_err(LibraryError::watcher)?;
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(LibraryError::watcher)?;
        Ok(Self { _watcher: watcher })
    }
}
