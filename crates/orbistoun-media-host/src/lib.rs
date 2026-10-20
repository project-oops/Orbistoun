//! Video decoding on the host's own media decoder: the codec behind the guest's players.
//!
//! The guest's video libraries hand a title decoded pictures in the console's layout. What a
//! picture holds is fixed by the coding standard - H.264 and H.265 define their decoding process
//! exactly, so every conforming decoder produces the same samples - and only the layout around
//! it is the console's. This crate produces the samples; `orbistoun-video` places them.
//!
//! The decoder is the one the host operating system ships (D925): Media Foundation on Windows.
//! Elsewhere [`Decoder::open`] refuses by name, and a title's video does not play.
//!
//! A [`Decoder`] takes one access unit at a time ([`annexb::access_units`] splits a stream) and
//! hands back the pictures that unit completes, in display order, as packed NV12. An
//! [`AacDecoder`] takes one raw AAC frame at a time and hands back 16-bit PCM. [`mp4`] reads the
//! tracks and samples of an MP4 file for both.

pub mod annexb;
pub mod mp4;
pub mod playback;

#[cfg(windows)]
mod mf;

pub use annexb::Codec;

/// Why a decoder could not be opened or could not decode.
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    /// This host has no decoder for the codec.
    #[error("no {0:?} decoder on this host")]
    NoDecoder(Codec),
    /// The host decoder refused the stream; the text is its own account.
    #[error("the host decoder refused: {0}")]
    Host(String),
    /// The host decoder delivered a picture in a shape this crate does not read.
    #[error("the host decoder delivered {0}")]
    Shape(String),
    /// This host has no decoder for the audio coding named.
    #[error("no {0} decoder on this host")]
    NoAudioDecoder(&'static str),
}

/// Decoded audio: 16-bit samples, channels interleaved.
#[derive(Clone, PartialEq, Eq)]
pub struct Pcm {
    /// Channels per sample frame.
    pub channels: u16,
    /// Sample frames per second.
    pub sample_rate: u32,
    /// The timestamp the frame that carried these samples was decoded with.
    pub timestamp: i64,
    /// `channels` values per sample frame, interleaved.
    pub samples: Vec<i16>,
}

impl std::fmt::Debug for Pcm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pcm")
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .field("timestamp", &self.timestamp)
            .field("samples", &self.samples.len())
            .finish()
    }
}

/// An AAC (MPEG-4 audio, Low Complexity) decoder for one stream, fed raw frames.
#[derive(Debug)]
pub struct AacDecoder {
    #[cfg(windows)]
    inner: mf::AacDecoder,
}

impl AacDecoder {
    /// Opens a decoder for a stream with this channel count, rate and AudioSpecificConfig (ISO/IEC
    /// 14496-3 1.6.2.1), as an MP4 sample entry gives them.
    pub fn open(channels: u16, sample_rate: u32, config: &[u8]) -> Result<Self, MediaError> {
        #[cfg(windows)]
        {
            Ok(Self {
                inner: mf::AacDecoder::open(channels, sample_rate, config)?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (channels, sample_rate, config);
            Err(MediaError::NoAudioDecoder("AAC"))
        }
    }

    /// Decodes one raw frame and returns the samples now ready.
    pub fn decode(&mut self, frame: &[u8], timestamp: i64) -> Result<Vec<Pcm>, MediaError> {
        #[cfg(windows)]
        {
            self.inner.decode(frame, timestamp)
        }
        #[cfg(not(windows))]
        {
            let _ = (frame, timestamp);
            unreachable!("a decoder is never opened on this host")
        }
    }

    /// Ends the stream and returns the samples the decoder still holds.
    pub fn drain(&mut self) -> Result<Vec<Pcm>, MediaError> {
        #[cfg(windows)]
        {
            self.inner.drain()
        }
        #[cfg(not(windows))]
        {
            unreachable!("a decoder is never opened on this host")
        }
    }

    /// Discards everything held, so the next frame decodes as the start of a stream.
    pub fn flush(&mut self) -> Result<(), MediaError> {
        #[cfg(windows)]
        {
            self.inner.flush()
        }
        #[cfg(not(windows))]
        {
            unreachable!("a decoder is never opened on this host")
        }
    }
}

/// One decoded picture: 8-bit 4:2:0, the luma plane followed by the interleaved chroma plane
/// (NV12), each row `width` bytes with no padding.
#[derive(Clone, PartialEq, Eq)]
pub struct Picture {
    /// Visible width in pixels, after the stream's cropping.
    pub width: u32,
    /// Visible height in pixels, after the stream's cropping.
    pub height: u32,
    /// The timestamp the access unit that carried this picture was decoded with.
    pub timestamp: i64,
    /// `width * height` luma bytes, then `width * height / 2` chroma bytes.
    pub nv12: Vec<u8>,
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Picture")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("timestamp", &self.timestamp)
            .field("bytes", &self.nv12.len())
            .finish()
    }
}

/// A video decoder for one stream.
#[derive(Debug)]
pub struct Decoder {
    #[cfg(windows)]
    inner: mf::Decoder,
}

impl Decoder {
    /// Opens a decoder for `codec`.
    pub fn open(codec: Codec) -> Result<Self, MediaError> {
        #[cfg(windows)]
        {
            Ok(Self {
                inner: mf::Decoder::open(codec)?,
            })
        }
        #[cfg(not(windows))]
        {
            Err(MediaError::NoDecoder(codec))
        }
    }

