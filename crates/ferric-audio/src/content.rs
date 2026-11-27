//! Platform-independent audio file opener supporting both standard POSIX paths and
//! Android Scoped Storage (`content://`) URIs via an injected `ContentOpener` FD bridge.

use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

/// Platform bridge that resolves Android `content://` URIs into readable `std::fs::File` handles.
pub trait ContentOpener: Send + Sync {
    fn open_file(&self, uri: &str) -> io::Result<File>;
}

static CONTENT_OPENER: OnceLock<RwLock<Option<Arc<dyn ContentOpener>>>> = OnceLock::new();

fn content_opener_lock() -> &'static RwLock<Option<Arc<dyn ContentOpener>>> {
    CONTENT_OPENER.get_or_init(|| RwLock::new(None))
}

/// Register the global Android `ContentResolver` bridge before opening `content://` tracks.
pub fn register_content_opener(opener: Arc<dyn ContentOpener>) {
    let mut guard = content_opener_lock()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(opener);
}

#[cfg(test)]
static TEST_CONTENT_OPENER_MUTEX: OnceLock<std::sync::Mutex<()>> = OnceLock::new();

#[cfg(test)]
pub(crate) struct ScopedTestContentOpenerGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    prev: Option<Arc<dyn ContentOpener>>,
}

#[cfg(test)]
impl Drop for ScopedTestContentOpenerGuard {
    fn drop(&mut self) {
        let mut guard = content_opener_lock()
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = self.prev.take();
    }
}

#[cfg(test)]
pub(crate) fn register_scoped_content_opener_for_test(
    opener: Arc<dyn ContentOpener>,
) -> ScopedTestContentOpenerGuard {
    let lock = TEST_CONTENT_OPENER_MUTEX
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let prev = {
        let mut guard = content_opener_lock()
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.replace(opener)
    };
    ScopedTestContentOpenerGuard { _lock: lock, prev }
}

fn active_content_opener() -> Option<Arc<dyn ContentOpener>> {
    content_opener_lock()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Open either a regular filesystem path or an Android `content://` URI via the registered bridge.
pub fn open_audio_file(path: &Path) -> io::Result<File> {
    if let Some(raw) = path.to_str()
        && raw.starts_with("content://")
    {
        let opener = active_content_opener().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "no ContentOpener registered for content:// URI",
            )
        })?;
        return opener.open_file(raw);
    }
    File::open(path)
}
