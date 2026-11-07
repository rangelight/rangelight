//! Generates the messages and Connect services of api/proto. Needs `buf` on PATH (the version
//! `scripts/check.sh` pins), which also reads the Buf module's imports.

use std::process::Command;

const FILES: [&str; 2] = [
    "rangelight/runner/v1alpha1/runner.proto",
    "rangelight/session/v1alpha1/session.proto",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo::rerun-if-changed=../../../api/proto");
    println!("cargo::rerun-if-changed=../../../api/buf.yaml");
    let descriptors = tempfile::NamedTempFile::new()?;
    let status = Command::new("buf")
        .args(["build", "../../../api", "--as-file-descriptor-set", "-o"])
        .arg(descriptors.path())
        .status()?;
    if !status.success() {
        return Err(format!("buf build failed: {status}").into());
    }
    connectrpc_build::Config::new()
        .descriptor_set(descriptors.path())
        .files(&FILES)
        .include_file("_connectrpc.rs")
        .compile()?;
    Ok(())
}
