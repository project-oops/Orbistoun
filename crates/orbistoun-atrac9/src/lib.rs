//! ATRAC9 decoding: the transform codec the platform's audio codec engine decodes, and most of
//! its first-party titles ship their sound in.
//!
//! A pure decoder with no knowledge of the guest. A stream is described by a four-byte
//! configuration word - sample rate, channel layout, frame size and frames per superframe - and
//! is a run of superframes, each a fixed number of bytes holding one to eight frames; each frame
//! decodes to 64, 128 or 256 samples per channel.
//!
//! The format is as LibAtrac9 (MIT, ACKNOWLEDGEMENTS.md) reads it; each function cites the file
//! and lines it follows. Decoded samples are LibAtrac9's to the last bit: every step is the same
//! double-precision arithmetic in the same order.

mod bits;
mod codes;
mod huffman;
mod synthesis;
mod tables;
mod unpack;

use bits::BitReader;
use synthesis::{Transform, synthesize};
use tables::{CHANNEL_CONFIGS, FRAME_SAMPLES_POWER, SAMPLE_RATES};
use unpack::Block;

/// Why a configuration word or a frame was refused. Each names the field that is out of range,
/// as LibAtrac9's status codes do (`C/src/error_codes.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The configuration word does not begin `0xfe`, sets its reserved bit, or names a channel
    /// layout past the six the format has.
    BadConfig,
    /// A frame says it starts a superframe in the middle of one.
    SuperframeFlag,
    /// A frame starting a superframe asks to reuse band parameters from the frame before.
    ReusedBandParams,
    /// The band count, shared band or extension band is out of range.
    BandParams,
    /// The bit-allocation gradient's units or boundary are out of range.
    Gradient,
    /// A scale-factor coding mode that needs a previous frame, in a superframe's first.
    ScaleFactorMode,
    /// A decoded scale factor outside 0 to 31.
    ScaleFactorRange,
    /// A unit's precision past the 15 coarse and 15 fine bits the format has steps for. The
    /// reference reads past its fine step table here; this decoder refuses the frame.
    PrecisionRange,
    /// Band-extension data longer than its stated length, or band extension on a block coding
    /// too few units to have it.
    ExtensionData,
    /// The frame's bits run past the end of the data it was given.
    Truncated,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self {
            Self::BadConfig => "configuration word is not ATRAC9's",
            Self::SuperframeFlag => "frame starts a superframe in the middle of one",
            Self::ReusedBandParams => "superframe's first frame reuses band parameters",
            Self::BandParams => "band parameters out of range",
            Self::Gradient => "bit-allocation gradient out of range",
            Self::ScaleFactorMode => "scale-factor mode needs a previous frame",
            Self::ScaleFactorRange => "scale factor out of range",
            Self::PrecisionRange => "precision past the format's",
            Self::ExtensionData => "band-extension data out of range",
            Self::Truncated => "frame runs past the end of its data",
        };
        write!(f, "ATRAC9: {what}")
    }
}

impl std::error::Error for Error {}

/// A stream's configuration, from its four-byte configuration word (LibAtrac9
/// `C/src/decinit.c` 39-76).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// The word itself.
    word: [u8; 4],
    /// Index into the sample rates.
    rate_index: usize,
    /// Index into the channel layouts.
    layout_index: usize,
    /// Bytes per frame.
    frame_bytes: usize,
    /// `log2` of frames per superframe.
    superframe_index: u32,
}

impl Config {
    /// The configuration a word describes: `0xfe`, then four bits of sample rate, three of
    /// channel layout, a zero bit, eleven of frame size less one and two of frames per
    /// superframe, most significant first.
    pub fn parse(word: [u8; 4]) -> Result<Self, Error> {
        let mut reader = BitReader::new(&word);
        let header = reader.read(8);
        let rate_index = reader.read(4) as usize;
        let layout_index = reader.read(3) as usize;
        let reserved = reader.read(1);
        let frame_bytes = reader.read(11) as usize + 1;
        let superframe_index = reader.read(2) as u32;
        if header != 0xfe || reserved != 0 || layout_index >= CHANNEL_CONFIGS.len() {
            return Err(Error::BadConfig);
        }
        Ok(Self {
            word,
            rate_index,
            layout_index,
            frame_bytes,
            superframe_index,
        })
    }

    /// The configuration word.
    pub fn word(&self) -> [u8; 4] {
        self.word
    }

