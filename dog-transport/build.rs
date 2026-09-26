fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "grpc")]
    {
        let protoc = protoc_bin_vendored::protoc_bin_path()?;
        std::env::set_var("PROTOC", protoc);
        tonic_prost_build::configure()
            .file_descriptor_set_path(
                std::path::PathBuf::from(std::env::var("OUT_DIR")?).join("dog_descriptor.bin"),
            )
            .compile_protos(&["proto/dog.proto"], &["proto"])?;
        println!("cargo:rerun-if-changed=proto/dog.proto");
    }
    Ok(())
}
