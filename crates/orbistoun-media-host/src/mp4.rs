//! Reading an MP4 file: its tracks, their coding parameters, and where each sample is.
//!
//! The file format is ISO/IEC 14496-12 (the ISO base media file format): a tree of boxes, each a
//! 32-bit size and a four-character type (4.2). A track's sample table (8.5-8.7) gives every
//! sample's size, its chunk's offset, its decoding time and its composition offset; the codec's
//! own parameters sit in the sample entry, as ISO/IEC 14496-15 places them for AVC (`avcC`,
//! 5.3.3.1) and HEVC (`hvcC`, 8.3.3.1), and ISO/IEC 14496-14 for MPEG-4 audio (`esds`, 3.1.2).
//!
//! Fragmented files (`moof`) are not read: a file whose samples are all in fragments has tracks
//! with no samples.

/// Why a file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum Mp4Error {
    /// A box or field runs past the bytes that hold it.
    #[error("{0} runs past the end of its box")]
    Truncated(&'static str),
    /// A field holds a value the format does not allow.
    #[error("{0}")]
    Invalid(String),
    /// The file has no `moov` box.
    #[error("no movie box")]
    NoMovie,
}

/// What a track carries, from its handler (8.4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    /// `vide`.
    Video,
    /// `soun`.
    Audio,
    /// Any other handler, by its code.
    Other([u8; 4]),
}

/// A track's coding format and the parameters a decoder needs before its first sample.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Format {
    /// H.264 (`avc1`/`avc3`): each sample is NAL units with `length_size`-byte lengths.
    Avc {
        /// Width in pixels, from the visual sample entry.
        width: u16,
        /// Height in pixels, from the visual sample entry.
        height: u16,
        /// Bytes in each NAL unit's length prefix: 1, 2 or 4.
        length_size: u8,
        /// Sequence and picture parameter sets, in the configuration record's order.
        parameter_sets: Vec<Vec<u8>>,
    },
    /// H.265 (`hvc1`/`hev1`): as AVC, with video, sequence and picture parameter sets.
    Hevc {
        /// Width in pixels, from the visual sample entry.
        width: u16,
        /// Height in pixels, from the visual sample entry.
        height: u16,
        /// Bytes in each NAL unit's length prefix: 1, 2 or 4.
        length_size: u8,
        /// Video, sequence and picture parameter sets and SEI, in the record's order.
        parameter_sets: Vec<Vec<u8>>,
    },
    /// MPEG-4 audio (`mp4a`) with its AudioSpecificConfig (ISO/IEC 14496-3 1.6.2.1).
    Aac {
        /// Channel count, from the audio sample entry.
        channels: u16,
        /// Samples per second, from the audio sample entry.
        sample_rate: u32,
        /// The AudioSpecificConfig, as the `esds` box holds it.
        config: Vec<u8>,
    },
    /// Any other sample entry, by its code.
    Other([u8; 4]),
}

/// Where one sample is and when it plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// Byte offset of the sample in the file.
    pub offset: u64,
    /// Its size in bytes.
    pub size: u32,
    /// Decoding time, in the track's timescale.
    pub decode_time: u64,
    /// Composition time minus decoding time (8.6.1.3).
    pub composition_offset: i64,
    /// Whether decoding can start at it (8.6.2); every sample when the table has no `stss`.
    pub sync: bool,
}

/// One track of a movie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    /// `track_ID` from the track header.
    pub id: u32,
    /// What the track carries.
    pub kind: TrackKind,
    /// Units per second of the track's times.
    pub timescale: u32,
    /// The media's duration, in the timescale.
    pub duration: u64,
    /// ISO 639-2/T code from the media header, as three lower-case ASCII letters.
    pub language: [u8; 3],
    /// Media time the presentation starts at: the first edit's `media_time` (8.6.6), else 0.
    pub start: i64,
    /// Its coding format, from the first sample entry.
    pub format: Format,
    /// Every sample, in decoding order.
    pub samples: Vec<Sample>,
}

/// The tracks of a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Movie {
    /// In the order the file holds them.
    pub tracks: Vec<Track>,
}

