//! Decoding frames this file encodes itself. The encoder writes the fields `unpack` reads in the
//! order LibAtrac9 reads them; what a frame decodes to is checked against the transform's
//! textbook definitions, computed here directly, not against the decoder's own arithmetic.

#![allow(
    clippy::cast_precision_loss,
    clippy::similar_names,
    clippy::many_single_char_names,
    reason = "transform indices are at most 512, and the formulas keep their textbook names"
)]

use crate::huffman::codebooks;
use crate::tables::{QUANTIZER_FINE_STEP, QUANTIZER_STEP, UNIT_COEFF_INDEX};
use crate::{Config, Decoder, Error, to_i16};
use std::f64::consts::PI;

/// Writes bits most significant first.
#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    bits: usize,
}

impl BitWriter {
    fn put(&mut self, value: i64, width: u32) {
        for i in (0..width).rev() {
            if self.bits % 8 == 0 {
                self.bytes.push(0);
            }
            if (value >> i) & 1 == 1 {
                let last = self.bytes.len() - 1;
                self.bytes[last] |= 0x80 >> (self.bits % 8);
            }
            self.bits += 1;
        }
    }

    /// The bytes, padded with zeros to `size`.
    fn finish(mut self, size: usize) -> Vec<u8> {
        assert!(self.bytes.len() <= size, "frame overflows its size");
        self.bytes.resize(size, 0);
        self.bytes
    }
}

/// A deterministic sequence for test values.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, range: i32) -> i32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        ((self.0 >> 33) % (2 * range as u64 + 1)) as i32 - range
    }
}

/// A configuration word: 48 kHz, `layout`, frames of `frame_bytes`, `2^superframe` per
/// superframe.
fn word(layout: u8, frame_bytes: u16, superframe: u8) -> [u8; 4] {
    let mut w = BitWriter::default();
    w.put(0xfe, 8);
    w.put(7, 4);
    w.put(i64::from(layout), 3);
    w.put(0, 1);
    w.put(i64::from(frame_bytes - 1), 11);
    w.put(i64::from(superframe), 2);
    w.finish(4).try_into().expect("four bytes")
}

/// What one unit of a channel codes: its scale factor and quantised values.
#[derive(Clone)]
struct Unit {
    sf: i32,
    coarse: Vec<i32>,
    fine: Vec<i32>,
}

/// A coded channel: units with gradient value `gradient` everywhere.
struct ChannelCode {
    units: Vec<Unit>,
}

/// The precision a unit with scale factor `sf` gets under a flat gradient of `g`, split into
/// coarse and fine as `calculate_precisions` does.
fn precisions(sf: i32, g: i32) -> (i32, i32) {
    let p = (sf - g).max(1);
    (p.min(15), (p - 15).max(0))
}

/// Writes one mono or stereo block's header for `bands` bands (`stereo_band` for a stereo
/// block's shared start), no band extension, a flat gradient of `g`, and no extension data.
fn write_header(
    w: &mut BitWriter,
    first: bool,
    bands: i64,
    stereo: Option<(i64, usize, &[bool])>,
    g: i64,
) {
    w.put(i64::from(!first), 1);
    w.put(0, 1);
    w.put(bands - 3, 4);
    if let Some((stereo_band, _, _)) = stereo {
        w.put(stereo_band - 3, 4);
    }
    w.put(0, 1);
    w.put(0, 2);
    w.put(0, 6);
    w.put(0, 6);
    w.put(g, 5);
    w.put(g, 5);
    w.put(0, 4);
    if let Some((_, primary, signs)) = stereo {
        w.put(primary as i64, 1);
        w.put(1, 1);
        for &sign in signs {
            w.put(i64::from(sign), 1);
        }
    }
    w.put(0, 1);
}

