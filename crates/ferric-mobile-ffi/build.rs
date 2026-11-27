fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        // oboe-sys compiles C++ code against libc++_static, which depends on libc++abi
        // for __cxa_pure_virtual, operator new/delete, guards, and RTTI base symbols.
        // Pass both archives directly to the NDK clang linker driver so it resolves them
        // from the active target sysroot, and enforce --no-undefined to catch any
        // missing symbol at link time instead of Android dlopen time.
        println!("cargo:rustc-cdylib-link-arg=-Wl,--start-group");
        println!("cargo:rustc-cdylib-link-arg=-lc++_static");
        println!("cargo:rustc-cdylib-link-arg=-lc++abi");
        println!("cargo:rustc-cdylib-link-arg=-Wl,--end-group");
        println!("cargo:rustc-cdylib-link-arg=-Wl,--no-undefined");
    }
}
