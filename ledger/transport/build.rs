fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/transport.proto");
    tonic_build::configure().compile(&["proto/transport.proto"], &["proto"])?;
    Ok(())
}