/// Writes a channel's coded spectrum for its first `coded` units, Huffman-coded where the
/// precision allows, as `read_spectra` and `read_spectra_fine` read it.
fn write_spectra(w: &mut BitWriter, channel: &ChannelCode, coded: usize, g: i32) {
    let books = codebooks();
    for (i, unit) in channel.units.iter().take(coded).enumerate() {
        let (coarse, _) = precisions(unit.sf, g);
        let p = coarse + 1;
        if p <= 7 {
            let book = books.spectrum[0][p as usize][crate::tables::UNIT_CODEBOOK_INDEX[i]]
                .as_ref()
                .expect("a codebook for every precision");
            for group in unit.coarse.chunks(book.value_count) {
                let mask = (1 << book.value_bits) - 1;
                let packed = group.iter().enumerate().fold(0, |acc, (k, &v)| {
                    acc | (v & mask) << (k as u32 * book.value_bits)
                });
                let length = book.bits[packed as usize];
                assert_ne!(length, 0, "a value with no code");
                w.put(i64::from(book.codes[packed as usize]), u32::from(length));
            }
        } else {
            for &v in &unit.coarse {
                w.put(i64::from(v), p as u32);
            }
        }
    }
    for unit in channel.units.iter().take(coded) {
        let (_, fine) = precisions(unit.sf, g);
        if fine > 0 {
            for &v in &unit.fine {
                w.put(i64::from(v), fine as u32 + 1);
            }
        }
    }
}

/// Writes scale factors in mode 1, five bits each, as the first channel may.
fn write_sf_fixed(w: &mut BitWriter, units: &[Unit]) {
    w.put(1, 2);
    w.put(3, 2);
    for unit in units {
        w.put(i64::from(unit.sf), 5);
    }
}

/// Each unit's dequantised, scaled coefficients, as the format defines them.
fn dequantized(channel: &ChannelCode, coded: usize, g: i32) -> [f64; 256] {
    let mut out = [0.0; 256];
    for (i, unit) in channel.units.iter().enumerate().take(coded) {
        let (coarse, fine) = precisions(unit.sf, g);
        let scale = 2.0_f64.powi(unit.sf - 15);
        for (k, (&c, &f)) in unit.coarse.iter().zip(&unit.fine).enumerate() {
            out[UNIT_COEFF_INDEX[i] + k] = (f64::from(c) * QUANTIZER_STEP[coarse as usize]
                + f64::from(f) * QUANTIZER_FINE_STEP[fine as usize])
                * scale;
        }
    }
    out
}

/// The type-IV DCT by its definition.
fn dct4(x: &[f64]) -> Vec<f64> {
    let n = x.len() as f64;
    (0..x.len())
        .map(|k| {
            x.iter()
                .enumerate()
                .map(|(i, v)| v * (PI / n * (i as f64 + 0.5) * (k as f64 + 0.5)).cos())
                .sum()
        })
        .collect()
}

/// The analysis window: a raised sine rising across one frame.
fn analysis_window(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| (((i as f64 + 0.5) / n as f64 - 0.5) * PI).sin() * 0.5 + 0.5)
        .collect()
}

/// The spectrum a frame codes for `current` and the block after it: the two blocks windowed and
/// folded into one, then transformed. `2/N` scales the transform to its own inverse.
fn forward(current: &[f64], next: &[f64]) -> Vec<f64> {
    let n = current.len();
    let h = n / 2;
    let a = analysis_window(n);
    let mut folded = vec![0.0; n];
    for i in 0..h {
        folded[i + h] = a[i] * current[i] - a[n - 1 - i] * current[n - 1 - i];
        folded[h - 1 - i] = -a[n - 1 - i] * next[i] - a[i] * next[n - 1 - i];
    }
    dct4(&folded).iter().map(|v| v * 2.0 / n as f64).collect()
}

