//! Guest image formats sampled on a device (D923): each format the host samples natively, and the
//! same texels widened on the CPU as a host without it would bind them, read back the same.
//!
//! The device's own decode is the oracle for the CPU's. Every texture is eight by eight texels
//! drawn over an eight-by-eight frame, so each pixel is one texel, point-sampled. A block-compressed
//! format is fed pseudo-random blocks, which reach every mode and partition of `BC6H` and `BC7` in
//! a few dozen.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::{Pixels, draw_with_guest_texture};
use orbistoun_gpu_vulkan::guest_formats::widening;
use orbistoun_spirv::{Lod, interpolated_vertex_module, sampling_fragment_module};

/// Whether a device is available, said aloud when it is not.
fn device_or_skip(what: &str) -> bool {
    match probe() {
        Availability::Available { properties } => {
            println!("[{what}] device: {}", properties.device);
            true
        }
        Availability::Unavailable { reason } => {
            println!("[{what}] SKIPPED - no device: {reason}");
            false
        }
    }
}

/// The identity selects: `X`, `Y`, `Z`, `W`.
const IDENTITY: [u8; 4] = [4, 5, 6, 7];

/// An eight-by-eight texture of `words` in format `code` under `selects`, drawn once.
fn draw(code: u16, selects: [u8; 4], words: &[u32]) -> Pixels {
    // The triangle reaches 1 at the frame's right and bottom edges, sweeping the texture once.
    let corners = [
        [0.0, 0.0, 0.0, 1.0],
        [2.0, 0.0, 0.0, 1.0],
        [0.0, 2.0, 0.0, 1.0],
    ];
    draw_with_guest_texture(
        (
            &interpolated_vertex_module(corners),
            &sampling_fragment_module(Lod::Zero),
        ),
        [1.0, 0.0, 1.0, 1.0],
        (8, 8),
        (words, 8, 8),
        (code, selects),
    )
    .unwrap_or_else(|e| panic!("format {code}: the draw ran: {e}"))
}

/// The largest difference between two frames' channels, and where.
fn widest(first: &Pixels, second: &Pixels) -> (u8, (u32, u32)) {
    let mut worst = (0, (0, 0));
    for y in 0..8 {
        for x in 0..8 {
            let one = first.at(x, y).expect("inside");
            let other = second.at(x, y).expect("inside");
            for (left, right) in one.into_iter().zip(other) {
                let difference = left.abs_diff(right);
                if difference > worst.0 {
                    worst = (difference, (x, y));
                }
            }
        }
    }
    worst
}

/// A small generator of the words a texture is fed: the same every run.
struct Words(u64);

impl Words {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 32) as u32
    }

    fn take(&mut self, count: usize) -> Vec<u32> {
        (0..count).map(|_| self.next()).collect()
    }
}

/// Every block-compressed variant's blocks, sampled natively and widened on the CPU, read back the
/// same: within a unit for `BC6H` and `BC7`, whose interpolation the specifications fix (three
/// for sRGB, whose decode steepens a unit near white), and within 3% for `BC1`-`BC5`, whose
/// rounding of an interpolated value is the implementation's. A misread index, partition or mode
/// is wrong by far more.
#[test]
fn every_block_format_widens_as_the_device_decodes_it() {
    if !device_or_skip("every_block_format_widens_as_the_device_decodes_it") {
        return;
    }
    let mut words = Words(0x5eed);
    for code in 169_u16..=182 {
        let name = orbistoun_gpu::formats::name(u32::from(code)).expect("named");
        let sampled = orbistoun_gpu::formats::sampled(u32::from(code)).expect("sampled");
        // Four blocks make the eight-by-eight texture.
        let per_texture = 4 * sampled.layout.element_bytes() as usize / 4;
        // `BC1`-`BC5`'s thirds, fifths and sevenths are the implementation's to round: one device's
        // green reads 113 where `(2a + b) / 3` is 117, and its six-value `BC4` index 4 reads
        // `(3a + 5b) / 8` for `(2a + 3b) / 5`. `BC6H` and `BC7` weigh in sixty-fourths, exactly.
        // The rounding is a share of the endpoints' span, which is two for a signed format.
        let rounded = code <= 178;
        let tolerance = match (rounded, name.ends_with("SRGB")) {
            (true, false) if name.ends_with("SNORM") => 16,
            (true, false) => 8,
            (true, true) => 20,
            (false, false) => 1,
            (false, true) => 3,
        };
        // Each channel the format holds, and the constants a missing one reads as on the host.
        let selects: [u8; 4] = std::array::from_fn(|channel| match sampled.channels[channel] {
            orbistoun_gpu::formats::HostChannel::Missing => u8::from(channel == 3),
            _ => IDENTITY[channel],
        });
        for round in 0..16 {
            let blocks = words.take(per_texture);
            let native = draw(code, selects, &blocks);
            let widened = widening(|| draw(code, selects, &blocks));
            let (difference, at) = widest(&native, &widened);
            assert!(
                difference <= tolerance,
                concat!(
                    "{} round {}: the CPU's decode is {} from the device's at {:?} ",
                    "(device {:?}, CPU {:?}); blocks {:08x?}"
                ),
                name,
                round,
                difference,
                at,
                native.at(at.0, at.1),
                widened.at(at.0, at.1),
                blocks
            );
        }
    }
}

