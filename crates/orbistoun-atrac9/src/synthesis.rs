//! Turning a block's quantised spectra into samples: dequantisation, intensity stereo, scaling,
//! band extension and the inverse transform (LibAtrac9 `C/src/decoder.c`, `quantization.c`,
//! `band_extension.c`, `imdct.c`).

use crate::tables::{
    BEX_GROUPS, BEX_MODE0_BANDS3, BEX_MODE0_BANDS4, BEX_MODE0_BANDS5, BEX_MODE2_SCALE,
    BEX_MODE3_INITIAL, BEX_MODE3_RATE, BEX_MODE4_MULTIPLIER, BlockKind, QUANTIZER_FINE_STEP,
    QUANTIZER_STEP, UNIT_COEFF_COUNT, UNIT_COEFF_INDEX, spectrum_scale,
};
use crate::unpack::{Block, Channel, MAX_FRAME_SAMPLES, Noise};
use std::f64::consts::PI;

/// Synthesises one decoded block's samples into each channel's `pcm` (`decoder.c` 28-44).
pub(crate) fn synthesize(block: &mut Block, transform: &Transform) {
    for channel in &mut block.channels {
        dequantize(channel);
    }
    apply_intensity_stereo(block);
    let units = block.unit_count;
    for channel in &mut block.channels {
        for i in 0..units {
            let scale = spectrum_scale(channel.scale_factors[i]);
            for value in &mut channel.spectra[UNIT_COEFF_INDEX[i]..UNIT_COEFF_INDEX[i + 1]] {
                *value *= scale;
            }
        }
    }
    if block.band_extension && block.has_extension_data {
        for channel in &mut block.channels {
            apply_band_extension(channel, units);
        }
    }
    for channel in &mut block.channels {
        transform.inverse(&channel.spectra, &mut channel.overlap, &mut channel.pcm);
    }
}

/// Coarse value times its step, plus fine value times its step, per coded unit
/// (`quantization.c` 8-35).
fn dequantize(channel: &mut Channel) {
    channel.spectra = [0.0; MAX_FRAME_SAMPLES];
    for i in 0..channel.coded_units {
        let step = QUANTIZER_STEP[channel.precisions[i] as usize];
        let fine_step = QUANTIZER_FINE_STEP[channel.precisions_fine[i] as usize];
        for k in UNIT_COEFF_INDEX[i]..UNIT_COEFF_INDEX[i + 1] {
            channel.spectra[k] = f64::from(channel.quantized[k]) * step
                + f64::from(channel.quantized_fine[k]) * fine_step;
        }
    }
}

/// Copies the primary channel's shared units into the other, negated where the sign says
/// (`decoder.c` 72-98).
fn apply_intensity_stereo(block: &mut Block) {
    if block.kind != BlockKind::Stereo || block.stereo_unit >= block.unit_count {
        return;
    }
    let source = block.channels[block.primary_channel].spectra;
    let dest = &mut block.channels[1 - block.primary_channel].spectra;
    for i in block.stereo_unit..block.unit_count {
        let sign = if block.joint_signs[i] { -1.0 } else { 1.0 };
        for k in UNIT_COEFF_INDEX[i]..UNIT_COEFF_INDEX[i + 1] {
            dest[k] = sign * source[k];
        }
    }
}

