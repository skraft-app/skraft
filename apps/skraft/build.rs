//! Build script: the executable's platform metadata.
//!
//! - macOS: embeds an Info.plist in the executable. An unbundled executable has no
//!   `CFBundleName`, so macOS titles the app menu of the menu bar with the executable's file
//!   name (`skraft`). `NSBundle` also reads an Info.plist from the `__TEXT,__info_plist`
//!   section of the binary, which gives the app its display name without a `.app` bundle (the
//!   bundle the installer ships has its own, `packaging/macos/Info.plist`).
//! - Windows: embeds the icon and the version resource the Explorer shows. Only when building
//!   on Windows (the resource compiler is the Windows SDK's): a cross build from elsewhere has
//!   no icon.
//! - Dev builds (the `dev` profile): what they were built from — the git worktree, branch,
//!   commit, whether changes not committed went in, when — for the title bar's badge and the
//!   registry of running apps (src/system/identity.rs). Built again whenever HEAD, the index or a
//!   source changes.

use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // The web build's stamp (scripts/build-web.sh), shown in « À propos ».
    println!("cargo:rerun-if-env-changed=SKRAFT_WEB_BUILD");
    if env::var("PROFILE").as_deref() == Ok("debug") {
        git_identity();
    }
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => macos_info_plist(),
        Ok("windows") => windows_resources(),
        _ => {}
    }
}

/// `git <args>` in the crate's folder, its output trimmed (none: no git, not a checkout).
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(env::var("CARGO_MANIFEST_DIR").ok()?).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn git_identity() {
    let Some(toplevel) = git(&["rev-parse", "--show-toplevel"]) else { return };
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    // Untracked files left out: scratch files are no change of the build.
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    let built = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    println!("cargo:rustc-env=SKRAFT_GIT_WORKTREE={toplevel}");
    println!("cargo:rustc-env=SKRAFT_GIT_BRANCH={branch}");
    println!("cargo:rustc-env=SKRAFT_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=SKRAFT_GIT_DIRTY={}", u8::from(dirty));
    println!("cargo:rustc-env=SKRAFT_BUILT={built}");
    // Run again on a commit, a checkout, a staging, or any source edited (dirty or not). Only
    // paths that exist: Cargo runs it at every build for one that does not.
    for path in ["HEAD", "index", "packed-refs", &format!("refs/heads/{branch}")] {
        if let Some(p) = git(&["rev-parse", "--path-format=absolute", "--git-path", path])
            && std::path::Path::new(&p).exists()
        {
            println!("cargo:rerun-if-changed={p}");
        }
    }
    for dir in ["apps", "crates", "Cargo.toml", "Cargo.lock"] {
        println!("cargo:rerun-if-changed={toplevel}/{dir}");
    }
}

fn macos_info_plist() {
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>app.skraft</string>
    <key>CFBundleName</key>
    <string>Skraft</string>
    <key>CFBundleDisplayName</key>
    <string>Skraft</string>
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

#[cfg(windows)]
fn windows_resources() {
    println!("cargo:rerun-if-changed=assets/app-icon.ico");
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/app-icon.ico").set("ProductName", "Skraft").set("FileDescription", "Skraft");
    res.compile().expect("compile the Windows resources");
}

#[cfg(not(windows))]
fn windows_resources() {
    println!("cargo:warning=cross build for Windows: the executable has no icon (build on Windows for it)");
}