/// Synthesis by definition: the spectra's DCT-IV, windowed by the analysis window over the sum
/// of its squares at mirrored positions, overlapped frame to frame.
fn synthesize(spectra: &[[f64; 256]]) -> Vec<f64> {
    let n = 256;
    let h = n / 2;
    let a = analysis_window(n);
    let s: Vec<f64> = (0..n)
        .map(|i| a[i] / (a[n - 1 - i].powi(2) + a[i].powi(2)))
        .collect();
    let mut previous = vec![0.0; n];
    let mut out = Vec::new();
    for spectrum in spectra {
        let d = dct4(spectrum);
        let mut frame = vec![0.0; n];
        for i in 0..h {
            frame[i] = s[i] * d[i + h] + previous[i];
            frame[i + h] = -s[i + h] * d[n - 1 - i] - previous[i + h];
        }
        for i in 0..h {
            previous[i] = -s[n - 1 - i] * d[h - i - 1];
            previous[i + h] = s[h - i - 1] * d[i];
        }
        out.extend(frame);
    }
    out
}

/// Quantises `spectrum` into 30 units at the precision a flat gradient of `g` gives: each unit's
/// scale factor is the smallest that holds its largest value.
fn quantize(spectrum: &[f64], g: i32) -> ChannelCode {
    let units = (0..30)
        .map(|i| {
            let range = &spectrum[UNIT_COEFF_INDEX[i]..UNIT_COEFF_INDEX[i + 1]];
            let peak = range.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
            let sf = if peak > 0.0 {
                (15 + peak.log2().ceil() as i32).clamp(0, 30)
            } else {
                0
            };
            assert!(
                peak <= 2.0_f64.powi(sf - 15),
                "the signal fits the scale factors"
            );
            let (coarse_bits, fine_bits) = precisions(sf, g);
            let scale = 2.0_f64.powi(sf - 15);
            let limit = (1 << coarse_bits) - 1;
            let fine_limit = (1 << fine_bits) - 1;
            let mut coarse = Vec::new();
            let mut fine = Vec::new();
            for v in range {
                let x = v / scale;
                let c = ((x / QUANTIZER_STEP[coarse_bits as usize]).round() as i32)
                    .clamp(-limit, limit);
                let rest = x - f64::from(c) * QUANTIZER_STEP[coarse_bits as usize];
                let f = if fine_bits > 0 {
                    ((rest / QUANTIZER_FINE_STEP[fine_bits as usize]).round() as i32)
                        .clamp(-fine_limit, fine_limit)
                } else {
                    0
                };
                coarse.push(c);
                fine.push(f);
            }
            Unit { sf, coarse, fine }
        })
        .collect();
    ChannelCode { units }
}

/// A two-tone signal, `blocks` frames of 256 samples.
fn signal(blocks: usize) -> Vec<Vec<f64>> {
    (0..blocks)
        .map(|b| {
            (0..256)
                .map(|i| {
                    let t = (b * 256 + i) as f64 / 48_000.0;
                    12_000.0 * (2.0 * PI * 440.0 * t).sin()
                        + 4_000.0 * (2.0 * PI * 3_150.0 * t).sin()
                })
                .collect()
        })
        .collect()
}

/// A configuration word names its rate, channels, frame and superframe sizes.
#[test]
fn a_configuration_word_describes_its_stream() {
    let config = Config::parse(word(2, 384, 2)).expect("a valid word");
    assert_eq!(config.word(), [0xfe, 0x74, 0x2f, 0xf0]);
    assert_eq!(config.sample_rate(), 48_000);
    assert_eq!(config.channels(), 2);
    assert_eq!(config.frame_samples(), 256);
    assert_eq!(config.frames_per_superframe(), 4);
    assert_eq!(config.superframe_bytes(), 1536);
    assert_eq!(config.superframe_samples(), 1024);
    assert_eq!(
        Config::parse([0xff, 0x74, 0x2f, 0xf0]),
        Err(Error::BadConfig)
    );
    assert_eq!(
        Config::parse([0xfe, 0x75, 0x2f, 0xf0]),
        Err(Error::BadConfig),
        "reserved bit"
    );
    assert_eq!(
        Config::parse([0xfe, 0x7c, 0x2f, 0xf0]),
        Err(Error::BadConfig),
        "layout 6"
    );
}

