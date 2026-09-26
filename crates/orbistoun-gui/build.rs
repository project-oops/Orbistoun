//! Embeds the project icon in the Windows executable's resource table.
//!
//! Explorer and the taskbar read the executable's resources; the title bar icon is set at
//! runtime by `main.rs` from `assets/logo.png`, so both are needed. `assets/logo.ico` holds
//! sizes 16 through 256 so Windows picks a size instead of downscaling the pixel-art mark.

fn main() {
    // The asset is the only input, so a change to it must trigger a relink.
    println!("cargo:rerun-if-changed=../../assets/logo.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/logo.ico");
        // Fail the build rather than ship a binary silently missing its icon.
        resource
            .compile()
            .expect("could not embed assets/logo.ico in the executable");
    }
}