/// A bounds-checked reader over one box's bytes.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    what: &'static str,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], what: &'static str) -> Self {
        Self { bytes, at: 0, what }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], Mp4Error> {
        let end = self
            .at
            .checked_add(count)
            .filter(|&end| end <= self.bytes.len())
            .ok_or(Mp4Error::Truncated(self.what))?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn u8(&mut self) -> Result<u8, Mp4Error> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, Mp4Error> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, Mp4Error> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, Mp4Error> {
        Ok((u64::from(self.u32()?) << 32) | u64::from(self.u32()?))
    }

    fn code(&mut self) -> Result<[u8; 4], Mp4Error> {
        let bytes = self.take(4)?;
        Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    fn rest(&mut self) -> &'a [u8] {
        let rest = &self.bytes[self.at..];
        self.at = self.bytes.len();
        rest
    }

    /// The version byte and 24 flag bits of a full box (4.2).
    fn full(&mut self) -> Result<(u8, u32), Mp4Error> {
        let word = self.u32()?;
        Ok(((word >> 24) as u8, word & 0x00ff_ffff))
    }
}

/// One box: its type and its payload.
type Child<'a> = ([u8; 4], &'a [u8]);

/// The child boxes of `bytes`, as `(type, payload)`.
fn boxes(bytes: &[u8]) -> Result<Vec<Child<'_>>, Mp4Error> {
    let mut reader = Reader::new(bytes, "a box header");
    let mut found = Vec::new();
    while reader.at < bytes.len() {
        let start = reader.at;
        let size = u64::from(reader.u32()?);
        let code = reader.code()?;
        let size = match size {
            0 => (bytes.len() - start) as u64,
            1 => reader.u64()?,
            size => size,
        };
        let header = reader.at - start;
        let size = usize::try_from(size).map_err(|_| Mp4Error::Truncated("a box"))?;
        if size < header {
            return Err(Mp4Error::Invalid(format!("a box of {size} bytes")));
        }
        reader.at = start;
        let whole = reader.take(size)?;
        found.push((code, &whole[header..]));
    }
    Ok(found)
}

fn child(parent: &[u8], code: [u8; 4]) -> Result<Option<&[u8]>, Mp4Error> {
    Ok(boxes(parent)?
        .into_iter()
        .find(|(found, _)| *found == code)
        .map(|(_, payload)| payload))
}

fn required(parent: &[u8], code: [u8; 4]) -> Result<&[u8], Mp4Error> {
    child(parent, code)?
        .ok_or_else(|| Mp4Error::Invalid(format!("no `{}` box", String::from_utf8_lossy(&code))))
}

/// Reads every track of the file in `bytes`.
pub fn parse(bytes: &[u8]) -> Result<Movie, Mp4Error> {
    let movie = child(bytes, *b"moov")?.ok_or(Mp4Error::NoMovie)?;
    let tracks = boxes(movie)?
        .into_iter()
        .filter(|(code, _)| code == b"trak")
        .map(|(_, payload)| track(payload))
        .collect::<Result<_, _>>()?;
    Ok(Movie { tracks })
}

fn track(trak: &[u8]) -> Result<Track, Mp4Error> {
    let mut header = Reader::new(required(trak, *b"tkhd")?, "tkhd");
    let (version, _) = header.full()?;
    header.take(if version == 1 { 16 } else { 8 })?;
    let id = header.u32()?;

    let media = required(trak, *b"mdia")?;
    let mut mdhd = Reader::new(required(media, *b"mdhd")?, "mdhd");
    let (version, _) = mdhd.full()?;
    let (timescale, duration) = if version == 1 {
        mdhd.take(16)?;
        (mdhd.u32()?, mdhd.u64()?)
    } else {
        mdhd.take(8)?;
        (mdhd.u32()?, u64::from(mdhd.u32()?))
    };
    let packed = mdhd.u16()?;
    let language = [
        0x60 + ((packed >> 10) & 0x1f) as u8,
        0x60 + ((packed >> 5) & 0x1f) as u8,
        0x60 + (packed & 0x1f) as u8,
    ];

    let mut hdlr = Reader::new(required(media, *b"hdlr")?, "hdlr");
    hdlr.full()?;
    hdlr.u32()?;
    let kind = match &hdlr.code()? {
        b"vide" => TrackKind::Video,
        b"soun" => TrackKind::Audio,
        other => TrackKind::Other(*other),
    };

    let start = match child(trak, *b"edts")? {
        Some(edts) => match child(edts, *b"elst")? {
            Some(elst) => first_edit(elst)?,
            None => 0,
        },
        None => 0,
    };

    let table = required(required(media, *b"minf")?, *b"stbl")?;
    Ok(Track {
        id,
        kind,
        timescale,
        duration,
        language,
        start,
        format: format(required(table, *b"stsd")?)?,
        samples: samples(table)?,
    })
}

