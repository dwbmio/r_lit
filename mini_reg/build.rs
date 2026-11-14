fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ENV");
    println!("cargo:rerun-if-env-changed=PROFILE");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").ok();
    let profile = std::env::var("PROFILE").ok();
    if msvc.as_deref() == Some("msvc") && profile.as_deref() == Some("release") {
        // Match the stripped release profile. Do not request unavailable SDK PDBs.
        println!("cargo:rustc-link-arg-bin=mini_reg=/DEBUG:NONE");
    }
}