    /// Samples per second.
    pub fn sample_rate(&self) -> u32 {
        SAMPLE_RATES[self.rate_index]
    }

    /// Channels, interleaved in the decoded output in the order the layout's blocks code them.
    pub fn channels(&self) -> usize {
        CHANNEL_CONFIGS[self.layout_index]
            .iter()
            .map(|kind| kind.channels())
            .sum()
    }

    /// Samples per channel in one frame.
    pub fn frame_samples(&self) -> usize {
        1 << FRAME_SAMPLES_POWER[self.rate_index]
    }

    /// Frames per superframe: 1, 2, 4 or 8.
    pub fn frames_per_superframe(&self) -> usize {
        1 << self.superframe_index
    }

    /// Bytes per superframe.
    pub fn superframe_bytes(&self) -> usize {
        self.frame_bytes << self.superframe_index
    }

    /// Samples per channel in one superframe.
    pub fn superframe_samples(&self) -> usize {
        self.frame_samples() * self.frames_per_superframe()
    }

    /// Whether the rate is one of the high ones (index 8 and up), which code fewer bands and no
    /// Huffman spectra.
    fn high_rate(&self) -> bool {
        self.rate_index > 7
    }
}

/// A decoder for one stream, holding what each frame carries over to the next: overlapped
/// samples, previous scale factors and band layout, and where in its superframe it is.
#[derive(Debug)]
pub struct Decoder {
    /// The stream's configuration.
    config: Config,
    /// The layout's blocks.
    blocks: Vec<Block>,
    /// The inverse transform for this frame length.
    transform: Transform,
    /// Which frame of the current superframe comes next.
    index_in_superframe: usize,
}

impl Decoder {
    /// A decoder at the start of a stream described by `config`.
    pub fn new(config: Config) -> Self {
        Self {
            blocks: CHANNEL_CONFIGS[config.layout_index]
                .iter()
                .map(|&kind| Block::new(kind))
                .collect(),
            transform: Transform::new(FRAME_SAMPLES_POWER[config.rate_index]),
            config,
            index_in_superframe: 0,
        }
    }

    /// The stream's configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Decodes the frame at the start of `input`, appending its samples to `out` interleaved,
    /// and answers how many bytes it used (LibAtrac9 `C/src/decoder.c` 16-60, `unpack.c`
    /// 28-50). On an error nothing is appended, and the decoder's state is whatever the frame
    /// left it in, as the reference's is.
    pub fn decode_frame(&mut self, input: &[u8], out: &mut Vec<f64>) -> Result<usize, Error> {
        let mut reader = BitReader::new(input);
        let high_rate = self.config.high_rate();
        for block in &mut self.blocks {
            block.unpack(&mut reader, high_rate, self.config.rate_index)?;
            if block.first_in_superframe() && self.index_in_superframe != 0 {
                return Err(Error::SuperframeFlag);
            }
        }
        if reader.overran() {
            return Err(Error::Truncated);
        }
        self.index_in_superframe =
            (self.index_in_superframe + 1) % self.config.frames_per_superframe();
        for block in &mut self.blocks {
            synthesize(block, &self.transform);
        }
        let channels: Vec<&[f64]> = self
            .blocks
            .iter()
            .flat_map(|block| &block.channels)
            .map(|channel| &channel.pcm[..])
            .collect();
        for sample in 0..self.config.frame_samples() {
            out.extend(channels.iter().map(|pcm| pcm[sample]));
        }
        Ok(reader.position() / 8)
    }

    /// Decodes every frame of the superframe at the start of `input`, appending its samples to
    /// `out` interleaved, and answers how many bytes the frames used. A superframe's frames follow
    /// one another, each starting on the byte after the last ends; padding, if any, is at the
    /// superframe's end (LibAtrac9 `CSharp/LibAtrac9/Atrac9Decoder.cs`, `DecodeSuperFrame`).
    pub fn decode_superframe(&mut self, input: &[u8], out: &mut Vec<f64>) -> Result<usize, Error> {
        let mut used = 0;
        for _ in 0..self.config.frames_per_superframe() {
            used += self.decode_frame(input.get(used..).unwrap_or_default(), out)?;
        }
        Ok(used)
    }
}

/// A decoded sample as a 16-bit one: rounded half up and clamped, as LibAtrac9 converts
/// (`C/src/utility.c` 23-36).
pub fn to_i16(sample: f64) -> i16 {
    (sample + 0.5)
        .floor()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

#[cfg(test)]
mod tests;
