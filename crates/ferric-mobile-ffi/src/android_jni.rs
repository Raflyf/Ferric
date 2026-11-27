//! Android JNI bootstrap that registers `JavaVM` + global `Application` `Context` with `ndk_context`
//! once before any Android audio (`oboe`) or NDK API runs.

use jni::JNIEnv;
use jni::objects::{JClass, JObject};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static NDK_CONTEXT_READY: AtomicBool = AtomicBool::new(false);
static NDK_CONTEXT_INIT_LOCK: Mutex<()> = Mutex::new(());

/// Called from `dev.livelydesk.ferric.core.NativeBridge.initAndroidContext(appContext: Context)`
/// during `FerricApp.onCreate()`.
///
/// # Safety
/// - Must be invoked by the Android ART JVM with valid `JNIEnv` and a non-null `Application` `Context`.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_livelydesk_ferric_core_NativeBridge_initAndroidContext(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    app_context: JObject<'_>,
) {
    if app_context.is_null() || NDK_CONTEXT_READY.load(Ordering::Acquire) {
        return;
    }
    let _guard = NDK_CONTEXT_INIT_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if NDK_CONTEXT_READY.load(Ordering::Acquire) {
        return;
    }
    let Ok(vm) = env.get_java_vm() else {
        return;
    };
    let Ok(global_ctx) = env.new_global_ref(app_context) else {
        return;
    };
    let vm_ptr: *mut std::ffi::c_void = vm.get_java_vm_pointer().cast();
    let ctx_ptr: *mut std::ffi::c_void = global_ctx.as_raw().cast();
    if vm_ptr.is_null() || ctx_ptr.is_null() {
        return;
    }
    // SAFETY: `vm_ptr` is the non-null process-lifetime `JavaVM*` from ART, `ctx_ptr` is a newly
    // created JNI `GlobalRef` to Android's process-lifetime `Application` Context, intentionally
    // leaked via `mem::forget` so `ndk_context` holds a valid non-dangling reference for process
    // life, and `NDK_CONTEXT_INIT_LOCK` + `NDK_CONTEXT_READY` guarantee single initialization.
    unsafe {
        ndk_context::initialize_android_context(vm_ptr, ctx_ptr);
    }
    std::mem::forget(global_ctx);
    NDK_CONTEXT_READY.store(true, Ordering::Release);
}
