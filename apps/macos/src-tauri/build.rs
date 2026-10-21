fn main() {
    // The app support matrix is a build input: `app_matrix.rs` includes the
    // generated file so the overlay allowlist and the tracked matrix cannot
    // drift apart. Re-run the build when the JSON changes.
    println!("cargo:rerun-if-changed=src/app_matrix.json");
    tauri_build::build()
}
