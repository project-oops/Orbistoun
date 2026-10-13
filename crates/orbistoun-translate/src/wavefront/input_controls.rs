//! `SPI_PS_INPUT_CNTL_0` .. `_31`: where each attribute a pixel shader interpolates comes from.
//!
//! Attribute `n` reads the vertex output `OFFSET` (bits 5:0) names, or, with `OFFSET` `0x20`, a
//! constant `DEFAULT_VAL` (bits 9:8) picks; `FLAT_SHADE` (bit 10) takes the provoking vertex's
//! value rather than interpolating (`gfx10.json` and `gfx103.json`, `SPI_PS_INPUT_CNTL_0`). The
//! driver writes them when it links the stages: radeonsi in `si_emit_spi_map`, from the vertex
//! shader's parameter offsets (`si_shader.c:1530-1565`). An attribute whose register the stream
//! never wrote reads the output of its own number.

use crate::TranslateError;

/// How many attributes a pixel shader can interpolate: one control register each.
pub const INPUT_CONTROLS: usize = 32;

/// `OFFSET`, bits 5:0: the vertex output an attribute reads, below [`DEFAULT_OFFSET`].
const OFFSET_MASK: u32 = 0x3f;

/// The `OFFSET` that reads the constant `DEFAULT_VAL` names instead of a vertex output (Mesa's
/// "OFFSET=0x20 means that DEFAULT_VAL is used", `si_shader.c:1553`).
const DEFAULT_OFFSET: u32 = 0x20;

/// `DEFAULT_VAL`, bits 9:8.
const DEFAULT_VALUE_SHIFT: u32 = 8;

/// `FLAT_SHADE`, bit 10.
const FLAT_SHADE: u32 = 1 << 10;

/// The constants `DEFAULT_VAL` 0, 1 and 2 name, in the order Mesa's `AC_EXP_PARAM_DEFAULT_VAL_*`
/// lists them and both radeonsi and radv subtract from (`ac_nir.h:28-31`,
/// `radv_cmd_buffer.c:3666`): `0000`, `0001`, `1110`. The fourth, `1111` by that list, radeonsi's
/// own `SI_PS_INPUT_CNTL_0001` writes for `0001` (`si_shader.h:138`), so which it is is not
/// established and it is refused.
const DEFAULT_VALUES: [[f32; 4]; 3] = [
    [0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
    [1.0, 1.0, 1.0, 0.0],
];

/// The control registers the stream wrote, by attribute; `None` for one it never wrote.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InputControls(pub [Option<u32>; INPUT_CONTROLS]);

/// Where one attribute's value comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Source {
    /// The vertex output at this location.
    Output(u32),
    /// A constant, the same everywhere.
    Constant([f32; 4]),
}

impl InputControls {
    /// Where `attribute` reads from: its register's `OFFSET`, or its own number where the stream
    /// wrote no register for it.
    ///
    /// # Errors
    ///
    /// Refused by name: an `OFFSET` past [`DEFAULT_OFFSET`], which radv writes for an explicitly
    /// interpolated input read straight from the parameter cache (`radv_cmd_buffer.c:3680`), and the
    /// fourth `DEFAULT_VAL`, whose constant Mesa's two sources disagree on.
    pub(super) fn source(&self, attribute: u32) -> Result<Source, TranslateError> {
        let refuse = |detail| TranslateError::Unsupported { offset: 0, detail };
        let Some(control) = self.control(attribute) else {
            return Ok(Source::Output(attribute));
        };
        match control & OFFSET_MASK {
            offset if offset < DEFAULT_OFFSET => Ok(Source::Output(offset)),
            DEFAULT_OFFSET => {
                let which = (control >> DEFAULT_VALUE_SHIFT) & 0x3;
                DEFAULT_VALUES
                    .get(which as usize)
                    .map(|value| Source::Constant(*value))
                    .ok_or_else(|| {
                        refuse(concat!(
                            "a pixel input whose DEFAULT_VAL is 3, which Mesa's parameter list ",
                            "names 1111 and radeonsi's own constant writes for 0001"
                        ))
                    })
            }
            _ => Err(refuse(concat!(
                "a pixel input read straight from the parameter cache (OFFSET past 0x20), as ",
                "explicit interpolation reads it, which is not translated"
            ))),
        }
    }

    /// Whether `attribute` takes the provoking vertex's value rather than interpolating.
    pub(super) fn flat(&self, attribute: u32) -> bool {
        self.control(attribute)
            .is_some_and(|control| control & FLAT_SHADE != 0)
    }

    /// Whether every attribute reads the output of its own number, which is what a module
    /// translated with no controls assumes.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        (0..INPUT_CONTROLS as u32).all(|attribute| {
            !self.flat(attribute)
                && matches!(self.source(attribute), Ok(Source::Output(at)) if at == attribute)
        })
    }

    fn control(&self, attribute: u32) -> Option<u32> {
        self.0.get(attribute as usize).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::{InputControls, Source};

    fn with(attribute: usize, control: u32) -> InputControls {
        let mut controls = InputControls::default();
        controls.0[attribute] = Some(control);
        controls
    }

    /// An attribute reads the output its `OFFSET` names, and one never written reads its own.
    #[test]
    fn an_attribute_reads_the_output_its_offset_names() {
        let controls = with(1, 5);
        assert_eq!(controls.source(1).ok(), Some(Source::Output(5)));
        assert_eq!(controls.source(2).ok(), Some(Source::Output(2)));
        assert!(!controls.is_identity());
        assert!(with(3, 3).is_identity(), "its own number is no remap");
    }

    /// `OFFSET` `0x20` reads a constant: `DEFAULT_VAL` 0 to 2 are `0000`, `0001` and `1110`; the
    /// fourth and every pass-through offset are refused.
    #[test]
    fn a_default_offset_reads_the_constant_its_value_names() {
        assert_eq!(
            with(0, 0x20).source(0).ok(),
            Some(Source::Constant([0.0; 4]))
        );
        assert_eq!(
            with(0, 0x120).source(0).ok(),
            Some(Source::Constant([0.0, 0.0, 0.0, 1.0]))
        );
        assert_eq!(
            with(0, 0x220).source(0).ok(),
            Some(Source::Constant([1.0, 1.0, 1.0, 0.0]))
        );
        assert!(with(0, 0x320).source(0).is_err());
        assert!(with(0, 0x21).source(0).is_err());
    }

    /// `FLAT_SHADE` is read per attribute, as MDEM00001's stream writes it (`0x400`, output 0).
    #[test]
    fn flat_shade_is_read_per_attribute() {
        let controls = with(0, 0x400);
        assert!(controls.flat(0));
        assert!(!controls.flat(1));
        assert_eq!(controls.source(0).ok(), Some(Source::Output(0)));
        assert!(!controls.is_identity());
    }
}
