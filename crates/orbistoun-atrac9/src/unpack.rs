//! Reading a frame's blocks: band layout, bit allocation, scale factors and quantised spectra
//! (LibAtrac9 `C/src/unpack.c`, `scale_factors.c`, `bit_allocation.c`).

use crate::Error;
use crate::bits::BitReader;
use crate::huffman::codebooks;
use crate::tables::{
    BAND_TO_UNIT_COUNT, BASE_CURVE, BEX_DATA_LENGTHS, BEX_GROUPS, BEX_VALUE_COUNTS, BlockKind,
    MAX_BAND_COUNT, MAX_EXTENSION_BAND, MAX_HUFF_PRECISION, MIN_BAND_COUNT, SCALE_FACTOR_WEIGHTS,
    UNIT_CODEBOOK_INDEX, UNIT_COEFF_COUNT, UNIT_COEFF_INDEX,
};

/// The most coefficients a frame holds per channel.
pub(crate) const MAX_FRAME_SAMPLES: usize = 256;

/// The most quantisation units a block codes.
const MAX_UNITS: usize = 30;

/// The state of the noise generator band extension fills with (`band_extension.c` 189-208).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Noise {
    /// The four 16-bit words of state, newest first.
    state: [u16; 4],
}

impl Noise {
    /// A generator seeded with `seed`.
    pub(crate) fn new(seed: u16) -> Self {
        let start = 0x4d93_i32.wrapping_mul(i32::from(seed ^ (seed >> 14)));
        Self {
            state: [3, 2, 1, 0].map(|k: i32| k.wrapping_sub(start) as u16),
        }
    }

    /// The next value.
    pub(crate) fn next_value(&mut self) -> u16 {
        let [newest, second, third, oldest] = self.state;
        let mixed = oldest ^ (oldest << 5);
        let next = mixed ^ newest ^ ((mixed ^ (newest >> 5)) >> 4);
        self.state = [next, newest, second, third];
        next
    }
}

/// One channel's state, kept from frame to frame where the format refers back to it.
#[derive(Clone, Debug)]
pub(crate) struct Channel {
    /// The second half of the last frame's inverse transform, overlapped with this one's.
    pub(crate) overlap: [f64; MAX_FRAME_SAMPLES],
    /// This frame's samples.
    pub(crate) pcm: [f64; MAX_FRAME_SAMPLES],
    /// This frame's spectrum.
    pub(crate) spectra: [f64; MAX_FRAME_SAMPLES],
    /// How many quantisation units this channel codes itself.
    pub(crate) coded_units: usize,
    /// The scale factor of each unit.
    pub(crate) scale_factors: [i32; MAX_UNITS + 1],
    /// The last frame's scale factors, which modes 2 and 3 code against.
    scale_factors_prev: [i32; MAX_UNITS + 1],
    /// Each unit's coarse precision, in bits.
    pub(crate) precisions: [i32; MAX_UNITS],
    /// Each unit's fine precision, in bits, past 15 coarse ones.
    pub(crate) precisions_fine: [i32; MAX_UNITS],
    /// Which codebook set each unit's spectrum is coded with.
    codebook_set: [usize; MAX_UNITS],
    /// The quantised coarse spectrum.
    pub(crate) quantized: [i32; MAX_FRAME_SAMPLES],
    /// The quantised fine spectrum.
    pub(crate) quantized_fine: [i32; MAX_FRAME_SAMPLES],
    /// Band extension's mode for this channel.
    pub(crate) bex_mode: usize,
    /// How many band-extension values this channel codes.
    bex_value_count: usize,
    /// The band-extension values.
    pub(crate) bex_values: [usize; 4],
    /// The noise generator, seeded the first time band extension needs it.
    pub(crate) noise: Option<Noise>,
}

impl Channel {
    /// A channel with nothing decoded.
    fn new() -> Self {
        Self {
            overlap: [0.0; MAX_FRAME_SAMPLES],
            pcm: [0.0; MAX_FRAME_SAMPLES],
            spectra: [0.0; MAX_FRAME_SAMPLES],
            coded_units: 0,
            scale_factors: [0; MAX_UNITS + 1],
            scale_factors_prev: [0; MAX_UNITS + 1],
            precisions: [0; MAX_UNITS],
            precisions_fine: [0; MAX_UNITS],
            codebook_set: [0; MAX_UNITS],
            quantized: [0; MAX_FRAME_SAMPLES],
            quantized_fine: [0; MAX_FRAME_SAMPLES],
            bex_mode: 0,
            bex_value_count: 0,
            bex_values: [0; 4],
            noise: None,
        }
    }
}

