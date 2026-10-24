//! Whether a stream's draws are carried out as one segment holding them all.

use crate::packet;

/// Whether [`super::execute`] carries out `stream`'s draws as one whole segment, drawn from the
/// stream's own preparation (D729): known before it runs, from its packets alone, so a preparation
/// whose draws are each drawn from their own segment's preparation need not bind what they read.
/// `false` for a stream with no draws, or one whose walk cannot be trusted, which draws nothing.
#[must_use]
pub fn carried_out_whole(stream: &[u8]) -> bool {
    let walked = packet::walk(stream);
    walked.is_trustworthy()
        && matches!(
            super::draw_segments(stream, &walked.packets).as_slice(),
            [only] if only.whole
        )
}

#[cfg(test)]
mod tests {
    use super::carried_out_whole;
    use crate::packet::build::{command_header, dispatch_direct, measured};

    fn draw() -> [u32; 3] {
        [command_header(measured::DRAW_INDEX_AUTO, 2), 3, 2]
    }

    /// A 32-bit end-of-pipe release of `value` to `address`: memory work between draws.
    fn release(address: u64, value: u32) -> [u32; 8] {
        [
            command_header(measured::RELEASE_MEM, 7),
            0x0660_3514,
            1 << 29,
            u32::try_from(address & 0xFFFF_FFFF).expect("low half"),
            u32::try_from(address >> 32).expect("high half"),
            value,
            0,
            0,
        ]
    }

    fn bytes(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// The same answer the command processor gives as it carries the stream out: draws with only
    /// register state between them are whole; memory work between them, a dispatch beside them,
    /// or no draws at all are not.
    #[test]
    fn a_stream_is_whole_only_where_one_segment_holds_every_draw() {
        let mut registers = draw().to_vec();
        registers.extend([command_header(measured::SET_SH_REG, 2), 0x8c, 7]);
        registers.extend(draw());
        registers.extend(release(0x1000, 1));
        assert!(carried_out_whole(&bytes(&registers)));

        let mut memory_work = draw().to_vec();
        memory_work.extend(release(0x1000, 1));
        memory_work.extend(draw());
        assert!(!carried_out_whole(&bytes(&memory_work)));

        let mut dispatch = dispatch_direct(0xc0, 1, 1).to_vec();
        dispatch.extend(draw());
        assert!(!carried_out_whole(&bytes(&dispatch)));

        assert!(!carried_out_whole(&bytes(&release(0x1000, 1))));
    }
}