/// Uncompressed formats - packed, sixteen- and thirty-two-bit, shared-exponent and sRGB - sampled
/// natively and widened read back the same, every texel of random bits; and those with no host
/// format of their own widen to their texels' values.
#[test]
fn uncompressed_formats_widen_as_the_device_reads_them() {
    if !device_or_skip("uncompressed_formats_widen_as_the_device_reads_them") {
        return;
    }
    let mut words = Words(0xf0f0);
    // Random bits, which the frame holds, clamps, or - a not-a-number - writes as zero either way.
    let codes: [(u16, [u8; 4]); 21] = [
        (13, [4, 0, 0, 1]),
        (29, [4, 5, 0, 1]),
        (22, [4, 0, 0, 1]),
        (36, [4, 5, 6, 1]),
        (132, [4, 5, 6, 1]),
        (1, [4, 0, 0, 1]),
        (14, [4, 5, 0, 1]),
        (7, [4, 0, 0, 1]),
        (23, [4, 5, 0, 1]),
        (65, IDENTITY),
        (24, [4, 5, 0, 1]),
        (50, IDENTITY),
        (133, [4, 5, 6, 1]),
        (134, IDENTITY),
        (135, IDENTITY),
        (136, IDENTITY),
        (137, [4, 5, 0, 1]),
        (128, [4, 0, 0, 1]),
        (129, [4, 5, 0, 1]),
        (130, IDENTITY),
        (57, IDENTITY),
    ];
    for (code, selects) in codes {
        let name = orbistoun_gpu::formats::name(u32::from(code)).expect("named");
        let sampled = orbistoun_gpu::formats::sampled(u32::from(code)).expect("sampled");
        let count = (64 * sampled.layout.element_bytes() as usize).div_ceil(4);
        let tolerance = if name.ends_with("SRGB") { 3 } else { 1 };
        for round in 0..4 {
            let texels = words.take(count);
            let native = draw(code, selects, &texels);
            let widened = widening(|| draw(code, selects, &texels));
            let (difference, at) = widest(&native, &widened);
            assert!(
                difference <= tolerance,
                concat!(
                    "{} round {}: widened is {} from native at {:?} ",
                    "(native {:?}, widened {:?})"
                ),
                name,
                round,
                difference,
                at,
                native.at(at.0, at.1),
                widened.at(at.0, at.1)
            );
        }
    }
    // `10_10_10_2_UNORM` has no host format: X is the low two bits, then Y, Z and W ten each. X = 3
    // (full), Y = 0, Z = 512 (about half) and W = 1023.
    let word: u32 = 3 | 1023 << 22 | 512 << 12;
    let texels = vec![word; 64];
    let pixels = draw(44, IDENTITY, &texels);
    assert_eq!(
        pixels.at(3, 3),
        Some([255, 0, 128, 255]),
        "10_10_10_2_UNORM"
    );
}

/// A select moves a channel within the view, as the guest's `DST_SEL` moves it within the sample:
/// `5_6_5` read `Z`, `Y`, `X`, one returns its blue in red.
#[test]
fn selects_move_channels_natively_and_widened() {
    if !device_or_skip("selects_move_channels_natively_and_widened") {
        return;
    }
    // X (the low five bits) full, Y zero, Z zero: red under the identity, blue reversed.
    let texels = vec![0x001F_001F_u32; 32];
    let straight = draw(133, [4, 5, 6, 1], &texels);
    assert_eq!(straight.at(2, 2), Some([255, 0, 0, 255]));
    let reversed = draw(133, [6, 5, 4, 1], &texels);
    assert_eq!(reversed.at(2, 2), Some([0, 0, 255, 255]));
    let widened = widening(|| draw(133, [6, 5, 4, 1], &texels));
    assert_eq!(widened.at(2, 2), Some([0, 0, 255, 255]));
}
