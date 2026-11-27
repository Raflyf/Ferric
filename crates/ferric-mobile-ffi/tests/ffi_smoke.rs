use ferric_mobile_ffi::{
    FerricMobileCore, FfiAccountStatus, FfiBackgroundMode, FfiCollectionDetail, FfiConnectState,
    FfiContentOpener, FfiError, FfiEventSink, FfiHomeState, FfiLocalScanEntry, FfiLyricsState,
    FfiSearchResults, FfiSecretCipher, FfiTelemetry, FfiTrack, FfiVideoStreamsState,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

struct TestCipher;
impl FfiSecretCipher for TestCipher {
    fn encrypt(&self, plain: Vec<u8>) -> Result<Vec<u8>, FfiError> {
        Ok(plain.into_iter().map(|b| b ^ 0xA5).collect())
    }
    fn decrypt(&self, cipher: Vec<u8>) -> Result<Vec<u8>, FfiError> {
        Ok(cipher.into_iter().map(|b| b ^ 0xA5).collect())
    }
}

struct TestOpener;
impl FfiContentOpener for TestOpener {
    fn open_fd(&self, uri: String) -> Result<i32, FfiError> {
        Err(FfiError::Io {
            reason: format!("test opener cannot open {uri}"),
        })
    }
}

struct TestSink {
    now_playing_events: Arc<AtomicUsize>,
}

impl FfiEventSink for TestSink {
    fn on_playback_telemetry(&self, _telemetry: FfiTelemetry) {}
    fn on_now_playing_changed(&self, _track: Option<FfiTrack>) {
        self.now_playing_events.fetch_add(1, Ordering::Relaxed);
    }
    fn on_queue_changed(&self, _up_next: Vec<FfiTrack>) {}
    fn on_library_changed(&self) {}
    fn on_home_updated(&self, _home: FfiHomeState) {}
    fn on_search_results(&self, _results: FfiSearchResults) {}
    fn on_detail_loaded(&self, _detail: FfiCollectionDetail) {}
    fn on_accounts_changed(&self, _status: FfiAccountStatus) {}
    fn on_lyrics_loaded(&self, _lyrics: FfiLyricsState) {}
    fn on_video_streams_loaded(&self, _video: FfiVideoStreamsState) {}
    fn on_connect_state_changed(&self, _state: FfiConnectState) {}
    fn on_download_progress(&self, _track_id: String, _percent: u8) {}
    fn on_spotify_auth_expired(&self) {}
    fn on_toast(&self, _message: String) {}
}

#[test]
fn mobile_ffi_seeds_plays_and_persists_liquid_glass_theme() {
    let root = std::env::temp_dir().join(format!(
        "ferric_ffi_smoke_{}_{}",
        std::process::id(),
        fastrand::u64(..)
    ));
    let events = Arc::new(AtomicUsize::new(0));
    let core = FerricMobileCore::new_headless_for_test(
        root.join("data").to_string_lossy().into_owned(),
        root.join("cache").to_string_lossy().into_owned(),
        Box::new(TestCipher),
        Box::new(TestOpener),
        Box::new(TestSink {
            now_playing_events: Arc::clone(&events),
        }),
    )
    .expect("construct FerricMobileCore");

    assert!(!core.is_audio_started());
    let seeded = core.seed_demo_if_empty().expect("seed demo");
    assert_eq!(seeded, 4);
    let songs = core.list_songs(String::new()).expect("list songs");
    assert_eq!(songs.len(), 4);

    core.play_track(songs[0].id.clone()).expect("play track");
    assert!(core.is_audio_started());
    let mut playing = false;
    for _ in 0..80 {
        if core.get_telemetry().is_playing {
            playing = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    assert!(playing, "FFI play_track must start headless audio playback");
    assert!(events.load(Ordering::Relaxed) >= 1);

    let mut theme = core.get_theme();
    theme.name = "Liquid Glass".to_string();
    theme.background_mode = FfiBackgroundMode::NativeAcrylic;
    theme.acrylic_enabled = true;
    theme.player_layout = "liquid_glass".to_string();
    theme.editorial_mode = true;
    core.set_theme(theme).expect("save theme");
    assert_eq!(core.get_theme().player_layout, "liquid_glass");
    assert!(core.get_theme().editorial_mode);

    let scanned = core
        .scan_local_entries(vec![FfiLocalScanEntry {
            uri: "content://media/external/audio/media/901".to_string(),
            title: "Glass Horizon".to_string(),
            artist: "Ferric Test".to_string(),
            album: "M2 Suite".to_string(),
            duration_ms: 185_000,
            codec: "FLAC".to_string(),
            sample_rate: 48_000,
            bit_depth: 24,
            bitrate_kbps: 1411,
        }])
        .expect("scan local");
    assert_eq!(scanned, 1);
    let hits = core
        .instant_search_local("glass".to_string())
        .expect("instant search");
    assert_eq!(hits.len(), 1);
    assert!(core.toggle_like(hits[0].id.clone()).expect("like"));
    assert!(!core.get_account_status().spotify_connected);

    // M3: EQ presets, 24 AutoEq profiles, APO export/import, Theme Studio JSON roundtrip, Lyrics offset
    assert_eq!(core.search_autoeq(String::new()).len(), 24);
    let xm5 = core
        .apply_autoeq_profile("Sony WH-1000XM5".to_string())
        .expect("autoeq");
    assert_eq!(xm5.bands.len(), 10);
    let apo = core.export_eq_apo();
    assert!(apo.contains("Preamp:"));
    let reimported = core.import_eq_apo(apo).expect("reimport apo");
    assert_eq!(reimported.bands.len(), 10);

    let glass = core
        .apply_theme_preset("Liquid Glass".to_string())
        .expect("liquid glass");
    assert_eq!(glass.player_layout, "full_bleed");
    let theme_json = core.export_theme_json().expect("export theme");
    assert!(theme_json.contains("NativeAcrylic"));
    let imported_theme = core.import_theme_json(theme_json).expect("import theme");
    assert_eq!(imported_theme.name, "Liquid Glass");
    assert_eq!(core.nudge_lyrics_offset_ms(500), 500);
    assert_eq!(core.nudge_lyrics_offset_ms(-1000), -500);

    // M4: Zero-Pairing Connect snapshot + QR matrix generation
    let conn = core.get_connect_state();
    assert_eq!(conn.local_device_id, "test_android_01");
    assert!(conn.is_local_active_speaker);
    assert!(conn.local_tcp_port >= 1024);
    let qr = core.connect_get_qr_code(false);
    assert!(qr.uri.starts_with("ferric://pair?"));
    assert!(qr.size >= 21);
    assert_eq!(qr.modules.len(), (qr.size * qr.size) as usize);

    // M5: Storage stats & offline download APIs
    let st = core.get_storage_stats().expect("storage stats");
    assert_eq!(st.offline_track_count, 0);
    assert!(st.database_bytes > 0);
    assert_eq!(core.clear_audio_cache(), 0);
    assert_eq!(core.clear_artwork_cache(), 0);

    core.shutdown();
    let _ = std::fs::remove_dir_all(&root);
}