    /// Decodes one access unit, with the timestamp its picture carries out, and returns the
    /// pictures now complete. A stream with reordered pictures completes none for its first
    /// units and catches up on [`Decoder::drain`].
    pub fn decode(&mut self, unit: &[u8], timestamp: i64) -> Result<Vec<Picture>, MediaError> {
        #[cfg(windows)]
        {
            self.inner.decode(unit, timestamp)
        }
        #[cfg(not(windows))]
        {
            let _ = (unit, timestamp);
            unreachable!("a decoder is never opened on this host")
        }
    }

    /// Ends the stream and returns every picture still held for reordering.
    pub fn drain(&mut self) -> Result<Vec<Picture>, MediaError> {
        #[cfg(windows)]
        {
            self.inner.drain()
        }
        #[cfg(not(windows))]
        {
            unreachable!("a decoder is never opened on this host")
        }
    }

    /// Discards everything held, so the next unit decodes as the start of a stream.
    pub fn flush(&mut self) -> Result<(), MediaError> {
        #[cfg(windows)]
        {
            self.inner.flush()
        }
        #[cfg(not(windows))]
        {
            unreachable!("a decoder is never opened on this host")
        }
    }
}

/// The fixtures under `tests/data` are generated, not captured: ffmpeg's `testsrc2` pattern and a
/// 1 kHz sine, 320x240 at 30 fps, 30 frames, GOP 30, two B-frames. obSCEne's probe package holds
/// the same files for REQ-20261020T1135Z-vd01, so a console's rows and these tests read the same
/// bytes. `probe_av.mp4` is H.264 High@3.0 with AAC-LC stereo 48 kHz (the sine on the left
/// channel only); `probe_avc.h264` is its video as an Annex-B stream; `probe_hevc.h265` is the
/// same picture content as HEVC Main. Each `.nv12.sha256` lists the SHA-256 of every decoded
/// picture, in display order, packed NV12, as a second and independent decoder (ffmpeg 7.1.1,
/// `-f rawvideo -pix_fmt nv12`) produced them.
#[cfg(all(test, windows))]
mod tests {
    use std::fmt::Write;

    use sha2::{Digest, Sha256};

    use super::{Codec, Decoder, annexb};

    fn decode_all(codec: Codec, stream: &[u8]) -> Vec<super::Picture> {
        let mut decoder = Decoder::open(codec).expect("the host has a decoder");
        let mut pictures = Vec::new();
        for (index, unit) in annexb::access_units(codec, stream).into_iter().enumerate() {
            pictures.extend(
                decoder
                    .decode(unit, index as i64)
                    .expect("the unit decodes"),
            );
        }
        pictures.extend(decoder.drain().expect("the stream drains"));
        pictures
    }

    fn hashes(pictures: &[super::Picture]) -> Vec<String> {
        pictures
            .iter()
            .map(|picture| {
                Sha256::digest(&picture.nv12)
                    .iter()
                    .fold(String::new(), |mut hex, byte| {
                        let _ = write!(hex, "{byte:02x}");
                        hex
                    })
            })
            .collect()
    }

    fn reference(listing: &str) -> Vec<String> {
        listing.lines().map(str::to_owned).collect()
    }

    /// Every picture of the H.264 fixture equals the independent decoder's, byte for byte, in
    /// display order, with the timestamps of the units that carried them.
    #[test]
    fn h264_pictures_equal_the_reference() {
        let pictures = decode_all(Codec::H264, include_bytes!("../tests/data/probe_avc.h264"));
        assert_eq!(pictures.len(), 30);
        assert!(pictures.iter().all(|p| p.width == 320 && p.height == 240));
        assert_eq!(
            hashes(&pictures),
            reference(include_str!("../tests/data/probe_avc.nv12.sha256"))
        );
        // Decoding order is I P B B P B B..., so the third unit carries the second picture.
        assert_eq!(pictures[0].timestamp, 0);
        assert_eq!(pictures[1].timestamp, 2);
        assert_eq!(pictures[3].timestamp, 1);
    }