/// The media time of the first edit that is not an empty one (8.6.6.3: `media_time` -1).
fn first_edit(elst: &[u8]) -> Result<i64, Mp4Error> {
    let mut reader = Reader::new(elst, "elst");
    let (version, _) = reader.full()?;
    for _ in 0..reader.u32()? {
        let media_time = if version == 1 {
            reader.u64()?;
            reader.u64()? as i64
        } else {
            reader.u32()?;
            i64::from(reader.u32()? as i32)
        };
        reader.u32()?;
        if media_time >= 0 {
            return Ok(media_time);
        }
    }
    Ok(0)
}

/// The first sample entry of a sample description (8.5.2).
fn format(stsd: &[u8]) -> Result<Format, Mp4Error> {
    let mut reader = Reader::new(stsd, "stsd");
    reader.full()?;
    reader.u32()?;
    let Some((code, entry)) = boxes(reader.rest())?.into_iter().next() else {
        return Err(Mp4Error::Invalid(
            "a sample description with no entry".to_owned(),
        ));
    };
    let mut entry = Reader::new(entry, "a sample entry");
    // SampleEntry: six reserved bytes and a data reference index.
    entry.take(8)?;
    match &code {
        b"avc1" | b"avc3" | b"hvc1" | b"hev1" => {
            // VisualSampleEntry (12.1.3): pre_defined, reserved, pre_defined[3], width, height,
            // resolutions, reserved, frame_count, compressorname[32], depth, pre_defined.
            entry.take(16)?;
            let width = entry.u16()?;
            let height = entry.u16()?;
            entry.take(50)?;
            let children = entry.rest();
            if code.starts_with(b"avc") {
                let (length_size, parameter_sets) = avc_config(required(children, *b"avcC")?)?;
                Ok(Format::Avc {
                    width,
                    height,
                    length_size,
                    parameter_sets,
                })
            } else {
                let (length_size, parameter_sets) = hevc_config(required(children, *b"hvcC")?)?;
                Ok(Format::Hevc {
                    width,
                    height,
                    length_size,
                    parameter_sets,
                })
            }
        }
        b"mp4a" => {
            // AudioSampleEntry (12.2.3): reserved[2], channelcount, samplesize, pre_defined,
            // reserved, samplerate as 16.16.
            entry.take(8)?;
            let channels = entry.u16()?;
            entry.take(6)?;
            let sample_rate = entry.u32()? >> 16;
            let config = audio_config(required(entry.rest(), *b"esds")?)?;
            Ok(Format::Aac {
                channels,
                sample_rate,
                config,
            })
        }
        other => Ok(Format::Other(*other)),
    }
}

fn length_prefixed(reader: &mut Reader<'_>) -> Result<Vec<u8>, Mp4Error> {
    let length = reader.u16()?;
    Ok(reader.take(usize::from(length))?.to_vec())
}

/// AVCDecoderConfigurationRecord (ISO/IEC 14496-15 5.3.3.1).
fn avc_config(record: &[u8]) -> Result<(u8, Vec<Vec<u8>>), Mp4Error> {
    let mut reader = Reader::new(record, "avcC");
    reader.take(4)?;
    let length_size = (reader.u8()? & 0x03) + 1;
    let mut sets = Vec::new();
    for _ in 0..(reader.u8()? & 0x1f) {
        sets.push(length_prefixed(&mut reader)?);
    }
    for _ in 0..reader.u8()? {
        sets.push(length_prefixed(&mut reader)?);
    }
    Ok((length_size, sets))
}

