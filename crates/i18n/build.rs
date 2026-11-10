//! rust-i18n reads the catalogs while compiling this crate, but Cargo does not know it: a
//! changed catalog must rebuild it.

fn main() {
    println!("cargo:rerun-if-changed=../../locales");
}
