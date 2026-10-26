//! A captured dispatch's images read as its program reads them, for a host trace of the dispatch
//! (`trace-dispatch`): the image a descriptor names, a texel of it as a shader receives it, and a
//! sample of it through a sampler.
//!
//! The capture keeps each image's texels but not the descriptor it was bound from, so a descriptor
//! is matched to a captured image by its extent, its layers, and its format where several share an
//! extent. A texel is answered as the image instruction returns it: normalised and float channels
//! as float bits, integer channels as integers, ordered by the descriptor's destination selects -
//! which an `8_8_8_8_UNORM` image's capture already holds its texels in, where they reorder its
//! channels.
//! A sample is a point or bilinear filter of the first level, which a device's own filter may round
//! differently in its last bits.

use orbistoun_gpu::{
    TextureFilter, TextureWrap, decode_image_descriptor, decode_sampler_descriptor,
};

use super::{Capture, CapturedImage};
use crate::compute_images::DispatchFormat;

/// GFX10 image format codes (`SQ_IMG_RSRC_WORD1.FORMAT`) and the formats a captured image of each
/// may be kept in: `8_UINT` (5) a word a texel, as the binding widens it.
const FORMAT_CODES: [(u32, DispatchFormat); 9] = [
    (1, DispatchFormat::R8),
    (5, DispatchFormat::R8Uint),
    (5, DispatchFormat::R32Uint),
    (7, DispatchFormat::R16Unorm),
    (20, DispatchFormat::R32Uint),
    (22, DispatchFormat::R32Float),
    (50, DispatchFormat::Rgb10A2),
    (56, DispatchFormat::Rgba8),
    (57, DispatchFormat::Rgba8Snorm),
];

/// A destination select's value that reads one (`SQ_SEL_1`); `SQ_SEL_0` is zero and 4-7 the
/// texel's four channels.
const SELECT_ONE: u32 = 1;
const SELECT_X: u32 = 4;

impl Capture<'_> {
    /// The index of the captured image `descriptor` names: the one image of its width, height and
    /// layers, or of those and its format where several share them. `None` where none, or more
    /// than one, matches.
    #[must_use]
    pub fn image_named(&self, descriptor: [u32; 8]) -> Option<usize> {
        let decoded = decode_image_descriptor(descriptor);
        // `SQ_IMG_RSRC_WORD4.DEPTH` (12:0): an array's layers less one.
        let layers = (descriptor[4] & 0x1fff) + 1;
        let shaped: Vec<usize> = self
            .images
            .iter()
            .enumerate()
            .filter(|(_, image)| {
                image.width == decoded.width
                    && image.height == decoded.height
                    && image.layers.unwrap_or(1).max(image.depth.unwrap_or(1)) <= layers
            })
            .map(|(index, _)| index)
            .collect();
        if let [only] = shaped[..] {
            return Some(only);
        }
        let formats: Vec<DispatchFormat> = FORMAT_CODES
            .iter()
            .filter(|(code, _)| *code == decoded.format)
            .map(|(_, format)| *format)
            .collect();
        let formed: Vec<usize> = shaped
            .into_iter()
            .filter(|&index| formats.contains(&self.images[index].format))
            .collect();
        match formed[..] {
            [only] => Some(only),
            _ => None,
        }
    }

    /// Texel `(x, y, layer)` of the first level of the image `descriptor` names, as an image load
    /// returns it ([`CapturedImage::components`], ordered by the descriptor's selects). `None`
    /// where no captured image is named, or the texel lies outside it.
    #[must_use]
    pub fn texel_read(&self, descriptor: [u32; 8], [x, y, layer]: [u32; 3]) -> Option<[u32; 4]> {
        let image = &self.images[self.image_named(descriptor)?];
        let texel = image.components([x, y, layer])?;
        Some(selected(texel, descriptor[3], image.format))
    }

    /// A sample of the first level of the image `descriptor` names at normalised `(u, v)` in
    /// `layer`, through `sampler`: a point or bilinear filter, as the sampler's magnifying filter
    /// says, with its wrap across and down. `None` where no captured image is named, the sampler
    /// asks for what is not modelled - a level past the first, a border, an anisotropic or
    /// mirrored wrap - or a texel the filter reads is not known.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        clippy::many_single_char_names,
        reason = "an image's extent as a float, and a bilinear filter's coordinates and weights"
    )]
    pub fn texel_sample(
        &self,
        descriptor: [u32; 8],
        sampler: [u32; 4],
        [u, v, layer]: [f32; 3],
        level: f32,
    ) -> Option<[u32; 4]> {
        let image = &self.images[self.image_named(descriptor)?];
        let sampling = decode_sampler_descriptor(sampler).ok()?;
        if level != 0.0
            || sampling.anisotropy != 1
            || sampling.magnify != sampling.minify
            || sampling.mirror_once.contains(&true)
        {
            return None;
        }
        let layer = layer.round_ties_even().max(0.0) as u32;
        let texel = |x: i64, y: i64| -> Option<[f32; 4]> {
            let x = wrapped(x, image.width, sampling.wrap[0])?;
            let y = wrapped(y, image.height, sampling.wrap[1])?;
            let words = image.components([x, y, layer])?;
            Some(words.map(f32::from_bits))
        };
        let (width, height) = (image.width as f32, image.height as f32);
        let filtered = match sampling.magnify {
            TextureFilter::Nearest => {
                texel((u * width).floor() as i64, (v * height).floor() as i64)?
            }
            TextureFilter::Linear => {
                let (across, down) = (u * width - 0.5, v * height - 0.5);
                let (x, y) = (across.floor(), down.floor());
                // Each weight to the eight fractional bits a texture unit filters with
                // (`subTexelPrecisionBits`), as the device rounds it.
                let weight = |fraction: f32| (fraction * 256.0).round_ties_even() / 256.0;
                let (a, b) = (weight(across - x), weight(down - y));
                let (x, y) = (x as i64, y as i64);
                let corners = [
                    texel(x, y)?,
                    texel(x + 1, y)?,
                    texel(x, y + 1)?,
                    texel(x + 1, y + 1)?,
                ];
                std::array::from_fn(|c| {
                    let top = corners[0][c] * (1.0 - a) + corners[1][c] * a;
                    let bottom = corners[2][c] * (1.0 - a) + corners[3][c] * a;
                    top * (1.0 - b) + bottom * b
                })
            }
        };
        if is_integer(image.format) {
            return None;
        }
        Some(selected(
            filtered.map(f32::to_bits),
            descriptor[3],
            image.format,
        ))
    }
}