/// One block's state: its kind, its band layout - which a block may reuse from the frame before
/// - and its channels.
#[derive(Clone, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a one-bit field of the frame, named as the format names it"
)]
pub(crate) struct Block {
    /// What the block codes.
    pub(crate) kind: BlockKind,
    /// Its channels, one or two.
    pub(crate) channels: Vec<Channel>,
    /// Whether this frame starts a superframe.
    first_in_superframe: bool,
    /// Whether this frame reuses the band layout of the one before.
    reuse_band_params: bool,
    /// How many quantisation units the block codes.
    pub(crate) unit_count: usize,
    /// The first unit coded once for both channels of a stereo block.
    pub(crate) stereo_unit: usize,
    /// The unit band extension reaches to.
    extension_unit: usize,
    /// The last frame's coded units, which modes 2 and 3 code scale factors against.
    units_prev: usize,
    /// Each unit's allocation gradient.
    gradient: [i32; MAX_UNITS + 1],
    /// The gradient mode: 0 for an explicit range, 1-3 for a scaled one.
    gradient_mode: i32,
    /// The units the gradient runs between, and its values there.
    gradient_range: [i32; 4],
    /// How many leading units get one bit more.
    gradient_boundary: usize,
    /// Which channel of a stereo block codes the shared units.
    pub(crate) primary_channel: usize,
    /// Each shared unit's sign: whether the other channel is the negation of the primary.
    pub(crate) joint_signs: [bool; MAX_UNITS],
    /// Whether band extension is on.
    pub(crate) band_extension: bool,
    /// Whether this frame carries band-extension data.
    pub(crate) has_extension_data: bool,
}

impl Block {
    /// A block of `kind` with nothing decoded.
    pub(crate) fn new(kind: BlockKind) -> Self {
        Self {
            kind,
            channels: (0..kind.channels()).map(|_| Channel::new()).collect(),
            first_in_superframe: false,
            reuse_band_params: false,
            unit_count: 0,
            stereo_unit: 0,
            extension_unit: 0,
            units_prev: 0,
            gradient: [0; MAX_UNITS + 1],
            gradient_mode: 0,
            gradient_range: [0; 4],
            gradient_boundary: 0,
            primary_channel: 0,
            joint_signs: [false; MAX_UNITS],
            band_extension: false,
            has_extension_data: false,
        }
    }

    /// Whether this frame's block says it starts a superframe.
    pub(crate) fn first_in_superframe(&self) -> bool {
        self.first_in_superframe
    }

    /// Reads this block from `reader`, byte-aligning after it (`unpack.c` 52-67).
    pub(crate) fn unpack(
        &mut self,
        reader: &mut BitReader<'_>,
        high_rate: bool,
        rate_index: usize,
    ) -> Result<(), Error> {
        self.first_in_superframe = reader.read(1) == 0;
        self.reuse_band_params = reader.read(1) == 1;
        if self.kind == BlockKind::Lfe {
            self.unpack_lfe(reader);
        } else {
            if self.first_in_superframe && self.reuse_band_params {
                return Err(Error::ReusedBandParams);
            }
            self.unpack_standard(reader, high_rate, rate_index)?;
        }
        reader.align_to_byte();
        Ok(())
    }

    /// A mono or stereo block (`unpack.c` 82-110).
    fn unpack_standard(
        &mut self,
        reader: &mut BitReader<'_>,
        high_rate: bool,
        rate_index: usize,
    ) -> Result<(), Error> {
        if !self.reuse_band_params {
            self.read_band_params(reader, high_rate, rate_index)?;
        }
        self.read_gradient_params(reader)?;
        self.create_gradient();
        self.read_stereo_params(reader);
        self.read_extension_params(reader)?;
        for ch in 0..self.channels.len() {
            self.channels[ch].coded_units = if self.primary_channel == ch {
                self.unit_count
            } else {
                self.stereo_unit
            };
            self.read_scale_factors(ch, reader)?;
            let mask = self.precision_mask(ch);
            self.calculate_precisions(ch, &mask)?;
            let channel = &mut self.channels[ch];
            channel.codebook_set = codebook_sets(channel, high_rate);
            read_spectra(channel, reader, high_rate);
            read_spectra_fine(channel, reader);
        }
        self.units_prev = if self.band_extension {
            self.extension_unit
        } else {
            self.unit_count
        };
        Ok(())
    }

