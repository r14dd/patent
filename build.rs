use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");

    // ort-sys 2.0.0-rc.15's prebuilt onnxruntime calls
    // `___isPlatformVersionAtLeast` (from `@available` in its CoreML code). That
    // symbol lives in clang's compiler-rt, which rustc does not link on macOS,
    // so the final link fails without it.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = match Command::new("xcrun")
        .args(["clang", "--print-resource-dir"])
        .output()
    {
        Ok(out) if out.status.success() => out,
        _ => {
            println!(
                "cargo:warning=`xcrun clang --print-resource-dir` failed; not linking compiler-rt, \
                 so the link may fail on `___isPlatformVersionAtLeast`"
            );
            return;
        }
    };
    let dir = Path::new(String::from_utf8_lossy(&out.stdout).trim()).join("lib/darwin");
    let lib = dir.join("libclang_rt.osx.a");
    // Rerun when the toolchain moves: a CLT update removes the old clang dir.
    println!("cargo:rerun-if-changed={}", lib.display());
    if !lib.exists() {
        println!(
            "cargo:warning={} not found; not linking compiler-rt",
            lib.display()
        );
        return;
    }
    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}
