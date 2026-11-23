fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let source = "src/capture_macos_native.m";
        println!("cargo:rerun-if-changed={source}");
        cc::Build::new()
            .file(source)
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("moyu_capture");
        for framework in [
            "AppKit",
            "ScreenCaptureKit",
            "CoreGraphics",
            "CoreMedia",
            "CoreVideo",
            "CoreImage",
            "Foundation",
            "ImageIO",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
    tauri_build::build()
}
