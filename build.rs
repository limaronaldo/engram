fn main() {
    // Only compile the protobuf schema when the `grpc` feature is active.
    // This keeps default builds free of any proto toolchain dependency.
    #[cfg(feature = "grpc")]
    compile_protos();
}

#[cfg(feature = "grpc")]
fn compile_protos() {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        // Emit file descriptor set so gRPC reflection can be added later
        .file_descriptor_set_path(
            std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap())
                .join("mcp_descriptor.bin"),
        )
        .compile_protos(&["proto/mcp.proto"], &["proto/"])
        .expect("Failed to compile proto/mcp.proto");

    // Re-run if the proto file changes
    println!("cargo:rerun-if-changed=proto/mcp.proto");
    println!("cargo:rerun-if-changed=build.rs");
}