impl CapturedImage<'_> {
    /// Texel `(x, y, layer)`'s four channels as a shader receives them, before any select: unsigned
    /// and signed normalised and float channels as float bits, integer channels as integers, a
    /// channel the format lacks zero and a missing alpha one. `None` outside the image, or for a
    /// format not decoded here.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "a normalised channel's steps, at most 16 bits"
    )]
    pub fn components(&self, [x, y, layer]: [u32; 3]) -> Option<[u32; 4]> {
        let layers = self.layers.unwrap_or(1).max(self.depth.unwrap_or(1));
        if x >= self.width || y >= self.height || layer >= layers {
            return None;
        }
        let words = words_per_texel(self.format);
        let at = ((layer as usize * self.height as usize + y as usize) * self.width as usize
            + x as usize)
            * words;
        let texel = self.texels.get(at..at + words)?;
        let word = texel[0];
        let unorm = |bits: u32, max: u32| (bits as f32 / max as f32).to_bits();
        let one = 1.0_f32.to_bits();
        Some(match self.format {
            DispatchFormat::Rgba8 => word.to_le_bytes().map(|byte| unorm(u32::from(byte), 255)),
            DispatchFormat::Rgba8Snorm => word.to_le_bytes().map(|byte| {
                (f32::from(i8::from_le_bytes([byte])) / 127.0)
                    .max(-1.0)
                    .to_bits()
            }),
            DispatchFormat::R8 => [unorm(word & 0xff, 255), 0, 0, one],
            DispatchFormat::R16Unorm => [unorm(word & 0xffff, 0xffff), 0, 0, one],
            DispatchFormat::Rg8Unorm => [
                unorm(word & 0xff, 255),
                unorm(word >> 8 & 0xff, 255),
                0,
                one,
            ],
            DispatchFormat::R32Float => [word, 0, 0, one],
            DispatchFormat::R32Uint => [word, 0, 0, 1],
            DispatchFormat::R16Uint => [word & 0xffff, 0, 0, 1],
            DispatchFormat::R8Uint => [word & 0xff, 0, 0, 1],
            DispatchFormat::Rgba8Uint => word.to_le_bytes().map(u32::from),
            DispatchFormat::Rg32Uint => [word, texel[1], 0, 1],
            DispatchFormat::Rgba32Uint => [word, texel[1], texel[2], texel[3]],
            DispatchFormat::Rgb10A2 => [
                unorm(word & 0x3ff, 0x3ff),
                unorm(word >> 10 & 0x3ff, 0x3ff),
                unorm(word >> 20 & 0x3ff, 0x3ff),
                unorm(word >> 30, 3),
            ],
            DispatchFormat::Rgba16Unorm => [
                unorm(word & 0xffff, 0xffff),
                unorm(word >> 16, 0xffff),
                unorm(texel[1] & 0xffff, 0xffff),
                unorm(texel[1] >> 16, 0xffff),
            ],
            // Blocks are not texels: a block image is not read texel by texel here (D990).
            DispatchFormat::R16Float
            | DispatchFormat::Rg16Float
            | DispatchFormat::Rgba16Float
            | DispatchFormat::Float11_11_10
            | DispatchFormat::Blocks { .. } => return None,
        })
    }
}