    /// The MP4's video samples decode to the same pictures as the elementary stream.
    #[test]
    fn mp4_video_samples_decode_to_the_reference() {
        let file = include_bytes!("../tests/data/probe_av.mp4");
        let movie = crate::mp4::parse(file).expect("the fixture parses");
        let video = movie
            .tracks
            .iter()
            .find(|track| track.kind == crate::mp4::TrackKind::Video)
            .expect("a video track");
        let mut decoder = Decoder::open(Codec::H264).expect("the host has a decoder");
        let mut pictures = Vec::new();
        for sample in &video.samples {
            let unit = video.annexb(file, sample).unwrap().unwrap();
            let time = sample.decode_time as i64 + sample.composition_offset;
            pictures.extend(decoder.decode(&unit, time).expect("the unit decodes"));
        }
        pictures.extend(decoder.drain().expect("the stream drains"));
        assert_eq!(
            hashes(&pictures),
            reference(include_str!("../tests/data/probe_avc.nv12.sha256"))
        );
        assert!(
            pictures
                .windows(2)
                .all(|pair| pair[0].timestamp < pair[1].timestamp)
        );
    }

    /// The MP4's audio decodes to its content, which is what AAC defines: AAC decoders are not
    /// bit-exact with one another (ISO/IEC 14496-26 sets tolerances), so the test reads the signal.
    /// The left channel is a 1 kHz sine, which crosses zero 2000 times a second; the right is
    /// silence.
    #[test]
    fn mp4_audio_decodes_to_a_left_channel_sine() {
        let file = include_bytes!("../tests/data/probe_av.mp4");
        let movie = crate::mp4::parse(file).expect("the fixture parses");
        let audio = movie
            .tracks
            .iter()
            .find(|track| track.kind == crate::mp4::TrackKind::Audio)
            .expect("an audio track");
        let crate::mp4::Format::Aac {
            channels,
            sample_rate,
            config,
        } = &audio.format
        else {
            panic!("{:?}", audio.format);
        };
        let mut decoder =
            super::AacDecoder::open(*channels, *sample_rate, config).expect("the host decodes AAC");
        let mut samples = Vec::new();
        for sample in &audio.samples {
            let start = sample.offset as usize;
            let frame = &file[start..start + sample.size as usize];
            for pcm in decoder.decode(frame, sample.decode_time as i64).unwrap() {
                assert_eq!((pcm.channels, pcm.sample_rate), (2, 48_000));
                samples.extend(pcm.samples);
            }
        }
        // The middle half second, clear of the encoder's priming and the stream's end.
        let frames: Vec<(i16, i16)> = samples
            .chunks_exact(2)
            .map(|pair| (pair[0], pair[1]))
            .skip(12_000)
            .take(24_000)
            .collect();
        assert_eq!(frames.len(), 24_000);
        let crossings = frames
            .windows(2)
            .filter(|pair| (pair[0].0 < 0) != (pair[1].0 < 0))
            .count();
        assert!((990..=1010).contains(&crossings), "{crossings} crossings");
        let loudest_right = frames.iter().map(|frame| frame.1.unsigned_abs()).max();
        let loudest_left = frames.iter().map(|frame| frame.0.unsigned_abs()).max();
        // ffmpeg's `sine` source peaks at an eighth of full scale, 4096.
        assert!(loudest_left > Some(3000), "{loudest_left:?}");
        assert!(loudest_right < Some(64), "{loudest_right:?}");
    }

    /// A flush discards what was held: the stream decodes again from its start.
    #[test]
    fn a_flush_starts_the_stream_again() {
        let stream = include_bytes!("../tests/data/probe_avc.h264");
        let units = annexb::access_units(Codec::H264, stream);
        let mut decoder = Decoder::open(Codec::H264).expect("the host has a decoder");
        for (index, unit) in units.iter().take(5).enumerate() {
            decoder
                .decode(unit, index as i64)
                .expect("the unit decodes");
        }
        decoder.flush().expect("the decoder flushes");
        let mut pictures = Vec::new();
        for (index, unit) in units.iter().enumerate() {
            pictures.extend(
                decoder
                    .decode(unit, index as i64)
                    .expect("the unit decodes"),
            );
        }
        pictures.extend(decoder.drain().expect("the stream drains"));
        assert_eq!(
            hashes(&pictures),
            reference(include_str!("../tests/data/probe_avc.nv12.sha256"))
        );
    }
}