/// The decoder's transform is the DCT-IV of its definition.
#[test]
fn the_fast_transform_is_the_dct4() {
    let transform = crate::synthesis::Transform::new(8);
    let mut rng = Lcg(7);
    let mut input = [0.0; 256];
    for v in &mut input {
        *v = f64::from(rng.next(1000)) / 1000.0;
    }
    let direct = dct4(&input);
    let fast = transform.dct4(&input);
    for (a, b) in fast.iter().zip(&direct) {
        assert!((a - b).abs() < 1e-9, "{a} against {b}");
    }
}

/// Stream A: the two-tone signal, transformed and quantised under a flat gradient of 4, as four
/// mono 48 kHz frames, two to a 2048-byte superframe. A superframe's frames follow each other,
/// each ending on a byte, and the superframe is padded to its size. Returns the configuration
/// word, the stream, and the signal's blocks; frame `t` codes blocks `t` and `t + 1`.
fn two_tone_stream() -> ([u8; 4], Vec<u8>, Vec<Vec<f64>>) {
    let g = 4;
    let blocks = signal(5);
    let mut stream = Vec::new();
    for superframe in 0..2 {
        let mut w = BitWriter::default();
        for t in [2 * superframe, 2 * superframe + 1] {
            let code = quantize(&forward(&blocks[t], &blocks[t + 1]), g);
            write_header(&mut w, t % 2 == 0, 18, None, i64::from(g));
            write_sf_fixed(&mut w, &code.units);
            write_spectra(&mut w, &code, 30, g);
            w.bits = w.bits.next_multiple_of(8);
        }
        stream.extend(w.finish(2048));
    }
    (word(0, 1024, 1), stream, blocks)
}

/// A two-tone signal, transformed, quantised and written as mono frames two to a superframe,
/// decodes back to itself one frame late: every 16-bit sample within one step.
#[test]
fn finely_quantised_frames_decode_to_their_signal() {
    let (config, stream, blocks) = two_tone_stream();
    let mut decoder = Decoder::new(Config::parse(config).expect("a valid word"));
    let mut pcm = Vec::new();
    for superframe in stream.chunks(2048) {
        let used = decoder
            .decode_superframe(superframe, &mut pcm)
            .expect("the superframe decodes");
        assert!(used <= 2048);
    }
    assert_eq!(pcm.len(), 4 * 256);
    for t in 1..4 {
        for i in 0..256 {
            let got = i32::from(to_i16(pcm[t * 256 + i]));
            let want = i32::from(to_i16(blocks[t][i]));
            assert!(
                (got - want).abs() <= 1,
                "block {t} sample {i}: {got} against {want}"
            );
        }
    }
}

/// Stream B: three mono 48 kHz frames of 1024 bytes, one to a superframe, whose coarse values are
/// random at precisions 2 to 7 and so Huffman-coded throughout. Returns the configuration word,
/// the stream, and each frame's code.
fn huffman_stream() -> ([u8; 4], Vec<u8>, Vec<ChannelCode>) {
    let mut rng = Lcg(3);
    let g = 13;
    let books = codebooks();
    let mut stream = Vec::new();
    let mut codes = Vec::new();
    for _ in 0..3 {
        let units: Vec<Unit> = (0..30)
            .map(|i| {
                let sf = 19 - i as i32 / 5;
                let (coarse, _) = precisions(sf, g);
                let p = coarse as usize + 1;
                let book = books.spectrum[0][p][crate::tables::UNIT_CODEBOOK_INDEX[i]]
                    .as_ref()
                    .expect("a codebook");
                let count = UNIT_COEFF_INDEX[i + 1] - UNIT_COEFF_INDEX[i];
                let range = (1 << (book.value_bits - 1)) - 1;
                let mut coarse_values: Vec<i32> = (0..count).map(|_| rng.next(range)).collect();
                for group in coarse_values.chunks_mut(book.value_count) {
                    let mask = (1 << book.value_bits) - 1;
                    let packed = group.iter().enumerate().fold(0, |acc, (k, &v)| {
                        acc | (v & mask) << (k as u32 * book.value_bits)
                    });
                    if book.bits[packed as usize] == 0 {
                        group.fill(0);
                    }
                }
                Unit {
                    sf,
                    coarse: coarse_values,
                    fine: vec![0; count],
                }
            })
            .collect();
        let code = ChannelCode { units };
        let mut w = BitWriter::default();
        write_header(&mut w, true, 18, None, i64::from(g));
        write_sf_fixed(&mut w, &code.units);
        write_spectra(&mut w, &code, 30, g);
        stream.extend(w.finish(1024));
        codes.push(code);
    }
    (word(0, 1024, 0), stream, codes)
}

