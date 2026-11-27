//! Android JNI bootstrap that registers `JavaVM` + global `Application` `Context` with `ndk_context`
//! once before any Android audio (`oboe`) or NDK API runs.

use jni::JNIEnv;
use jni::objects::{JClass, JObject};
use std::sync::Once;

static NDK_CONTEXT_INIT: Once = Once::new();

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
    NDK_CONTEXT_INIT.call_once(|| {
        let Ok(vm) = env.get_java_vm() else {
            return;
        };
        let Ok(global_ctx) = env.new_global_ref(app_context) else {
            return;
        };
        let vm_ptr = vm.get_java_vm_pointer().cast();
        let ctx_ptr = global_ctx.as_raw().cast();
        // SAFETY: `vm_ptr` is the process-lifetime `JavaVM*` from ART, `ctx_ptr` is a newly created
        // JNI `GlobalRef` to Android's process-lifetime `Application` Context, intentionally leaked
        // via `mem::forget` so `ndk_context` holds a valid non-dangling reference for process life,
        // and `NDK_CONTEXT_INIT` guarantees `initialize_android_context` is called at most once.
        unsafe {
            ndk_context::initialize_android_context(vm_ptr, ctx_ptr);
        }
        std::mem::forget(global_ctx);
    });
}
