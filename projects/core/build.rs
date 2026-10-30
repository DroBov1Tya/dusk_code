fn main() {
    println!("cargo:rerun-if-changed=src/state/migrations");
    // The update picks the release build made for the same target.
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=DUSK_TARGET={target}");
}