/// Fills the bands above the coded ones from mirrored copies of those below, then shapes them
/// in the channel's mode (`band_extension.c` 33-187).
fn apply_band_extension(channel: &mut Channel, group_a_unit: usize) {
    let (second_group_unit, third_group_unit, band_count) = BEX_GROUPS[group_a_unit - 13];
    let total_units = third_group_unit.max(22);
    let [a_bin, b_bin, c_bin, total_bins] = [
        group_a_unit,
        second_group_unit,
        third_group_unit,
        total_units,
    ]
    .map(|u| UNIT_COEFF_INDEX[u]);
    let spectra = &mut channel.spectra;
    for (start, end) in [(a_bin, b_bin), (b_bin, c_bin), (c_bin, total_bins)] {
        for i in 0..end - start {
            spectra[start + i] = spectra[start - i - 1];
        }
    }
    let values = channel.bex_values;
    let sf = channel.scale_factors;
    let scale_range =
        |spectra: &mut [f64; MAX_FRAME_SAMPLES], range: std::ops::Range<usize>, scale: f64| {
            for value in &mut spectra[range] {
                *value *= scale;
            }
        };
    match channel.bex_mode {
        0 => {
            let units = total_units - group_a_unit;
            let mut scales = [0.0; 6];
            match band_count {
                3 => {
                    scales[0] = BEX_MODE0_BANDS3[0][values[0]];
                    scales[1] = BEX_MODE0_BANDS3[1][values[0]];
                    scales[2] = BEX_MODE0_BANDS3[2][values[1]];
                    scales[3] = BEX_MODE0_BANDS3[3][values[2]];
                    scales[4] = BEX_MODE0_BANDS3[4][values[3]];
                }
                4 => {
                    scales[0] = BEX_MODE0_BANDS4[0][values[0]];
                    scales[1] = BEX_MODE0_BANDS4[1][values[0]];
                    scales[2] = BEX_MODE0_BANDS4[2][values[1]];
                    scales[3] = BEX_MODE0_BANDS4[3][values[2]];
                    scales[4] = BEX_MODE0_BANDS4[4][values[3]];
                }
                5 => {
                    scales[0] = BEX_MODE0_BANDS5[0][values[0]];
                    scales[1] = BEX_MODE0_BANDS5[1][values[1]];
                    scales[2] = BEX_MODE0_BANDS5[2][values[1]];
                }
                _ => {}
            }
            scales[units - 1] = spectrum_scale(sf[group_a_unit]);
            add_noise(
                channel,
                UNIT_COEFF_INDEX[total_units - 1],
                UNIT_COEFF_COUNT[total_units - 1],
            );
            scale_units(&mut channel.spectra, &scales, group_a_unit, total_units);
        }
        1 => {
            let mut scales = [0.0; 6];
            for i in group_a_unit..total_units {
                scales[i - group_a_unit] = spectrum_scale(sf[i]);
            }
            add_noise(channel, a_bin, total_bins - a_bin);
            scale_units(&mut channel.spectra, &scales, group_a_unit, total_units);
        }
        2 => {
            scale_range(spectra, a_bin..b_bin, BEX_MODE2_SCALE[values[0]]);
            scale_range(spectra, b_bin..c_bin, BEX_MODE2_SCALE[values[1]]);
        }
        3 => {
            let rate = 2.0_f64.powf(BEX_MODE3_RATE[values[1]]);
            let mut scale = BEX_MODE3_INITIAL[values[0]];
            for value in &mut spectra[a_bin..total_bins] {
                scale *= rate;
                *value *= scale;
            }
        }
        _ => {
            let multiplier = BEX_MODE4_MULTIPLIER[values[0]];
            scale_range(spectra, a_bin..b_bin, 0.707_946_8 * multiplier);
            scale_range(spectra, b_bin..c_bin, 0.501_190_2 * multiplier);
            scale_range(spectra, c_bin..total_bins, 0.354_827_9 * multiplier);
        }
    }
}

/// Multiplies each unit from `start` to `end` by its scale (`band_extension.c` 146-155).
fn scale_units(
    spectra: &mut [f64; MAX_FRAME_SAMPLES],
    scales: &[f64; 6],
    start: usize,
    end: usize,
) {
    for i in start..end {
        for value in &mut spectra[UNIT_COEFF_INDEX[i]..UNIT_COEFF_INDEX[i + 1]] {
            *value *= scales[i - start];
        }
    }
}

/// Overwrites `count` coefficients from `index` with noise in [-1, 1], seeding the channel's
/// generator from its scale factors the first time (`band_extension.c` 175-187).
fn add_noise(channel: &mut Channel, index: usize, count: usize) {
    let sf = channel.scale_factors;
    let noise = channel
        .noise
        .get_or_insert_with(|| Noise::new((543 * (sf[8] + sf[12] + sf[15] + 1)) as u16));
    for value in &mut channel.spectra[index..index + count] {
        *value = f64::from(noise.next_value()) / 65535.0 * 2.0 - 1.0;
    }
}

