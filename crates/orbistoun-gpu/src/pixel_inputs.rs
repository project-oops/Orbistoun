//! A pixel shader's input registers, decoded from the stream for the translator.
//!
//! `SPI_PS_INPUT_ENA` and `SPI_PS_INPUT_ADDR` decide which vector registers a pixel wave starts
//! with; the translator seeds the system values among them (`orbistoun_translate::wavefront::
//! PixelInputs`).

use orbistoun_translate::wavefront::{PixelInputs, UserData};

use crate::registers::RegisterWrite;

/// `SPI_PS_INPUT_ENA`, a context register: `gfx103.json` maps it at byte `165580` (`0x286CC`), the
/// `SET_CONTEXT_REG` base `0xA000` plus `0x1B3`.
const SPI_PS_INPUT_ENA: u32 = 0xA1B3;

/// `SPI_PS_INPUT_ADDR`, the next register: byte `165584` (`0x286D0`), sharing `ENA`'s field layout.
const SPI_PS_INPUT_ADDR: u32 = 0xA1B4;

/// The pixel shader's input registers as the stream last wrote them, or `None` when it wrote no
/// `SPI_PS_INPUT_ENA`. An absent `ADDR` reads as zero, which the translator takes to mean `ENA`.
pub(crate) fn decode(writes: &[RegisterWrite]) -> Option<PixelInputs> {
    let last = |register: u32| {
        writes
            .iter()
            .rev()
            .find(|write| write.register == register)
            .map(|write| write.value)
    };
    Some(PixelInputs {
        enable: last(SPI_PS_INPUT_ENA)?,
        address: last(SPI_PS_INPUT_ADDR).unwrap_or(0),
    })
}

/// Distinguishes, in the cache key, modules whose stage starts in different register state: the
/// pixel input layout and the `DX10_CLAMP` mode are both in the module.
///
/// Held in bits 0-47, clear of the user-data count and first register in bits 48-63: `ENA` in the
/// low sixteen bits and `ADDR` from bit 32, both sixteen-bit fields (`gfx103.json`).
pub(crate) fn salt(user_data: UserData) -> u64 {
    let inputs = user_data.pixel_inputs.map_or(0, |inputs| {
        1 << 18 | u64::from(inputs.address & 0xffff) << 32 | u64::from(inputs.enable & 0xffff)
    });
    let clamp = match user_data.dx10_clamp {
        None => 0,
        Some(false) => 1 << 16,
        Some(true) => 2 << 16,
    };
    inputs | clamp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(register: u32, value: u32) -> RegisterWrite {
        RegisterWrite {
            packet_offset: 0,
            register,
            value,
        }
    }

    /// The last write of each register wins, and each lands in its own field.
    #[test]
    fn the_last_enable_and_address_writes_are_decoded() {
        let writes = [
            write(SPI_PS_INPUT_ENA, 0x2),
            write(SPI_PS_INPUT_ADDR, 0x2),
            write(SPI_PS_INPUT_ENA, 0x2302),
            write(SPI_PS_INPUT_ADDR, 0x2303),
        ];
        assert_eq!(
            decode(&writes),
            Some(PixelInputs {
                enable: 0x2302,
                address: 0x2303
            })
        );
    }

    /// A stream that writes no enable register has no layout; one that writes no address register
    /// leaves it zero for the translator to read as the enable.
    #[test]
    fn a_missing_enable_is_no_layout_and_a_missing_address_is_zero() {
        assert_eq!(decode(&[write(SPI_PS_INPUT_ADDR, 0x302)]), None);
        assert_eq!(
            decode(&[write(SPI_PS_INPUT_ENA, 0x302)]),
            Some(PixelInputs {
                enable: 0x302,
                address: 0
            })
        );
    }

    /// Every input layout and clamp mode that changes the module changes the key.
    #[test]
    fn the_salt_tells_apart_what_changes_the_module() {
        let with = |pixel_inputs, dx10_clamp| {
            salt(UserData {
                pixel_inputs,
                dx10_clamp,
                ..UserData::default()
            })
        };
        let layouts = [
            None,
            Some(PixelInputs {
                enable: 0,
                address: 0,
            }),
            Some(PixelInputs {
                enable: 0x302,
                address: 0,
            }),
            Some(PixelInputs {
                enable: 0x302,
                address: 0x303,
            }),
        ];
        let mut seen = std::collections::BTreeSet::new();
        for layout in layouts {
            for clamp in [None, Some(false), Some(true)] {
                assert!(
                    seen.insert(with(layout, clamp)),
                    "{layout:?} {clamp:?} collides"
                );
            }
        }
    }
}