/// Coarse values at precisions 2 to 7 are Huffman-coded; the frame decodes to exactly what the
/// textbook synthesis makes of those values, over three frames of overlap.
#[test]
fn huffman_coded_spectra_decode_to_their_synthesis() {
    let (config, stream, codes) = huffman_stream();
    let mut decoder = Decoder::new(Config::parse(config).expect("a valid word"));
    let mut pcm = Vec::new();
    let mut spectra = Vec::new();
    for (frame, code) in stream.chunks(1024).zip(&codes) {
        decoder
            .decode_frame(frame, &mut pcm)
            .expect("the frame decodes");
        let decoded = &decoder.blocks[0].channels[0].quantized;
        let written: Vec<i32> = code.units.iter().flat_map(|u| u.coarse.clone()).collect();
        assert_eq!(&decoded[..], &written[..], "the coarse values read back");
        spectra.push(dequantized(code, 30, 13));
    }
    let want = synthesize(&spectra);
    for (i, (got, want)) in pcm.iter().zip(&want).enumerate() {
        assert!(
            (got - want).abs() < 1e-6,
            "sample {i}: {got} against {want}"
        );
    }
}

/// In a stereo block the second channel's shared units are the primary's, negated where the
/// sign is set, each then scaled by the second channel's own scale factors.
#[test]
fn shared_units_copy_the_primary_channel() {
    let mut rng = Lcg(11);
    let (g, stereo_band) = (0, 9);
    let stereo_unit = crate::tables::BAND_TO_UNIT_COUNT[stereo_band as usize];
    let signs: Vec<bool> = (stereo_unit..30).map(|i| i % 3 == 0).collect();
    let make = |rng: &mut Lcg, sf: i32| -> ChannelCode {
        ChannelCode {
            units: (0..30)
                .map(|i| {
                    let count = UNIT_COEFF_INDEX[i + 1] - UNIT_COEFF_INDEX[i];
                    Unit {
                        sf,
                        coarse: (0..count).map(|_| rng.next(500)).collect(),
                        fine: vec![0; count],
                    }
                })
                .collect(),
        }
    };
    let primary = make(&mut rng, 12);
    let second = make(&mut rng, 10);
    let mut w = BitWriter::default();
    write_header(&mut w, true, 18, Some((stereo_band, 0, &signs)), g);
    write_sf_fixed(&mut w, &primary.units);
    write_spectra(&mut w, &primary, 30, g as i32);
    // The second channel's scale factors in mode 0: weight set 0, base 10 plus each unit's
    // weight, coded as five-bit deltas from the first.
    let weights = crate::tables::SCALE_FACTOR_WEIGHTS[0];
    w.put(0, 2);
    w.put(0, 3);
    w.put(10, 5);
    w.put(2, 2);
    let book = codebooks().scale_unsigned[5]
        .as_ref()
        .expect("five-bit deltas");
    w.put(i64::from(weights[0]), 5);
    for i in 1..30 {
        let delta = (weights[i] - weights[i - 1]).rem_euclid(32) as usize;
        w.put(i64::from(book.codes[delta]), u32::from(book.bits[delta]));
    }
    write_spectra(&mut w, &second, stereo_unit, g as i32);
    let mut decoder = Decoder::new(Config::parse(word(2, 2048, 0)).expect("a valid word"));
    let mut pcm = Vec::new();
    decoder
        .decode_frame(&w.finish(2048), &mut pcm)
        .expect("the frame decodes");
    assert_eq!(pcm.len(), 512, "two channels interleaved");
    let block = &decoder.blocks[0];
    assert_eq!(block.channels[1].scale_factors[..30], [10; 30]);
    let own = dequantized(&second, stereo_unit, g as i32);
    let source = dequantized(&primary, 30, g as i32);
    for i in 0..30 {
        for k in UNIT_COEFF_INDEX[i]..UNIT_COEFF_INDEX[i + 1] {
            let want = if i < stereo_unit {
                own[k]
            } else {
                let sign = if signs[i - stereo_unit] { -1.0 } else { 1.0 };
                sign * source[k] * 2.0_f64.powi(10 - 12)
            };
            let got = block.channels[1].spectra[k];
            assert!(
                (got - want).abs() < 1e-9,
                "unit {i} coefficient {k}: {got} against {want}"
            );
        }
    }
}

