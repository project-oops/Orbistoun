//! Embeds the project icon in the Windows executable.
//!
//! Only a Windows executable can carry an icon; Linux and macOS keep theirs in packaging metadata,
//! so the dependency is gated to Windows in `Cargo.toml`. `assets/logo.ico` holds six sizes (16 to
//! 256) so Windows picks one instead of downscaling the pixel-art mark.

fn main() {
    // The icon is the only input; without this a changed logo is never re-embedded.
    println!("cargo:rerun-if-changed=../../assets/logo.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/logo.ico");
        // A missing icon fails the build rather than producing a binary without it.
        resource
            .compile()
            .expect("could not embed assets/logo.ico in the executable");
    }
}
