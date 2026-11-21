fn main() {
    println!("cargo:rerun-if-changed=assets/branding/neolint.exe.ico");

    // Embed the application icon only in Windows executables. The Linux build
    // remains a normal console binary and does not gain platform UI resources.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        #[cfg(target_os = "windows")]
        {
            let mut resource = winres::WindowsResource::new();
            resource
                .set_icon("assets/branding/neolint.exe.ico")
                .set("FileDescription", "neolint - LSL Code Quality Linter")
                .set("ProductName", "neolint");
            resource
                .compile()
                .expect("failed to embed neolint Windows icon");
        }
    }
}
