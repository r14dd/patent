fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // ort-sys 2.0.0-rc.15's prebuilt onnxruntime calls
    // `___isPlatformVersionAtLeast` (from `@available` in its CoreML code). That
    // symbol lives in clang's compiler-rt, which rustc does not link on macOS,
    // so the final link fails without it.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let Ok(out) = std::process::Command::new("xcrun")
        .args(["clang", "--print-resource-dir"])
        .output()
    else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let dir = String::from_utf8_lossy(&out.stdout);
    println!("cargo:rustc-link-search=native={}/lib/darwin", dir.trim());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}