/// HEVCDecoderConfigurationRecord (ISO/IEC 14496-15 8.3.3.1).
fn hevc_config(record: &[u8]) -> Result<(u8, Vec<Vec<u8>>), Mp4Error> {
    let mut reader = Reader::new(record, "hvcC");
    reader.take(21)?;
    let length_size = (reader.u8()? & 0x03) + 1;
    let mut sets = Vec::new();
    for _ in 0..reader.u8()? {
        reader.u8()?;
        for _ in 0..reader.u16()? {
            sets.push(length_prefixed(&mut reader)?);
        }
    }
    Ok((length_size, sets))
}

/// One descriptor of ISO/IEC 14496-1 7.2.2: its tag and its body.
fn descriptor<'a>(reader: &mut Reader<'a>) -> Result<(u8, &'a [u8]), Mp4Error> {
    let tag = reader.u8()?;
    let mut length = 0_usize;
    for _ in 0..4 {
        let byte = reader.u8()?;
        length = (length << 7) | usize::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            break;
        }
    }
    Ok((tag, reader.take(length)?))
}

/// The DecoderSpecificInfo inside an `esds` box's ES_Descriptor (14496-1 7.2.6.5-7.2.6.7).
fn audio_config(esds: &[u8]) -> Result<Vec<u8>, Mp4Error> {
    let mut reader = Reader::new(esds, "esds");
    reader.full()?;
    let (tag, body) = descriptor(&mut reader)?;
    if tag != 0x03 {
        return Err(Mp4Error::Invalid(format!(
            "an esds starting with tag {tag:#x}"
        )));
    }
    let mut es = Reader::new(body, "an ES_Descriptor");
    es.u16()?;
    let flags = es.u8()?;
    if flags & 0x80 != 0 {
        es.u16()?;
    }
    if flags & 0x40 != 0 {
        let length = es.u8()?;
        es.take(usize::from(length))?;
    }
    if flags & 0x20 != 0 {
        es.u16()?;
    }
    while es.at < body.len() {
        let (tag, body) = descriptor(&mut es)?;
        if tag != 0x04 {
            continue;
        }
        let mut config = Reader::new(body, "a DecoderConfigDescriptor");
        config.take(13)?;
        while config.at < body.len() {
            let (tag, info) = descriptor(&mut config)?;
            if tag == 0x05 {
                return Ok(info.to_vec());
            }
        }
    }
    Err(Mp4Error::Invalid(
        "an esds with no DecoderSpecificInfo".to_owned(),
    ))
}

/// Run-length tables of `(count, value)` pairs, as `stts` and `ctts` hold them.
fn runs(table: &[u8], what: &'static str) -> Result<(u8, Vec<(u32, u32)>), Mp4Error> {
    let mut reader = Reader::new(table, what);
    let (version, _) = reader.full()?;
    let count = reader.u32()?;
    let mut runs = Vec::new();
    for _ in 0..count {
        runs.push((reader.u32()?, reader.u32()?));
    }
    Ok((version, runs))
}

