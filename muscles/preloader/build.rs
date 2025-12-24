use std::env;
use std::fs;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/");

    // Check that we're building for UEFI (for production builds)
    let target = env::var("TARGET").unwrap();
    if target.contains("uefi") {
        // UEFI target - enable full checks
        println!("cargo:rustc-cfg=uefi_target");
        generate_size_checks();
    } else {
        // Non-UEFI target (testing) - skip UEFI-specific checks
        // This allows `cargo test --workspace` to work
        println!("cargo:warning=Pre-nucleus loader: building for testing (not UEFI target)");
    }
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