impl CapturedImage<'_> {
    /// The word a store of `components` - each a channel's place and the value a shader stored,
    /// float bits for a normalised or float channel - leaves in a one-word texel of this image,
    /// over `before` for the channels it does not store: normalised channels rounded to the nearest
    /// step, as a device converts them. `None` for a format not encoded here, or a channel value
    /// that is not known.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "a normalised channel's steps, at most 16 bits"
    )]
    pub fn stored_word(&self, components: &[(u32, Option<u32>)], before: u32) -> Option<u32> {
        let unorm = |bits: u32, max: u32| {
            let value = f32::from_bits(bits);
            let value = if value.is_nan() {
                0.0
            } else {
                value.clamp(0.0, 1.0)
            };
            (value * max as f32).round_ties_even() as u32
        };
        let mut word = before;
        for &(channel, value) in components {
            let value = value?;
            let (shift, bits) = match self.format {
                DispatchFormat::R8 if channel == 0 => (0, unorm(value, 255)),
                DispatchFormat::Rgba8 if channel < 4 => (8 * channel, unorm(value, 255)),
                DispatchFormat::R32Float | DispatchFormat::R32Uint if channel == 0 => {
                    word = value;
                    continue;
                }
                DispatchFormat::R8 | DispatchFormat::Rgba8 => continue,
                _ => return None,
            };
            word = (word & !(0xff << shift)) | bits << shift;
        }
        Some(word)
    }
}

/// Words one texel of `format` takes in a captured image: one for a texel of four bytes or fewer.
const fn words_per_texel(format: DispatchFormat) -> usize {
    match format {
        DispatchFormat::Rgba16Float | DispatchFormat::Rgba16Unorm | DispatchFormat::Rg32Uint => 2,
        DispatchFormat::Rgba32Uint => 4,
        _ => 1,
    }
}

/// Whether `format`'s channels are integers, which no filter blends.
const fn is_integer(format: DispatchFormat) -> bool {
    matches!(
        format,
        DispatchFormat::R8Uint
            | DispatchFormat::Rgba8Uint
            | DispatchFormat::Rgba32Uint
            | DispatchFormat::R32Uint
            | DispatchFormat::R16Uint
            | DispatchFormat::Rg32Uint
    )
}

/// `texel` ordered by the destination selects in `word3` (bits 11:0, three bits each for x, y, z
/// and w): zero, one - an integer one for an integer format - or one of its channels. An
/// `8_8_8_8_UNORM` image whose selects reorder its four channels is captured with its texels
/// already in the order the shader reads them, as the binding hands them to the device
/// (`channel_order` in the pipeline), so it is answered as it is.
fn selected(texel: [u32; 4], word3: u32, format: DispatchFormat) -> [u32; 4] {
    let selects: [u32; 4] = std::array::from_fn(|component| word3 >> (3 * component) & 7);
    let reordering = selects.iter().all(|select| *select >= SELECT_X)
        && selects
            .iter()
            .fold(0_u32, |seen, select| seen | 1 << (select - SELECT_X))
            == 0xf;
    if format == DispatchFormat::Rgba8 && reordering {
        return texel;
    }
    let one = if is_integer(format) {
        1
    } else {
        1.0_f32.to_bits()
    };
    std::array::from_fn(|component| match selects[component] {
        SELECT_ONE => one,
        select @ SELECT_X.. => texel[(select - SELECT_X) as usize],
        _ => 0,
    })
}

