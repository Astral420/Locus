fn main() {
    // ScreenCaptureKit's Swift bridge links against @rpath/libswiftCore.dylib.
    // The Swift runtime ships in the OS (dyld cache) at /usr/lib/swift.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
    tauri_build::build();
}
