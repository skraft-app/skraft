//! Build script: on macOS, embeds an Info.plist in the executable.
//!
//! An unbundled executable has no `CFBundleName`, so macOS titles the app menu
//! of the menu bar with the executable's file name (`solidsketch`). `NSBundle`
//! also reads an Info.plist from the `__TEXT,__info_plist` section of the
//! binary, which gives the app its display name without a `.app` bundle.

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>fr.lairdubois.solidsketch</string>
    <key>CFBundleName</key>
    <string>SolidSketch</string>
    <key>CFBundleDisplayName</key>
    <string>SolidSketch</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
"#
    );
    let path = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("Info.plist");
    fs::write(&path, plist).expect("write Info.plist");
    println!("cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}", path.display());
}