    /// The band layout: how many bands, the first shared one, and band extension's reach
    /// (`unpack.c` 112-160).
    fn read_band_params(
        &mut self,
        reader: &mut BitReader<'_>,
        high_rate: bool,
        rate_index: usize,
    ) -> Result<(), Error> {
        let min_bands = MIN_BAND_COUNT[usize::from(high_rate)];
        let band_count = reader.read(4) as usize + min_bands;
        if band_count > MAX_BAND_COUNT[rate_index] {
            return Err(Error::BandParams);
        }
        self.unit_count = BAND_TO_UNIT_COUNT[band_count];
        if self.kind == BlockKind::Stereo {
            let band = reader.read(4) as usize + min_bands;
            if band > band_count {
                return Err(Error::BandParams);
            }
            self.stereo_unit = BAND_TO_UNIT_COUNT[band];
        }
        self.band_extension = reader.read(1) == 1;
        self.extension_unit = if self.band_extension {
            let band = reader.read(4) as usize + min_bands;
            if band < band_count || band > MAX_EXTENSION_BAND[usize::from(high_rate)] {
                return Err(Error::BandParams);
            }
            BAND_TO_UNIT_COUNT[band]
        } else {
            self.unit_count
        };
        Ok(())
    }

    /// The bit-allocation gradient's shape (`unpack.c` 162-207).
    fn read_gradient_params(&mut self, reader: &mut BitReader<'_>) -> Result<(), Error> {
        self.gradient_mode = reader.read(2);
        self.gradient_range = if self.gradient_mode > 0 {
            let start_unit = reader.read(5);
            let start_value = reader.read(5);
            [start_unit, 31, start_value, 31]
        } else {
            let start_unit = reader.read(6);
            let end_unit = reader.read(6) + 1;
            let start_value = reader.read(5);
            let end_value = reader.read(5);
            [start_unit, end_unit, start_value, end_value]
        };
        self.gradient_boundary = reader.read(4) as usize;
        let [start_unit, end_unit, _, _] = self.gradient_range;
        if self.gradient_boundary > self.unit_count {
            return Err(Error::Gradient);
        }
        // Start values are five bits and end values five bits or 31, so only the units can be
        // out of range.
        if start_unit >= 48 || end_unit >= 48 || start_unit > end_unit {
            return Err(Error::Gradient);
        }
        Ok(())
    }

    /// Draws the gradient: flat at the start value up to the start unit, along the base curve
    /// to the end unit, flat at the end value after it (`bit_allocation.c` 8-46).
    fn create_gradient(&mut self) {
        let [start_unit, end_unit, start_value, end_value] = self.gradient_range;
        let (start_unit, end_unit) = (start_unit as usize, end_unit as usize);
        for (i, slot) in self
            .gradient
            .iter_mut()
            .enumerate()
            .take(self.unit_count + 1)
        {
            *slot = if i < end_unit { start_value } else { end_value };
        }
        let units = end_unit.saturating_sub(start_unit);
        let values = end_value - start_value;
        if units == 0 || values == 0 {
            return;
        }
        let curve = gradient_curve(units);
        let (scale, base, sign) = if values < 0 {
            (f64::from(-values - 1) / 31.0, start_value - 1, -1)
        } else {
            (f64::from(values - 1) / 31.0, start_value + 1, 1)
        };
        for i in start_unit..end_unit.min(self.gradient.len()) {
            self.gradient[i] = base + sign * (f64::from(curve[i - start_unit]) * scale) as i32;
        }
    }

