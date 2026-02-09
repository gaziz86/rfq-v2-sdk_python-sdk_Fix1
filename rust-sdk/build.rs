fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Use vendored protoc to avoid building C++ protobuf via autotools
    let protoc_path = protoc_bin_vendored::protoc_bin_path()?;
    unsafe {
        std::env::set_var("PROTOC", protoc_path);
    }

    // Emit file descriptor set for gRPC server reflection support
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    tonic_prost_build::configure()
        .file_descriptor_set_path(out_dir.join("market_maker_descriptor.bin"))
        .compile_protos(&["../protos/market_maker.proto"], &["../protos"])?;

    Ok(())
}
