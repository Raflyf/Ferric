//! Milestone M5 UniFFI DTOs for Offline Downloads & Storage Diagnostics.

use ferric_controller::types::StorageStatsSnapshot;

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiStorageStats {
    pub offline_track_count: u32,
    pub offline_bytes: u64,
    pub stream_cache_bytes: u64,
    pub stream_cache_files: u32,
    pub artwork_cache_bytes: u64,
    pub database_bytes: u64,
}

impl From<StorageStatsSnapshot> for FfiStorageStats {
    fn from(s: StorageStatsSnapshot) -> Self {
        Self {
            offline_track_count: u32::try_from(s.offline_track_count).unwrap_or(u32::MAX),
            offline_bytes: s.offline_bytes,
            stream_cache_bytes: s.stream_cache_bytes,
            stream_cache_files: u32::try_from(s.stream_cache_files).unwrap_or(u32::MAX),
            artwork_cache_bytes: s.artwork_cache_bytes,
            database_bytes: s.database_bytes,
        }
    }
}