/// Band extension fills the bands above the coded ones with mirrored copies of those below and,
/// in mode 4, scales its three groups by fixed fractions of a coded multiplier.
#[test]
fn band_extension_mirrors_and_scales_the_upper_bands() {
    let mut rng = Lcg(5);
    let units: Vec<Unit> = (0..30)
        .map(|i| {
            let count = UNIT_COEFF_INDEX[i + 1] - UNIT_COEFF_INDEX[i];
            Unit {
                sf: 14,
                coarse: (0..count).map(|_| rng.next(300)).collect(),
                fine: vec![0; count],
            }
        })
        .collect();
    let code = ChannelCode { units };
    let mut w = BitWriter::default();
    w.put(0, 1);
    w.put(0, 1);
    w.put(5 - 3, 4);
    w.put(1, 1);
    w.put(18 - 3, 4);
    w.put(0, 2);
    w.put(0, 6);
    w.put(0, 6);
    w.put(0, 5);
    w.put(0, 5);
    w.put(0, 4);
    w.put(0, 1);
    w.put(1, 1);
    w.put(0, 2);
    w.put(3, 5);
    w.put(6, 3);
    write_sf_fixed(&mut w, &code.units);
    write_spectra(&mut w, &code, 13, 0);
    let mut decoder = Decoder::new(Config::parse(word(0, 2048, 0)).expect("a valid word"));
    let mut pcm = Vec::new();
    decoder
        .decode_frame(&w.finish(2048), &mut pcm)
        .expect("the frame decodes");
    let mut want = dequantized(&code, 13, 0).to_vec();
    for (start, end) in [(40, 64), (64, 112), (112, 128)] {
        for i in 0..end - start {
            want[start + i] = want[start - i - 1];
        }
    }
    let multiplier = crate::tables::BEX_MODE4_MULTIPLIER[6];
    for (range, fraction) in [
        (40..64, 0.707_946_8),
        (64..112, 0.501_190_2),
        (112..128, 0.354_827_9),
    ] {
        for k in range {
            want[k] *= fraction * multiplier;
        }
    }
    let got = &decoder.blocks[0].channels[0].spectra;
    for k in 0..256 {
        assert!(
            (got[k] - want[k]).abs() < 1e-9,
            "coefficient {k}: {} against {}",
            got[k],
            want[k]
        );
    }
}

