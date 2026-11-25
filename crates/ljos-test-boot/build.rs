/// Writes one `cargo:` directive line on stdout, where cargo reads it.
#[allow(
    clippy::disallowed_macros,
    reason = "cargo reads build script directives from stdout"
)]
fn directive(line: &str) {
    println!("cargo:{line}");
}

fn compile(out_dir: &str, src: &str, obj_name: &str, lib_name: &str) {
    let obj = std::path::Path::new(out_dir).join(obj_name);
    let lib = std::path::Path::new(out_dir).join(lib_name);
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let cc = std::process::Command::new("cc")
        .args(["-fPIC", "-O2", "-Wall", "-Wextra", "-c", src, "-o"])
        .arg(&obj)
        .status()
        .unwrap_or_else(|_| panic!("cc is required to build {src}"));
    if !cc.success() {
        panic!("cc failed to compile {src}");
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let ar = std::process::Command::new("ar")
        .args(["crs"])
        .arg(&lib)
        .arg(&obj)
        .status()
        .expect("ar is required to archive the test object");
    if !ar.success() {
        panic!("ar failed to archive {src}");
    }
}

fn main() {
    directive("rerun-if-changed=build.rs");
    directive("rerun-if-changed=src/move.c");
    directive("rerun-if-changed=src/canon.c");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    compile(&out_dir, "src/canon.c", "canon.o", "libljos_test_canon.a");
    directive(&format!("rustc-link-search=native={out_dir}"));
    directive("rustc-link-lib=static=ljos_test_canon");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    compile(&out_dir, "src/move.c", "move.o", "libljos_test_move.a");
    // Propagated to crates that depend on this one. Those are test
    // binaries. The ljos package does not depend on this crate.
    directive("rustc-link-lib=static=ljos_test_move");
}
