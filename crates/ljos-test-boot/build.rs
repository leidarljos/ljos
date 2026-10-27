fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/move.c");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let obj = std::path::Path::new(&out_dir).join("move.o");
    let lib = std::path::Path::new(&out_dir).join("libljos_test_move.a");
    let cc = std::process::Command::new("cc")
        .args(["-fPIC", "-O2", "-Wall", "-Wextra", "-c", "src/move.c", "-o"])
        .arg(&obj)
        .status()
        .expect("cc is required to build the test cgroup mover");
    if !cc.success() {
        panic!("cc failed to compile src/move.c");
    }
    let ar = std::process::Command::new("ar")
        .args(["crs"])
        .arg(&lib)
        .arg(&obj)
        .status()
        .expect("ar is required to archive the test cgroup mover");
    if !ar.success() {
        panic!("ar failed to archive the test cgroup mover");
    }
    // Propagated to crates that depend on this one. Those are test
    // binaries. The ljos package does not depend on this crate.
    println!("cargo:rustc-link-search=native={out_dir}");
    println!("cargo:rustc-link-lib=static=ljos_test_move");
}