/// Every sample of a sample table (8.5-8.7), in decoding order.
fn samples(table: &[u8]) -> Result<Vec<Sample>, Mp4Error> {
    let mut stsz = Reader::new(required(table, *b"stsz")?, "stsz");
    stsz.full()?;
    let fixed = stsz.u32()?;
    let count = stsz.u32()? as usize;
    let sizes = (0..count)
        .map(|_| if fixed == 0 { stsz.u32() } else { Ok(fixed) })
        .collect::<Result<Vec<_>, _>>()?;

    let chunk_offsets = if let Some(stco) = child(table, *b"stco")? {
        let mut reader = Reader::new(stco, "stco");
        reader.full()?;
        (0..reader.u32()?)
            .map(|_| reader.u32().map(u64::from))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let mut reader = Reader::new(required(table, *b"co64")?, "co64");
        reader.full()?;
        (0..reader.u32()?)
            .map(|_| reader.u64())
            .collect::<Result<Vec<_>, _>>()?
    };

    let mut chunk_map = Reader::new(required(table, *b"stsc")?, "stsc");
    chunk_map.full()?;
    let mut chunk_runs = Vec::new();
    for _ in 0..chunk_map.u32()? {
        let first = chunk_map.u32()?;
        let per_chunk = chunk_map.u32()?;
        chunk_map.u32()?;
        chunk_runs.push((first, per_chunk));
    }

    let mut offsets = Vec::with_capacity(count);
    for (index, &(first, per_chunk)) in chunk_runs.iter().enumerate() {
        let last = chunk_runs
            .get(index + 1)
            .map_or(chunk_offsets.len() as u32, |next| next.0 - 1);
        for chunk in first..=last {
            let mut offset = *chunk_offsets
                .get(chunk.wrapping_sub(1) as usize)
                .ok_or_else(|| Mp4Error::Invalid(format!("no offset for chunk {chunk}")))?;
            for _ in 0..per_chunk {
                let Some(&size) = sizes.get(offsets.len()) else {
                    break;
                };
                offsets.push(offset);
                offset += u64::from(size);
            }
        }
    }
    if offsets.len() != count {
        return Err(Mp4Error::Invalid(format!(
            "{} samples sized and {} placed in chunks",
            count,
            offsets.len()
        )));
    }

    let (_, deltas) = runs(required(table, *b"stts")?, "stts")?;
    let mut decode_times = Vec::with_capacity(count);
    let mut time = 0_u64;
    for (run, delta) in deltas {
        for _ in 0..run {
            decode_times.push(time);
            time += u64::from(delta);
        }
    }

    let mut composition = vec![0_i64; count];
    if let Some(ctts) = child(table, *b"ctts")? {
        let (version, offsets) = runs(ctts, "ctts")?;
        let values = offsets.into_iter().flat_map(|(run, value)| {
            let value = if version == 0 {
                i64::from(value)
            } else {
                i64::from(value as i32)
            };
            std::iter::repeat_n(value, run as usize)
        });
        for (slot, value) in composition.iter_mut().zip(values) {
            *slot = value;
        }
    }

    let sync: Option<Vec<u32>> = child(table, *b"stss")?
        .map(|stss| {
            let mut reader = Reader::new(stss, "stss");
            reader.full()?;
            (0..reader.u32()?).map(|_| reader.u32()).collect()
        })
        .transpose()?;

    Ok((0..count)
        .map(|index| Sample {
            offset: offsets[index],
            size: sizes[index],
            decode_time: decode_times.get(index).copied().unwrap_or(time),
            composition_offset: composition[index],
            sync: sync
                .as_ref()
                .is_none_or(|sync| sync.binary_search(&(index as u32 + 1)).is_ok()),
        })
        .collect())
}

impl Track {
    /// The bytes of `sample` in `file` as an Annex-B access unit: each length-prefixed NAL unit
    /// behind a start code, with the parameter sets first on a sync sample. `None` for a track
    /// that is not AVC or HEVC.
    pub fn annexb(&self, file: &[u8], sample: &Sample) -> Result<Option<Vec<u8>>, Mp4Error> {
        let (length_size, sets) = match &self.format {
            Format::Avc {
                length_size,
                parameter_sets,
                ..
            }
            | Format::Hevc {
                length_size,
                parameter_sets,
                ..
            } => (usize::from(*length_size), parameter_sets),
            _ => return Ok(None),
        };
        let start = usize::try_from(sample.offset).map_err(|_| Mp4Error::Truncated("a sample"))?;
        let bytes = file
            .get(start..)
            .and_then(|rest| rest.get(..sample.size as usize))
            .ok_or(Mp4Error::Truncated("a sample"))?;
        let mut unit = Vec::with_capacity(bytes.len() + 64);
        if sample.sync {
            for set in sets {
                unit.extend_from_slice(&[0, 0, 0, 1]);
                unit.extend_from_slice(set);
            }
        }
        let mut reader = Reader::new(bytes, "a sample's NAL units");
        while reader.at < bytes.len() {
            let length = reader
                .take(length_size)?
                .iter()
                .fold(0_usize, |length, &byte| (length << 8) | usize::from(byte));
            unit.extend_from_slice(&[0, 0, 0, 1]);
            unit.extend_from_slice(reader.take(length)?);
        }
        Ok(Some(unit))
    }
}

