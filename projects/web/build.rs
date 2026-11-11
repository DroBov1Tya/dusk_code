//! The bundle is embedded from `web-ui/dist`, which trunk fills. A checkout that has not
//! built it still compiles: the folder is created empty and the server says so.

fn main() {
    let dist = std::path::Path::new("../web-ui/dist");
    if let Err(error) = std::fs::create_dir_all(dist) {
        println!("cargo:warning=cannot create {}: {error}", dist.display());
    }
    println!("cargo:rerun-if-changed=../web-ui/dist");
}