/// A frame that says it starts a superframe in the middle of one, a first frame reusing band
/// parameters, a first frame coding scale factors against a previous one, a precision past the
/// format's, and a frame cut short are each refused by name, and nothing is appended.
#[test]
fn malformed_frames_are_refused_by_name() {
    // Ten units of silence at precision 10: 36 header bits, 54 of scale factors and 24
    // coefficients of 11 bits, 354 bits in all.
    let silent = |first: bool, sf: i32| {
        let units: Vec<Unit> = (0..10)
            .map(|i| {
                let count = UNIT_COEFF_INDEX[i + 1] - UNIT_COEFF_INDEX[i];
                Unit {
                    sf,
                    coarse: vec![0; count],
                    fine: vec![0; count],
                }
            })
            .collect();
        let code = ChannelCode { units };
        let mut w = BitWriter::default();
        write_header(&mut w, first, 3, None, 0);
        write_sf_fixed(&mut w, &code.units);
        write_spectra(&mut w, &code, 10, 0);
        w.finish(64)
    };
    let silent = |first: bool| silent(first, 10);
    let mut pcm = Vec::new();
    let mut decoder = Decoder::new(Config::parse(word(0, 64, 1)).expect("a valid word"));
    assert_eq!(decoder.decode_frame(&silent(true), &mut pcm), Ok(45));
    assert_eq!(
        decoder.decode_frame(&silent(true), &mut pcm),
        Err(Error::SuperframeFlag)
    );

    let mut w = BitWriter::default();
    write_header(&mut w, true, 3, None, 0);
    write_sf_fixed(
        &mut w,
        &[Unit {
            sf: 31,
            coarse: vec![],
            fine: vec![],
        }],
    );
    let mut decoder = Decoder::new(Config::parse(word(0, 64, 0)).expect("a valid word"));
    assert_eq!(
        decoder.decode_frame(&w.finish(64), &mut pcm),
        Err(Error::PrecisionRange)
    );

    let mut decoder = Decoder::new(Config::parse(word(0, 64, 1)).expect("a valid word"));
    let mut reuse = silent(true);
    reuse[0] |= 0x40;
    assert_eq!(
        decoder.decode_frame(&reuse, &mut pcm),
        Err(Error::ReusedBandParams)
    );

    let mut w = BitWriter::default();
    write_header(&mut w, true, 3, None, 0);
    w.put(2, 2);
    let mut decoder = Decoder::new(Config::parse(word(0, 64, 0)).expect("a valid word"));
    assert_eq!(
        decoder.decode_frame(&w.finish(64), &mut pcm),
        Err(Error::ScaleFactorMode)
    );

    let mut decoder = Decoder::new(Config::parse(word(0, 64, 0)).expect("a valid word"));
    assert_eq!(
        decoder.decode_frame(&silent(true)[..40], &mut pcm),
        Err(Error::Truncated)
    );
    assert_eq!(pcm.len(), 256, "only the one frame that decoded");
}

/// Prints streams A and B for a hardware decode to check this decoder against: each stream's
/// configuration word and bytes, and the 16-bit samples decoded here, as hex. Run with
/// `cargo test -p orbistoun-atrac9 -- --ignored --nocapture print_console_fixtures`.
#[test]
#[ignore = "prints the fixtures a hardware request ships; it checks nothing"]
fn print_console_fixtures() {
    let hex = |bytes: &[u8]| {
        bytes.iter().fold(String::new(), |mut out, b| {
            use std::fmt::Write as _;
            let _ = write!(out, "{b:02x}");
            out
        })
    };
    let streams = [
        ("a", two_tone_stream().0, two_tone_stream().1, 2048),
        ("b", huffman_stream().0, huffman_stream().1, 1024),
    ];
    for (name, config, stream, superframe) in streams {
        let mut decoder = Decoder::new(Config::parse(config).expect("a valid word"));
        let mut pcm = Vec::new();
        for chunk in stream.chunks(superframe) {
            decoder
                .decode_superframe(chunk, &mut pcm)
                .expect("the superframe decodes");
        }
        let samples: Vec<u8> = pcm.iter().flat_map(|&s| to_i16(s).to_le_bytes()).collect();
        println!("stream-{name}-config {}", hex(&config));
        println!("stream-{name}-bytes {}", hex(&stream));
        println!("stream-{name}-pcm16 {}", hex(&samples));
    }
}