    /// Which channel is primary, and the shared units' signs (`unpack.c` 209-228).
    fn read_stereo_params(&mut self, reader: &mut BitReader<'_>) {
        if self.kind != BlockKind::Stereo {
            return;
        }
        self.primary_channel = reader.read(1) as usize;
        let has_signs = reader.read(1) == 1;
        self.joint_signs = [false; MAX_UNITS];
        if has_signs {
            for sign in &mut self.joint_signs[self.stereo_unit..self.unit_count] {
                *sign = reader.read(1) == 1;
            }
        }
    }

    /// Band extension's header and values (`unpack.c` 230-292).
    fn read_extension_params(&mut self, reader: &mut BitReader<'_>) -> Result<(), Error> {
        let mut bex_band = 0;
        if self.band_extension {
            let group = self
                .unit_count
                .checked_sub(13)
                .and_then(|i| BEX_GROUPS.get(i));
            let Some(&(_, _, bands)) = group else {
                return Err(Error::ExtensionData);
            };
            bex_band = bands;
            if self.kind == BlockKind::Stereo {
                read_bex_header(&mut self.channels[1], reader, bex_band);
            } else {
                reader.skip(1);
            }
        }
        self.has_extension_data = reader.read(1) == 1;
        if !self.has_extension_data {
            return Ok(());
        }
        if !self.band_extension {
            // A mode and a length, and that many bits nothing reads.
            reader.skip(2);
            let length = reader.read(5) as usize;
            reader.skip(length);
            return Ok(());
        }
        read_bex_header(&mut self.channels[0], reader, bex_band);
        let length = reader.read(5) as usize;
        if length == 0 {
            return Ok(());
        }
        let end = reader.position() + length;
        for channel in &mut self.channels {
            let lengths = &BEX_DATA_LENGTHS[channel.bex_mode][bex_band];
            for (value, &bits) in channel
                .bex_values
                .iter_mut()
                .zip(lengths)
                .take(channel.bex_value_count)
            {
                *value = reader.read(bits) as usize;
            }
        }
        if reader.position() > end {
            return Err(Error::ExtensionData);
        }
        Ok(())
    }

    /// Channel `ch`'s scale factors, in the coding mode it names (`scale_factors.c` 12-68).
    fn read_scale_factors(&mut self, ch: usize, reader: &mut BitReader<'_>) -> Result<(), Error> {
        let mode = reader.read(2);
        let units = self.extension_unit;
        let first = self.first_in_superframe;
        let previous = self.channels[ch].scale_factors_prev;
        let primary = self.channels[0].scale_factors;
        let units_prev = self.units_prev;
        let sf = &mut self.channels[ch].scale_factors;
        *sf = [0; MAX_UNITS + 1];
        match (ch, mode) {
            (_, 0) => read_vlc_delta_offset(sf, units, reader),
            (0, 1) => read_clc_offset(sf, units, reader),
            (0, 2) | (_, 3) if first => return Err(Error::ScaleFactorMode),
            (0, 2) | (1.., 3) => read_vlc_distance(sf, units, &previous, units_prev, reader),
            (0, 3) => read_vlc_delta_with_baseline(sf, units, &previous, units_prev, reader),
            (_, 1) => read_vlc_distance(sf, units, &primary, units, reader),
            _ => read_vlc_delta_with_baseline(sf, units, &primary, units, reader),
        }
        if sf[..units].iter().any(|&f| !(0..=31).contains(&f)) {
            return Err(Error::ScaleFactorRange);
        }
        self.channels[ch].scale_factors_prev = *sf;
        Ok(())
    }

    /// How much each unit's precision rises for a steep step in scale factor beside it
    /// (`bit_allocation.c` 48-63).
    fn precision_mask(&self, ch: usize) -> [i32; MAX_UNITS] {
        let sf = &self.channels[ch].scale_factors;
        let mut mask = [0; MAX_UNITS];
        for i in 1..self.unit_count {
            let delta = sf[i] - sf[i - 1];
            if delta > 1 {
                mask[i] += (delta - 1).min(5);
            } else if delta < -1 {
                mask[i - 1] += (-delta - 1).min(5);
            }
        }
        mask
    }

