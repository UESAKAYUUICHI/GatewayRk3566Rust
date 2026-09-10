fn main() {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    println!("cargo:rustc-env=GW_BUILD_EPOCH_MS={now_ms}");
    println!("cargo:rerun-if-changed=build.rs");
}
