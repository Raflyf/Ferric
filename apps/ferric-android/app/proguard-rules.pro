# Keep JNA classes, native methods, and Struct field layouts required by UniFFI Kotlin bindings.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.Structure {
    public <fields>;
    public <init>();
    protected void write();
    protected void read();
}
-keepclassmembers class * implements com.sun.jna.Callback {
    *;
}
-dontwarn com.sun.jna.**

# Keep all UniFFI generated bindings, records, enums, exceptions, and callback bridges.
-keep class uniffi.ferric_mobile_ffi.** { *; }
-keepclassmembers class uniffi.ferric_mobile_ffi.** { *; }

# Keep JNI entry point class matching Rust's Java_dev_livelydesk_ferric_core_NativeBridge_initAndroidContext.
-keep class dev.livelydesk.ferric.core.NativeBridge {
    native <methods>;
}