#[cfg(test)]
mod tests {
    use super::{Format, TrackKind, parse};

    const FILE: &[u8] = include_bytes!("../tests/data/probe_av.mp4");

    /// The fixture's video track: AVC 320x240, 30 samples, only the first a sync sample (GOP 30),
    /// one SPS and one PPS with 4-byte lengths, and B-frames, so composition offsets differ.
    #[test]
    fn the_video_track_is_read() {
        let movie = parse(FILE).expect("the fixture parses");
        let video = movie
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Video)
            .expect("a video track");
        let Format::Avc {
            width,
            height,
            length_size,
            parameter_sets,
        } = &video.format
        else {
            panic!("{:?}", video.format);
        };
        assert_eq!((*width, *height, *length_size), (320, 240, 4));
        assert_eq!(parameter_sets.len(), 2);
        assert_eq!(parameter_sets[0][0] & 0x1f, 7);
        assert_eq!(parameter_sets[1][0] & 0x1f, 8);
        assert_eq!(video.samples.len(), 30);
        assert_eq!(video.samples.iter().filter(|sample| sample.sync).count(), 1);
        assert!(video.samples[0].sync);
        let mut composition: Vec<i64> = video
            .samples
            .iter()
            .map(|sample| sample.decode_time as i64 + sample.composition_offset)
            .collect();
        assert_ne!(composition, {
            let mut sorted = composition.clone();
            sorted.sort_unstable();
            sorted
        });
        composition.sort_unstable();
        composition.dedup();
        assert_eq!(composition.len(), 30);
        assert_eq!(&video.language, b"und");
    }

    /// The audio track: AAC, 2 channels at 48 kHz. Its AudioSpecificConfig is object type 2 (AAC
    /// LC) with frequency index 3 (48000) and channel configuration 2 (ISO/IEC 14496-3 1.6.2.1),
    /// then the backward-compatible SBR extension: sync word 0x2b7, extension object type 5, and
    /// the SBR flag clear (1.6.5).
    #[test]
    fn the_audio_track_is_read() {
        let movie = parse(FILE).expect("the fixture parses");
        let audio = movie
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Audio)
            .expect("an audio track");
        assert_eq!(
            audio.format,
            Format::Aac {
                channels: 2,
                sample_rate: 48_000,
                config: vec![0x11, 0x90, 0x56, 0xe5, 0x00],
            }
        );
        assert_eq!(audio.timescale, 48_000);
        assert!(!audio.samples.is_empty());
    }

    /// The video samples, as Annex-B units, hold the elementary stream's NAL units, unit for unit:
    /// the first with the parameter sets ahead of its picture. Start codes may differ in width,
    /// and parameter sets and SEI may come in either order before the first slice (H.264
    /// 7.4.1.2.3), so each unit's NAL units are compared as a set.
    #[test]
    fn samples_convert_to_the_elementary_stream() {
        let movie = parse(FILE).expect("the fixture parses");
        let video = movie
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Video)
            .expect("a video track");
        let stream = include_bytes!("../tests/data/probe_avc.h264");
        let expected = crate::annexb::access_units(crate::Codec::H264, stream);
        assert_eq!(video.samples.len(), expected.len());
        for (sample, expected) in video.samples.iter().zip(expected) {
            let unit = video
                .annexb(FILE, sample)
                .expect("the sample is in the file")
                .expect("an AVC track converts");
            let mut ours = crate::annexb::nal_payloads(&unit);
            let mut theirs = crate::annexb::nal_payloads(expected);
            ours.sort_unstable();
            theirs.sort_unstable();
            assert_eq!(ours, theirs);
        }
    }

    /// Bytes that are not a box tree are refused rather than read as an empty movie.
    #[test]
    fn a_file_without_a_movie_is_refused() {
        assert!(parse(&[0, 0, 0, 8, b'f', b'r', b'e', b'e']).is_err());
        assert!(parse(&[0, 0, 0, 0x40, b'm', b'o', b'o', b'v']).is_err());
    }
}