    /// Each unit's precision, from its scale factor, the mask and the gradient
    /// (`bit_allocation.c` 65-121).
    fn calculate_precisions(&mut self, ch: usize, mask: &[i32; MAX_UNITS]) -> Result<(), Error> {
        let units = self.unit_count;
        let (mode, boundary, gradient) =
            (self.gradient_mode, self.gradient_boundary, self.gradient);
        let channel = &mut self.channels[ch];
        for i in 0..units {
            let mut precision = if mode == 0 {
                channel.scale_factors[i] - gradient[i]
            } else {
                let p = channel.scale_factors[i] + mask[i] - gradient[i];
                match (p > 0, mode) {
                    (true, 1) => p / 2,
                    (true, 2) => 3 * p / 8,
                    (true, _) => p / 4,
                    (false, _) => p,
                }
            };
            precision = precision.max(1);
            if i < boundary {
                precision += 1;
            }
            if precision > 30 {
                return Err(Error::PrecisionRange);
            }
            channel.precisions_fine[i] = (precision - 15).max(0);
            channel.precisions[i] = precision.min(15);
        }
        Ok(())
    }

    /// A low-frequency block: two units, five-bit scale factors, fixed precision
    /// (`unpack.c` 411-458).
    fn unpack_lfe(&mut self, reader: &mut BitReader<'_>) {
        self.unit_count = 2;
        let precision = if self.reuse_band_params { 8 } else { 4 };
        let channel = &mut self.channels[0];
        channel.scale_factors = [0; MAX_UNITS + 1];
        for sf in &mut channel.scale_factors[..2] {
            *sf = reader.read(5);
        }
        channel.coded_units = 2;
        channel.quantized = [0; MAX_FRAME_SAMPLES];
        for i in 0..2 {
            channel.precisions[i] = precision;
            channel.precisions_fine[i] = 0;
            for coefficient in UNIT_COEFF_INDEX[i]..UNIT_COEFF_INDEX[i + 1] {
                channel.quantized[coefficient] = reader.read_signed(precision as u32 + 1);
            }
        }
        channel.quantized_fine = [0; MAX_FRAME_SAMPLES];
    }
}

/// The gradient curve `units` units long: the base curve, resampled (`bit_allocation.c`
/// 130-141).
fn gradient_curve(units: usize) -> Vec<i32> {
    (0..units)
        .map(|i| BASE_CURVE[i * BASE_CURVE.len() / units])
        .collect()
}

/// A channel's band-extension mode and how many values it codes (`unpack.c` 230-235).
fn read_bex_header(channel: &mut Channel, reader: &mut BitReader<'_>, bex_band: usize) {
    let mode = reader.read(2) as usize;
    channel.bex_mode = if bex_band > 2 { mode } else { 4 };
    channel.bex_value_count = BEX_VALUE_COUNTS[channel.bex_mode][bex_band];
}

/// Mode 1 for the first channel: fixed-width offsets from a base (`scale_factors.c` 70-81).
fn read_clc_offset(sf: &mut [i32], units: usize, reader: &mut BitReader<'_>) {
    let width = reader.read(2) as u32 + 2;
    let base = if width < 5 { reader.read(5) } else { 0 };
    for f in &mut sf[..units] {
        *f = reader.read(width) + base;
    }
}

/// Mode 0: Huffman-coded deltas from the first, wrapped, offset by a base less a weight curve
/// (`scale_factors.c` 83-105).
fn read_vlc_delta_offset(sf: &mut [i32], units: usize, reader: &mut BitReader<'_>) {
    let weights = &SCALE_FACTOR_WEIGHTS[reader.read(3) as usize];
    let base = reader.read(5);
    let width = reader.read(2) as usize + 3;
    let Some(book) = codebooks().scale_unsigned[width].as_ref() else {
        return;
    };
    sf[0] = reader.read(width as u32);
    for i in 1..units {
        sf[i] = (sf[i - 1] + book.read(reader, false)) & (book.value_max - 1);
    }
    for (f, weight) in sf[..units].iter_mut().zip(weights) {
        *f += base - weight;
    }
}

/// Huffman-coded signed distances from a baseline, then five-bit values past it
/// (`scale_factors.c` 107-124).
fn read_vlc_distance(
    sf: &mut [i32],
    units: usize,
    baseline: &[i32],
    baseline_units: usize,
    reader: &mut BitReader<'_>,
) {
    let width = reader.read(2) as usize + 2;
    let Some(book) = codebooks().scale_signed[width].as_ref() else {
        return;
    };
    let shared = units.min(baseline_units);
    for i in 0..shared {
        sf[i] = (baseline[i] + book.read(reader, true)) & 31;
    }
    for f in &mut sf[shared..units] {
        *f = reader.read(5);
    }
}

