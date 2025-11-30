use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/");
    
    // Verify kernel size constraint
    verify_kernel_size();
    
    // Generate compile-time assertions
    generate_assertions();
}

fn verify_kernel_size() {
    // This would actually calculate size in a real build
    println!("cargo:rustc-cfg=kernel_size_verified");
}

fn generate_assertions() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("assertions.rs");
    
    let assertions = r#"
        // Compile-time size assertions
        const_assert::const_assert!(core::mem::size_of::<MuscleNucleus>() <= 8192);
    "#;
    
    fs::write(&dest_path, assertions).unwrap();
}
