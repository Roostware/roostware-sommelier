fn main() {
    // On Windows (MSVC), a test binary that links Tauri needs the Common-Controls v6
    // manifest or it dies at startup with STATUS_ENTRYPOINT_NOT_FOUND. The real app
    // gets its manifest from tauri-build; test binaries don't, so embed one for them.
    // (Scoped to tests so it can't conflict with tauri-build's manifest.)
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='win32' \
             name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }

    tauri_build::build()
}
