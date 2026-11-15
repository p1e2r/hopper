fn main() {
    // Release builds get their tag baked in for the updater (see src/update.rs).
    println!("cargo:rerun-if-env-changed=HOPPER_RELEASE_TAG");
    println!("cargo:rerun-if-changed=assets/icon/icon.ico");

    // Windows: embed the app icon (GPUI loads resource id 1 for the window icon)
    // and the file details shown in Explorer.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon_with_id("assets/icon/icon.ico", "1")
            .set("ProductName", "Hopper")
            .set("FileDescription", "Hopper - Minecraft mod manager")
            .set("LegalCopyright", "MIT License");
        if let Err(e) = res.compile() {
            panic!("couldn't embed the Windows icon: {e}");
        }
    }
}
