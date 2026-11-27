use ferric_mobile_ffi::{
    FerricMobileCore, FfiBackgroundMode, FfiContentOpener, FfiError, FfiEventSink, FfiSecretCipher,
    FfiTelemetry, FfiTrack,
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

    core.shutdown();
    let _ = std::fs::remove_dir_all(&root);
}
