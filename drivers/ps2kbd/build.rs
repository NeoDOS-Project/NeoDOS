fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:warning=ps2kbd.nem: layout generation removed (NeoKBD handles layouts)");
}
