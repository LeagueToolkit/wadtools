//! Build script: embeds the application icon into the Windows executable so the binary
//! (and the Explorer context-menu entries that reference it) carry wadtools branding.
//!
//! The icon is optional — if `assets/wadtools.ico` is not present the embed is skipped so
//! source builds keep working before the asset is added. Non-Windows targets do nothing.

use std::path::Path;

fn main() {
    let icon = "assets/wadtools.ico";
    println!("cargo:rerun-if-changed={icon}");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS");
    if target_os.as_deref() == Ok("windows") && Path::new(icon).exists() {
        #[cfg(windows)]
        {
            let mut res = winresource::WindowsResource::new();
            res.set_icon(icon);
            if let Err(e) = res.compile() {
                println!("cargo:warning=failed to embed Windows icon: {e}");
            }
        }
    }
}
