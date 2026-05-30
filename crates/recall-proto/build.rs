use protoc_bin_vendored::protoc_bin_path;
use std::env;
use std::path::PathBuf;

fn main() {
    // Use vendored protoc binary — no system installation required.
    env::set_var("PROTOC", protoc_bin_path().unwrap());

    let spec_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("spec");

    let proto_files = [
        spec_dir.join("common.proto"),
        spec_dir.join("passport/passport-v1.proto"),
        spec_dir.join("receipt/receipt-v1.proto"),
        spec_dir.join("capability/capability-v1.proto"),
        spec_dir.join("memory/memory-v1.proto"),
        spec_dir.join("registry/registry-v1.proto"),
        spec_dir.join("control-plane/v1.proto"),
    ];

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile(&proto_files, &[&spec_dir])
        .expect("failed to compile proto files");

    // Re-run if any proto changes.
    println!("cargo:rerun-if-changed={}", spec_dir.display());
}
