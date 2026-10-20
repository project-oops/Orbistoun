//! Splitting an Annex-B byte stream into access units.
//!
//! A decoder is handed one access unit - one coded picture with the parameter sets and SEI before
//! it - at a time, each with its own timestamp. The stream format is ITU-T H.264 Annex B (and the
//! same Annex B of H.265): every NAL unit follows a `00 00 01` start code, which may carry extra
//! leading zero bytes.
//!
//! Where one access unit ends is H.264 7.4.1.2.3 and H.265 7.4.2.4.4: the first of an access unit
//! delimiter, a parameter set, an SEI or another reserved non-picture NAL unit after a picture's
//! slices, or the first slice of the next picture. The first slice is the one whose
//! `first_mb_in_slice` is 0 (H.264 7.4.3; its `ue(v)` code for 0 is a single 1 bit) or whose
//! `first_slice_segment_in_pic_flag` is set (H.265 7.4.7.1). Streams with arbitrary slice order
//! are not distinguished by that rule, and nothing here handles them.

/// The coding standard a stream follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    /// ITU-T H.264 / MPEG-4 AVC.
    H264,
    /// ITU-T H.265 / HEVC.
    Hevc,
}

/// What a NAL unit is, as far as access-unit boundaries are concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// The first slice of a picture.
    FirstSlice,
    /// Any other slice of a picture.
    Slice,
    /// A NAL unit that may only come before a picture's first slice, so after a slice it starts
    /// the next access unit.
    Leading,
    /// Anything else, which belongs to the access unit it is in.
    Other,
}

fn kind(codec: Codec, nal: &[u8]) -> Kind {
    let Some(&header) = nal.first() else {
        return Kind::Other;
    };
    match codec {
        Codec::H264 => match header & 0x1f {
            // Slice of a non-IDR or IDR picture: Table 7-1.
            1 | 5 => match nal.get(1) {
                Some(byte) if byte & 0x80 != 0 => Kind::FirstSlice,
                _ => Kind::Slice,
            },
            // SEI, SPS, PPS, access unit delimiter, and types 14..=18 (7.4.1.2.3).
            6..=9 | 14..=18 => Kind::Leading,
            _ => Kind::Other,
        },
        Codec::Hevc => match (header >> 1) & 0x3f {
            // VCL NAL unit types 0..=31: Table 7-1.
            0..=31 => match nal.get(2) {
                Some(byte) if byte & 0x80 != 0 => Kind::FirstSlice,
                _ => Kind::Slice,
            },
            // VPS, SPS, PPS, access unit delimiter, prefix SEI, and 41..=44 and 48..=55
            // (7.4.2.4.4).
            32..=35 | 39 | 41..=44 | 48..=55 => Kind::Leading,
            _ => Kind::Other,
        },
    }
}

/// Every NAL unit of `stream`, as `(start, payload)`: where its start code's first zero byte is,
/// and the unit itself after the start code. Bytes before the first start code are skipped.
fn nal_units(stream: &[u8]) -> Vec<(usize, &[u8])> {
    let mut codes = Vec::new();
    let mut at = 0;
    while at + 3 <= stream.len() {
        if stream[at] == 0 && stream[at + 1] == 0 && stream[at + 2] == 1 {
            let mut start = at;
            while start > 0 && stream[start - 1] == 0 {
                start -= 1;
            }
            codes.push((start, at + 3));
            at += 3;
        } else {
            at += 1;
        }
    }
    codes
        .iter()
        .enumerate()
        .map(|(index, &(start, payload))| {
            let end = codes.get(index + 1).map_or(stream.len(), |next| next.0);
            (start, &stream[payload..end.max(payload)])
        })
        .collect()
}

/// The NAL units of an Annex-B `stream`, without their start codes.
#[cfg(test)]
pub(crate) fn nal_payloads(stream: &[u8]) -> Vec<&[u8]> {
    nal_units(stream)
        .into_iter()
        .map(|(_, payload)| payload)
        .collect()
}

