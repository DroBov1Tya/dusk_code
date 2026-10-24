fn main() {
    println!("cargo:rerun-if-changed=src/state/migrations");
}
