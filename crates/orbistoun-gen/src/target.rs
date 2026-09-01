//! The GPU generation this project targets, in one place.
//!
//! Every generator that invokes the reference assembler reads the target from here. It
//! used to be a constant in each of them, which is fine right up to the day the target
//! changes - and then it is four edits, of which three get made.
//!
//! That day arrived: see D139. The generation was `gfx900` for months because nobody had
//! checked it, and the check when it came measured 52% of encodings differing from the
//! right one. A retarget has to be one edit, or the next wrong target lasts as long.

/// The architecture revision, as the reference toolchain names it.
///
/// RDNA2. The target console's GPU is an RDNA2 derivative, and this is the revision the
/// published RDNA2 instruction-set reference describes - so an encoding this assembles is
/// one a person can look up. It is *a* member of the generation rather than the exact part
/// in the console, which nobody outside the vendor can name and which would not help: what
/// is being derived here is the generation's encoding scheme.
pub(crate) const MCPU: &str = "gfx1030";

/// Architecture features the target is assembled with.
///
/// **Sixty-four-lane wavefronts.** This generation supports both 32- and 64-lane
/// wavefronts and the reference toolchain defaults to 32, which is why a first retarget
/// reported 69 rejected probes: `vcc` and `s[4:5]` are 64-lane spellings and the assembler
/// was in the other mode. Only two of those rejections were real.
///
/// Chosen rather than defaulted, for a reason that costs nothing either way: the width
/// **does not change the encodings**. `v_cndmask_b32_e64 v0, v1, v2, s[4:5]` and its
/// 32-lane spelling produce the same bytes with the same field holding the same 4; what
/// differs is whether the mask that field names is 32 or 64 bits wide. So the width is a
/// property of the *shader* - selected per wave, in the shader's own metadata - and not of
/// the tables. Generating in either mode yields the same table.
///
/// 64 then, because it is the mode the translator already models, and because the
/// previous-generation console has no other. Supporting 32-lane shaders is a translator
/// change when a real one turns up, not a regeneration.
pub(crate) const MATTR: &str = "+wavefrontsize64";

/// The target triple. Compute shaders assemble against the HSA runtime.
pub(crate) const TRIPLE: &str = "amdgcn-amd-amdhsa";

/// The triple graphics shaders need instead.
///
/// HSA refuses a graphics stage outright, and surfaced it as a crash rather than a
/// diagnostic - which cost a run to work out.
pub(crate) const GRAPHICS_TRIPLE: &str = "amdgcn-mesa-mesa3d";

/// One field of the target, by the name a caller would type.
///
/// Exists so a shell script can read the same source the generators do
/// (`orbistoun-gen target mcpu`), which is what `tools/shader-fixtures/probes/run.sh`
/// needs. A script that hardcodes the target is the four-edits-three-made problem
/// returning by another route.
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
    /// The usage message and the match arms are two lists, and a field named in one and
    /// missing from the other is a script that silently gets an empty string.
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
