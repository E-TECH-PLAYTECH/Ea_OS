// referee/build.rs
// Eä Referee Build Script v5.0

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/");

    // Set UEFI target configuration
    println!("cargo:rustc-cfg=uefi");
}