/// Huffman-coded deltas added to a baseline and an offset, then five-bit values past it
/// (`scale_factors.c` 126-151).
fn read_vlc_delta_with_baseline(
    sf: &mut [i32],
    units: usize,
    baseline: &[i32],
    baseline_units: usize,
    reader: &mut BitReader<'_>,
) {
    let base = reader.read_offset_binary(5);
    let width = reader.read(2) as usize + 1;
    let Some(book) = codebooks().scale_unsigned[width].as_ref() else {
        return;
    };
    let shared = units.min(baseline_units);
    sf[0] = reader.read(width as u32);
    for i in 1..shared {
        sf[i] = (sf[i - 1] + book.read(reader, false)) & (book.value_max - 1);
    }
    for i in 0..shared {
        sf[i] += base + baseline[i];
    }
    for f in &mut sf[shared..units] {
        *f = reader.read(5);
    }
}

/// Which units use the second codebook set: those whose scale factor peaks above its
/// neighbours (`unpack.c` 306-354).
fn codebook_sets(channel: &Channel, high_rate: bool) -> [usize; MAX_UNITS] {
    let mut sets = [0; MAX_UNITS];
    let units = channel.coded_units;
    if units <= 1 || high_rate {
        return sets;
    }
    let mut sf = channel.scale_factors;
    sf[units] = sf[units - 1];
    let average = if units > 12 {
        (sf[..12].iter().sum::<i32>() + 6) / 12
    } else {
        0
    };
    for i in 8..units {
        let (prev, next) = (sf[i - 1], sf[i + 1]);
        if sf[i] - prev.min(next) >= 3 || sf[i] - prev + sf[i] - next >= 3 {
            sets[i] = 1;
        }
    }
    for i in 12..units {
        if sets[i] == 0 {
            let lowest = sf[i - 1].min(sf[i + 1]);
            let wide = i32::from(UNIT_COEFF_COUNT[i] == 16);
            if sf[i] - lowest >= 2 && sf[i] >= average - wide {
                sets[i] = 1;
            }
        }
    }
    sets
}

/// The coarse spectrum: Huffman-coded groups at low precisions, fixed-width values above
/// (`unpack.c` 356-388).
fn read_spectra(channel: &mut Channel, reader: &mut BitReader<'_>, high_rate: bool) {
    channel.quantized = [0; MAX_FRAME_SAMPLES];
    let max_huff = MAX_HUFF_PRECISION[usize::from(high_rate)];
    for i in 0..channel.coded_units {
        let (start, end) = (UNIT_COEFF_INDEX[i], UNIT_COEFF_INDEX[i + 1]);
        let precision = channel.precisions[i] + 1;
        let book = (precision <= max_huff)
            .then(|| {
                codebooks().spectrum[channel.codebook_set[i]][precision as usize]
                    [UNIT_CODEBOOK_INDEX[i]]
                    .as_ref()
            })
            .flatten();
        if let Some(book) = book {
            let groups = UNIT_COEFF_COUNT[i] >> book.value_count_power;
            let values: Vec<i32> = (0..groups).map(|_| book.read(reader, false)).collect();
            book.unpack(&values, &mut channel.quantized[start..end]);
        } else {
            for value in &mut channel.quantized[start..end] {
                *value = reader.read_signed(precision as u32);
            }
        }
    }
}

/// The fine spectrum, for units whose precision passed 15 bits (`unpack.c` 390-409).
fn read_spectra_fine(channel: &mut Channel, reader: &mut BitReader<'_>) {
    channel.quantized_fine = [0; MAX_FRAME_SAMPLES];
    for i in 0..channel.coded_units {
        let fine = channel.precisions_fine[i];
        if fine > 0 {
            for value in &mut channel.quantized_fine[UNIT_COEFF_INDEX[i]..UNIT_COEFF_INDEX[i + 1]] {
                *value = reader.read_signed(fine as u32 + 1);
            }
        }
    }
}
