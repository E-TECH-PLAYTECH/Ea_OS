use std::env;
use std::fs;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/");

    // Check that we're building for UEFI
    let target = env::var("TARGET").unwrap();
    if !target.contains("uefi") {
        panic!("Pre-nucleus loader must be built for UEFI target");
    }

    // Generate size verification
    generate_size_checks();
}

fn generate_size_checks() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = std::path::Path::new(&out_dir).join("size_check.rs");

    let check_code = r#"
        // Compile-time size assertion for pre-nucleus loader
        const _: () = assert!(core::mem::size_of::<PreNucleusLoader>() <= 2048, 
                             "Pre-nucleus loader exceeds 2KiB size limit");
    "#;

    fs::write(&dest_path, check_code).unwrap();
    println!("cargo:rustc-cfg=size_checked");
}
