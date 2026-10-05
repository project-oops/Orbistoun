//! The GPU generation this project targets, in one place (D139).
//!
//! Every generator that invokes the reference assembler reads the target from here, so a
//! retarget is one edit.

/// The architecture revision, as the reference toolchain names it.
///
/// The console's own part (D739): oops-mesa's device information reports `GFX1013` inside the
/// Navi family, and radeonsi's compiler emits that revision's instructions - `v_mac_f32` among
/// them, which `gfx1030` removed and the reference assembler then refuses. Every name and layout
/// the two revisions share records the same; the published RDNA2 reference, which describes
/// `gfx1030`, stays the document an encoding is looked up in.
pub(crate) const MCPU: &str = "gfx1013";

/// Architecture features the target is assembled with.
///
/// Sixty-four-lane wavefronts. The toolchain defaults to 32, in which 64-lane spellings
/// such as `vcc` and `s[4:5]` are rejected. The width does not change the encodings, only
/// how wide the named mask is, so it is a property of the shader rather than the tables.
/// 64 is the mode the translator models and the only one the previous generation has.
pub(crate) const MATTR: &str = "+wavefrontsize64";

/// The target triple. Compute shaders assemble against the HSA runtime.
pub(crate) const TRIPLE: &str = "amdgcn-amd-amdhsa";

/// The triple graphics shaders need instead.
///
/// HSA refuses a graphics stage, and the assembler reports that as a crash rather than a
/// diagnostic.
pub(crate) const GRAPHICS_TRIPLE: &str = "amdgcn-mesa-mesa3d";

/// One field of the target, by the name a caller would type.
///
/// So a shell script such as `tools/shader-fixtures/probes/run.sh` reads the same target
/// the generators do (`orbistoun-gen target mcpu`) instead of hardcoding it.
#[must_use]
pub(crate) fn field(name: &str) -> Option<&'static str> {
    match name {
        "mcpu" => Some(MCPU),
        "mattr" => Some(MATTR),
        "triple" => Some(TRIPLE),
        "graphics-triple" => Some(GRAPHICS_TRIPLE),
        _ => None,
    }
}

/// Every field name `field` accepts, for a usage message.
pub(crate) const FIELDS: [&str; 4] = ["mcpu", "mattr", "triple", "graphics-triple"];

#[cfg(test)]
mod tests {
    use super::{FIELDS, field};

    /// Every advertised field resolves.
    ///
    /// The usage list and the match arms agree.
    #[test]
    fn every_advertised_field_resolves() {
        for name in FIELDS {
            assert!(
                field(name).is_some(),
                "{name} is advertised but unresolvable"
            );
        }
    }

    /// An unknown field is refused rather than answered with a plausible default.
    #[test]
    fn an_unknown_field_is_refused() {
        assert!(field("mcpuu").is_none());
        assert!(field("").is_none());
    }
}
