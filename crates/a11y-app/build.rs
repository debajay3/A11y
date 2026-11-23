//! Embeds the app icon and version info into `a11y.exe` (Windows targets only).
//! On every other target this does nothing, so Linux and macOS builds are unaffected.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/a11y/A11y.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // The UI thread walks the layer tree recursively (render, layout, import). Windows reserves
    // 1 MiB for the main thread, which deeply nested imported files overflow; Linux and macOS
    // give it 8 MiB.
    match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => println!("cargo:rustc-link-arg-bin=a11y=/STACK:16777216"),
        Ok("gnu") => println!("cargo:rustc-link-arg-bin=a11y=-Wl,--stack,16777216"),
        _ => {}
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/a11y/A11y.ico");
    res.set("ProductName", "A11y");
    res.set("FileDescription", "A11y design editor");
    res.set("CompanyName", "a11y contributors");
    res.set("LegalCopyright", "MIT OR Apache-2.0");
    res.set("OriginalFilename", "A11y.exe");
    res.set("ProductVersion", &version);
    res.set("FileVersion", &version);
    if let Err(e) = res.compile() {
        // CI (and releases) must never ship an icon-less exe; local builds without a Windows SDK
        // just get a warning.
        if std::env::var_os("CI").is_some() {
            panic!("could not embed the Windows icon and version info: {e}");
        }
        println!("cargo:warning=could not embed the Windows icon and version info: {e}");
    }
}
