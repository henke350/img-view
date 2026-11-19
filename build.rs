// Build script: embeds the eye icon in the .exe, so Explorer, shortcuts and the
// taskbar show it. The icon is drawn by tools/make_icon.py.

fn main() {
    println!("cargo:rerun-if-changed=assets/eye.ico");

    // Only Windows executables carry resources; skip this when building for anything else.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/eye.ico")
            .compile()
            .expect("failed to embed the app icon (is the Windows SDK installed?)");
    }
}