/// The access units of an Annex-B `stream`, in decoding order, each with its start codes.
#[must_use]
pub fn access_units(codec: Codec, stream: &[u8]) -> Vec<&[u8]> {
    let mut units = Vec::new();
    let mut open: Option<usize> = None;
    let mut has_slice = false;
    for (start, nal) in nal_units(stream) {
        let kind = kind(codec, nal);
        let starts_next = has_slice && matches!(kind, Kind::FirstSlice | Kind::Leading);
        match open {
            Some(begin) if starts_next => {
                units.push(&stream[begin..start]);
                open = Some(start);
                has_slice = false;
            }
            None => open = Some(start),
            Some(_) => {}
        }
        has_slice |= matches!(kind, Kind::FirstSlice | Kind::Slice);
    }
    if let Some(begin) = open {
        units.push(&stream[begin..]);
    }
    units
}

#[cfg(test)]
mod tests {
    use super::{Codec, access_units};

    /// SPS, PPS and an IDR slice make one unit; a slice with `first_mb_in_slice` 0 starts the
    /// next, and the second slice of that picture stays with it.
    #[test]
    fn h264_units_end_where_the_next_picture_begins() {
        let stream = [
            0, 0, 0, 1, 0x67, 0x42, // SPS
            0, 0, 1, 0x68, 0xce, // PPS
            0, 0, 1, 0x65, 0x88, 0x10, // IDR, first_mb_in_slice 0
            0, 0, 1, 0x41, 0x9a, // non-IDR, first_mb_in_slice 0
            0, 0, 1, 0x41, 0x40, 0x22, // non-IDR, first_mb_in_slice 1
        ];
        let units = access_units(Codec::H264, &stream);
        assert_eq!(units.len(), 2);
        assert_eq!(units[0], &stream[..17]);
        assert_eq!(units[1], &stream[17..]);
    }

    /// A parameter set after a picture's slices starts a new access unit.
    #[test]
    fn h264_parameter_sets_after_a_picture_start_the_next_unit() {
        let stream = [
            0, 0, 1, 0x65, 0x88, // IDR
            0, 0, 1, 0x67, 0x42, // SPS
            0, 0, 1, 0x65, 0x88, // IDR
        ];
        let units = access_units(Codec::H264, &stream);
        assert_eq!(units, vec![&stream[..5], &stream[5..]]);
    }

    /// HEVC: VPS, SPS, PPS and the first slice segment, then a picture whose first segment has
    /// `first_slice_segment_in_pic_flag` set, and a dependent segment that has it clear.
    #[test]
    fn hevc_units_end_where_the_next_picture_begins() {
        let stream = [
            0, 0, 0, 1, 0x40, 0x01, // VPS (32)
            0, 0, 1, 0x42, 0x01, // SPS (33)
            0, 0, 1, 0x44, 0x01, // PPS (34)
            0, 0, 1, 0x26, 0x01, 0xaf, // IDR_W_RADL (19), first segment
            0, 0, 1, 0x02, 0x01, 0xd0, // TRAIL_R (1), first segment
            0, 0, 1, 0x02, 0x01, 0x40, // TRAIL_R (1), a later segment
        ];
        let units = access_units(Codec::Hevc, &stream);
        assert_eq!(units.len(), 2);
        assert_eq!(units[0], &stream[..22]);
        assert_eq!(units[1], &stream[22..]);
    }

    /// Bytes before the first start code belong to no unit, and a stream with no start code
    /// has none.
    #[test]
    fn leading_garbage_and_empty_streams() {
        assert!(access_units(Codec::H264, &[1, 2, 3]).is_empty());
        let stream = [9, 9, 0, 0, 1, 0x65, 0x88];
        assert_eq!(access_units(Codec::H264, &stream), vec![&stream[2..]]);
    }

    /// The fixture splits into one unit per coded picture: 30 frames.
    #[test]
    fn the_fixtures_hold_thirty_pictures() {
        let avc = include_bytes!("../tests/data/probe_avc.h264");
        assert_eq!(access_units(Codec::H264, avc).len(), 30);
        let hevc = include_bytes!("../tests/data/probe_hevc.h265");
        assert_eq!(access_units(Codec::Hevc, hevc).len(), 30);
    }
}