/// Texel index `at` along an axis of `size` texels under `wrap`; `None` for a wrap not modelled.
fn wrapped(at: i64, size: u32, wrap: TextureWrap) -> Option<u32> {
    let size = i64::from(size.max(1));
    let at = match wrap {
        TextureWrap::Repeat => at.rem_euclid(size),
        TextureWrap::ClampToEdge => at.clamp(0, size - 1),
        TextureWrap::Mirror | TextureWrap::ClampHalfBorder | TextureWrap::ClampBorder => {
            return None;
        }
    };
    u32::try_from(at).ok()
}

#[cfg(test)]
mod tests {
    use super::{selected, words_per_texel};
    use crate::capture::{CapturedImage, ImageRole};
    use crate::compute_images::DispatchFormat;
    use std::borrow::Cow;

    fn image(format: DispatchFormat, texels: Vec<u32>, width: u32) -> CapturedImage<'static> {
        let height = u32::try_from(texels.len() / words_per_texel(format)).expect("texels") / width;
        CapturedImage {
            role: ImageRole::Fetched,
            texels: Cow::Owned(texels),
            format,
            width,
            height,
            depth: None,
            layers: None,
            sampling: None,
            cube: false,
        }
    }

    /// A texel of each kind of format as a shader receives it: normalised channels as floats over
    /// their range, a missing alpha one, integers as they are.
    #[test]
    fn a_texel_reads_as_a_shader_receives_it() {
        let rgba8 = image(DispatchFormat::Rgba8, vec![0x00ff_8000, 0xff00_00ff], 2);
        assert_eq!(
            rgba8.components([1, 0, 0]),
            Some([1.0_f32, 0.0, 0.0, 1.0].map(f32::to_bits))
        );
        assert_eq!(
            rgba8
                .components([0, 0, 0])
                .map(|texel| texel.map(f32::from_bits)),
            Some([0.0, 128.0 / 255.0, 1.0, 0.0])
        );
        let r32 = image(DispatchFormat::R32Uint, vec![9], 1);
        assert_eq!(r32.components([0, 0, 0]), Some([9, 0, 0, 1]));
        assert_eq!(r32.components([1, 0, 0]), None, "outside the image");
    }

    /// The selects order a texel's channels, and supply zero and one; an `8_8_8_8_UNORM` image whose
    /// selects only reorder its channels is captured already in the shader's order, and is answered
    /// as it is.
    #[test]
    fn selects_order_a_texel_unless_its_capture_already_is() {
        let texel = [10, 20, 30, 40];
        // X, Y, Z from R, G, B and W one: `4 | 5 << 3 | 6 << 6 | 1 << 9`.
        let rgb_one = 4 | 5 << 3 | 6 << 6 | 1 << 9;
        assert_eq!(
            selected(texel, rgb_one, DispatchFormat::Rgba8),
            [10, 20, 30, 1.0_f32.to_bits()]
        );
        assert_eq!(
            selected(texel, rgb_one, DispatchFormat::R32Uint),
            [10, 20, 30, 1]
        );
        // B, G, R, A: a reordering, which the capture already holds.
        let bgra = 6 | 5 << 3 | 4 << 6 | 7 << 9;
        assert_eq!(selected(texel, bgra, DispatchFormat::Rgba8), texel);
    }

    /// A store's channels land in the word as a device converts them: a normalised channel rounded
    /// to its nearest step and clamped, the others left as they were.
    #[test]
    fn a_stored_channel_converts_into_the_texel_word() {
        let r8 = image(DispatchFormat::R8, vec![0x77], 1);
        assert_eq!(
            r8.stored_word(&[(0, Some(0.5_f32.to_bits()))], 0x77),
            Some(128)
        );
        assert_eq!(
            r8.stored_word(&[(0, Some(2.0_f32.to_bits()))], 0),
            Some(255)
        );
        assert_eq!(r8.stored_word(&[(0, None)], 0), None);
        let rgba8 = image(DispatchFormat::Rgba8, vec![0], 1);
        assert_eq!(
            rgba8.stored_word(&[(1, Some(1.0_f32.to_bits()))], 0x1122_3344),
            Some(0x1122_ff44)
        );
    }
}
