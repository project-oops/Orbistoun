//! What the command-buffer side asks of the display: to queue a flip a builder encodes, and to be
//! told when the command processor releases a flip label (D728).
//!
//! This crate names no display of its own; the worker installs one. Until it does, a flip builder
//! writes nothing and answers `0`, as hardware does for a port that is not open.

use std::sync::OnceLock;

/// The display's half of a command-buffer flip.
#[derive(Debug, Clone, Copy)]
pub struct Display {
    /// Queues a flip of `(handle, buffer_index, flip_arg)`, answering the interrupt context id and
    /// the label its release carries, or `None` when the port or buffer is not there.
    pub queue_flip: fn(u64, u64, u64) -> Option<(u32, u64)>,
    /// Reports a release of `label` with interrupt context id `context`; answers whether it was a
    /// flip and was performed.
    pub released: fn(u64, u32) -> bool,
}

static DISPLAY: OnceLock<Display> = OnceLock::new();

/// Installs the display. First install wins.
pub fn install(display: Display) {
    let _ = DISPLAY.set(display);
}

/// The installed display's [`Display::queue_flip`], or `None` with none installed.
#[must_use]
pub fn queue_flip(handle: u64, buffer_index: u64, flip_arg: u64) -> Option<(u32, u64)> {
    DISPLAY
        .get()
        .and_then(|display| (display.queue_flip)(handle, buffer_index, flip_arg))
}

/// Hands a release to the installed display, if there is one.
pub fn released(label: u64, context: u32) -> bool {
    DISPLAY
        .get()
        .is_some_and(|display| (display.released)(label, context))
}

/// The dwords `sceAgcDcbSetFlip` writes for a flip of `buffer_index` with `flip_arg`, released to
/// `label` under interrupt `context` - measured whole (obSCEne `-1d54`, `166-agc/dcb-set-flip`,
/// sweep 20260927-013000 lines 12923-12975), with flip mode 1:
///
/// - `SET_UCONFIG_REG` `0x342..0x343`: `0xc7010101 + 8 * buffer_index`, then `0`;
/// - `WRITE_DATA` to register `0xc343`: the 64-bit flip argument, low half first;
/// - `RELEASE_MEM` of the 64-bit value `1` to the label, interrupt context id `context`;
/// - the header of a `NOP` whose 45-dword body the builder skips over unwritten, making 64 dwords.
#[must_use]
pub fn set_flip_words(buffer_index: u32, flip_arg: u64, context: u32, label: u64) -> [u32; 19] {
    [
        0xc002_7904,
        0x0000_0342,
        0xc701_0101_u32.wrapping_add(buffer_index.wrapping_mul(8)),
        0x0000_0000,
        0xc004_3704,
        0x0601_0000,
        0x0000_c343,
        0x0000_0000,
        flip_arg as u32,
        (flip_arg >> 32) as u32,
        0xc006_4900,
        0x0620_0504,
        0x4201_0000,
        label as u32,
        (label >> 32) as u32,
        0x0000_0001,
        0x0000_0000,
        context,
        0xc02c_1000,
    ]
}

/// How far `sceAgcDcbSetFlip` advances the cursor: 256 bytes, the written dwords and the skipped
/// `NOP` body together (`-1d54`: `bytes 0x100` on every written call).
pub const SET_FLIP_DWORDS: usize = 64;

#[cfg(test)]
mod tests {
    use super::set_flip_words;
    use std::fmt::Write as _;

    /// The words, as bytes, are the ones obSCEne dumped for buffer 1 on its first call there, and
    /// for buffer 0 on the first call of the run.
    #[test]
    fn the_flip_words_are_the_measured_packet() {
        let bytes = |words: &[u32]| -> String {
            words
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .fold(String::new(), |mut out, b| {
                    let _ = write!(out, "{b:02x}");
                    out
                })
        };
        assert_eq!(
            bytes(&set_flip_words(
                0,
                0x1122_3344_5566_7788,
                0x0800_0101,
                0xC_8000_40A0
            )),
            concat!(
                "047902c042030000010101c700000000",
                "043704c00000010643c3000000000000",
                "8877665544332211004906c004052006",
                "00000142a04000800c00000001000000",
                "000000000101000800102cc0",
            )
        );
        assert_eq!(
            bytes(&set_flip_words(
                1,
                0x1122_3344_5566_7788,
                0x0800_0103,
                0xC_8000_40A8
            )),
            concat!(
                "047902c042030000090101c700000000",
                "043704c00000010643c3000000000000",
                "8877665544332211004906c004052006",
                "00000142a84000800c00000001000000",
                "000000000301000800102cc0",
            )
        );
    }
}