/// The inverse transform for one frame length: its window and twiddle tables (`decinit.c`
/// 138-195).
#[derive(Debug)]
pub(crate) struct Transform {
    /// `log2` of the frame length.
    bits: u32,
    /// The synthesis window.
    window: Vec<f64>,
    /// `sin` and `cos` of `pi (4i + 1) / 4n`, for each power of two `n` up to the frame.
    trig: Vec<(Vec<f64>, Vec<f64>)>,
    /// The output permutation of the transform's last stage.
    shuffle: Vec<usize>,
}

impl Transform {
    /// The transform for frames of `2^bits` samples.
    pub(crate) fn new(bits: u32) -> Self {
        let size = 1_usize << bits;
        let trig = (0..=bits)
            .map(|b| {
                let n = 1_u32 << b;
                (0..n)
                    .map(|i| PI * f64::from(4 * i + 1) / f64::from(4 * n))
                    .map(|angle| (angle.sin(), angle.cos()))
                    .unzip()
            })
            .collect();
        let shuffle = (0..size)
            .map(|i| ((i ^ (i / 2)) as u32).reverse_bits() as usize >> (32 - bits))
            .collect();
        // Separate multiplies and adds, never fused, so every value rounds as the reference's
        // does.
        let length = f64::from(1_u32 << bits);
        let analysis: Vec<f64> = (0..1_u32 << bits)
            .map(|i| ((((f64::from(i) + 0.5) / length - 0.5) * PI).sin() + 1.0) * 0.5)
            .collect();
        let window = (0..size)
            .map(|i| {
                let mirror = analysis[size - 1 - i];
                analysis[i] / (mirror * mirror + analysis[i] * analysis[i])
            })
            .collect();
        Self {
            bits,
            window,
            trig,
            shuffle,
        }
    }

    /// One frame's samples from its spectrum, overlapped with the previous frame's second half,
    /// which it replaces (`imdct.c` 6-23).
    fn inverse(
        &self,
        input: &[f64; MAX_FRAME_SAMPLES],
        previous: &mut [f64; MAX_FRAME_SAMPLES],
        output: &mut [f64; MAX_FRAME_SAMPLES],
    ) {
        let size = 1 << self.bits;
        let half = size / 2;
        let dct = self.dct4(input);
        let window = &self.window;
        for i in 0..half {
            output[i] = window[i] * dct[i + half] + previous[i];
            output[i + half] = window[i + half] * -dct[size - 1 - i] - previous[i + half];
            previous[i] = window[size - 1 - i] * -dct[half - i - 1];
            previous[i + half] = window[half - i - 1] * dct[i];
        }
    }

    /// The type-IV DCT of `input`'s first `2^bits` values (`imdct.c` 25-82).
    pub(crate) fn dct4(&self, input: &[f64; MAX_FRAME_SAMPLES]) -> [f64; MAX_FRAME_SAMPLES] {
        let size = 1 << self.bits;
        let last = size - 1;
        let mut temp = [0.0; MAX_FRAME_SAMPLES];
        let (sin, cos) = &self.trig[self.bits as usize];
        for i in 0..size / 2 {
            let (a, b) = (input[2 * i], input[last - 2 * i]);
            temp[2 * i] = a * cos[i] + b * sin[i];
            temp[2 * i + 1] = a * sin[i] - b * cos[i];
        }
        let stages = self.bits - 1;
        for stage in 0..stages {
            let block_bits = stages - stage;
            let block_size = 1 << block_bits;
            let block_half = 1 << (block_bits - 1);
            let (sin, cos) = &self.trig[block_bits as usize - 1];
            for block in 0..1 << stage {
                for i in 0..block_half {
                    let front = (block * block_size + i) * 2;
                    let back = front + block_size;
                    let a = temp[front] - temp[back];
                    let b = temp[front + 1] - temp[back + 1];
                    temp[front] += temp[back];
                    temp[front + 1] += temp[back + 1];
                    temp[back] = a * cos[i] + b * sin[i];
                    temp[back + 1] = a * sin[i] - b * cos[i];
                }
            }
        }
        let mut output = [0.0; MAX_FRAME_SAMPLES];
        for (out, &from) in output.iter_mut().zip(&self.shuffle) {
            *out = temp[from];
        }
        output
    }
}
