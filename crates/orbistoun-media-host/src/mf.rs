//! The Media Foundation decoders: the synchronous software decoder transform Windows registers for
//! a codec, fed one access unit per input sample, read as NV12 pictures or 16-bit PCM.
//!
//! The protocol is the transform's own (Microsoft Learn, "Basic MFT Processing Model"): set the
//! input type, pick an output type, then alternate `ProcessInput` and `ProcessOutput` until the
//! transform asks for more input. A format change mid-stream (`MF_E_TRANSFORM_STREAM_CHANGE`) is
//! answered by setting the output type again. The AAC input type is the one Microsoft Learn's
//! "AAC Decoder" page lists: raw payloads, with the HEAACWAVEINFO fields that follow WAVEFORMATEX
//! and then the AudioSpecificConfig as `MF_MT_USER_DATA`.

use std::mem::ManuallyDrop;

use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFMediaType, IMFSample, IMFTransform, MF_E_NOTACCEPTING,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE, MF_MT_AAC_PAYLOAD_TYPE,
    MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE,
    MF_MT_SUBTYPE, MF_MT_USER_DATA, MF_VERSION, MFAudioFormat_AAC, MFAudioFormat_PCM,
    MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Audio, MFMediaType_Video,
    MFSTARTUP_NOSOCKET, MFStartup, MFT_CATEGORY_AUDIO_DECODER, MFT_CATEGORY_VIDEO_DECODER,
    MFT_ENUM_FLAG_LOCALMFT, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT,
    MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_MESSAGE_TYPE, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoArea,
    MFVideoFormat_H264, MFVideoFormat_HEVC, MFVideoFormat_NV12,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree};
use windows::core::GUID;

use crate::{Codec, MediaError, Pcm, Picture};

fn host(error: &windows::core::Error) -> MediaError {
    MediaError::Host(format!("{error} ({:#010x})", error.code().0))
}

/// Starts COM on this thread and Media Foundation in this process. Both count their callers, and
/// neither is stopped: a decoder may be opened again on any thread for the life of the process.
fn start() -> Result<(), MediaError> {
    // SAFETY: no reserved argument is passed; a thread already initialised answers S_FALSE or
    // RPC_E_CHANGED_MODE, and either leaves COM usable for the free-threaded objects used here.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    // SAFETY: MFStartup takes the version it was compiled against and a flag; it has no
    // pointer arguments.
    unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) }.map_err(|error| host(&error))
}

/// What a transform decodes from and to.
#[derive(Clone, Copy)]
struct Route {
    category: GUID,
    major: GUID,
    input: GUID,
    output: GUID,
}

impl Route {
    fn video(codec: Codec) -> Self {
        Self {
            category: MFT_CATEGORY_VIDEO_DECODER,
            major: MFMediaType_Video,
            input: match codec {
                Codec::H264 => MFVideoFormat_H264,
                Codec::Hevc => MFVideoFormat_HEVC,
            },
            output: MFVideoFormat_NV12,
        }
    }

    fn aac() -> Self {
        Self {
            category: MFT_CATEGORY_AUDIO_DECODER,
            major: MFMediaType_Audio,
            input: MFAudioFormat_AAC,
            output: MFAudioFormat_PCM,
        }
    }

    /// The first synchronous decoder transform registered for this route. `None` when the host
    /// has none.
    fn find(self) -> Result<Option<IMFTransform>, MediaError> {
        let input = MFT_REGISTER_TYPE_INFO {
            guidMajorType: self.major,
            guidSubtype: self.input,
        };
        let output = MFT_REGISTER_TYPE_INFO {
            guidMajorType: self.major,
            guidSubtype: self.output,
        };
        let mut found: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0_u32;
        // SAFETY: both type descriptions outlive the call; `found` and `count` are written by it,
        // and the array it allocates is freed below.
        unsafe {
            MFTEnumEx(
                self.category,
                MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_LOCALMFT | MFT_ENUM_FLAG_SORTANDFILTER,
                Some(&raw const input),
                Some(&raw const output),
                &raw mut found,
                &raw mut count,
            )
        }
        .map_err(|error| host(&error))?;
        let activates: Vec<Option<IMFActivate>> = if found.is_null() {
            Vec::new()
        } else {
            (0..count as usize)
                .map(|index| {
                    // SAFETY: `index` is below the `count` entries MFTEnumEx wrote at `found`.
                    let entry = unsafe { found.add(index) };
                    // SAFETY: an initialised entry, moved out exactly once; the array is released
                    // below without dropping its entries.
                    unsafe { entry.read() }
                })
                .collect()
        };
        if !found.is_null() {
            // SAFETY: the array came from CoTaskMemAlloc inside MFTEnumEx, and its entries have
            // been moved out above.
            unsafe { CoTaskMemFree(Some(found.cast_const().cast())) };
        }
        let Some(activate) = activates.into_iter().flatten().next() else {
            return Ok(None);
        };
        // SAFETY: an activation object MFTEnumEx returned, asked for the interface every decoder
        // transform implements.
        unsafe { activate.ActivateObject::<IMFTransform>() }
            .map(Some)
            .map_err(|error| host(&error))
    }
}

fn media_type(major: GUID, subtype: GUID) -> Result<IMFMediaType, MediaError> {
    // SAFETY: MFCreateMediaType takes no arguments and returns an owned type.
    let kind = unsafe { MFCreateMediaType() }.map_err(|error| host(&error))?;
    // SAFETY: setting an attribute on a type this function owns, with GUIDs that outlive the
    // call.
    unsafe { kind.SetGUID(&MF_MT_MAJOR_TYPE, &raw const major) }.map_err(|error| host(&error))?;
    // SAFETY: as above.
    unsafe { kind.SetGUID(&MF_MT_SUBTYPE, &raw const subtype) }.map_err(|error| host(&error))?;
    Ok(kind)
}

fn set_u32(kind: &IMFMediaType, key: &GUID, value: u32) -> Result<(), MediaError> {
    // SAFETY: setting an attribute on a type the caller owns.
    unsafe { kind.SetUINT32(key, value) }.map_err(|error| host(&error))
}

/// A transform set up for one stream, and the buffers it fills.
#[derive(Debug)]
struct Transform {
    mft: IMFTransform,
    output: GUID,
    /// Whether the transform allocates its own output samples.
    provides_samples: bool,
    output_bytes: u32,
}

impl Transform {
    /// Finds the route's transform and sets `input` on it; `missing` is the refusal when the host
    /// registers none.
    fn open(route: Route, input: &IMFMediaType, missing: MediaError) -> Result<Self, MediaError> {
        let Some(transform) = route.find()? else {
            return Err(missing);
        };
        // SAFETY: the transform's only input stream, and a type this module built for it.
        unsafe { transform.SetInputType(0, input, 0) }.map_err(|error| host(&error))?;
        Ok(Self {
            mft: transform,
            output: route.output,
            provides_samples: false,
            output_bytes: 0,
        })
    }

    fn message(&self, message: MFT_MESSAGE_TYPE) -> Result<(), MediaError> {
        // SAFETY: a message that takes no parameter.
        unsafe { self.mft.ProcessMessage(message, 0) }.map_err(|error| host(&error))
    }

    fn begin(&self) -> Result<(), MediaError> {
        self.message(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING)?;
        self.message(MFT_MESSAGE_NOTIFY_START_OF_STREAM)
    }

    /// Sets the first output type of the route's output format that `accept` takes, and returns
    /// it.
    fn choose_output(
        &mut self,
        accept: impl Fn(&IMFMediaType) -> bool,
    ) -> Result<IMFMediaType, MediaError> {
        let mut index = 0;
        let chosen = loop {
            // SAFETY: the transform's only output stream, 0; an index past the end answers
            // MF_E_NO_MORE_TYPES, which ends the search as an error.
            let candidate = unsafe { self.mft.GetOutputAvailableType(0, index) }
                .map_err(|error| host(&error))?;
            // SAFETY: reading an attribute of a type the transform returned.
            let format = unsafe { candidate.GetGUID(&MF_MT_SUBTYPE) };
            if format.is_ok_and(|format| format == self.output) && accept(&candidate) {
                break candidate;
            }
            index += 1;
        };
        // SAFETY: a type the transform offered for its only output stream.
        unsafe { self.mft.SetOutputType(0, &chosen, 0) }.map_err(|error| host(&error))?;
        // SAFETY: the transform's only output stream.
        let info = unsafe { self.mft.GetOutputStreamInfo(0) }.map_err(|error| host(&error))?;
        self.provides_samples = info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
        self.output_bytes = info.cbSize;
        Ok(chosen)
    }

    /// Hands the transform one input and returns every output sample it then has, reading each
    /// with `read`. A format change is answered by `rechoose`.
    fn feed<T>(
        &mut self,
        unit: &[u8],
        timestamp: i64,
        read: &mut impl FnMut(&IMFSample) -> Result<T, MediaError>,
        rechoose: &mut impl FnMut(&mut Self) -> Result<(), MediaError>,
    ) -> Result<Vec<T>, MediaError> {
        let sample = input_sample(unit, timestamp)?;
        let mut outputs = Vec::new();
        loop {
            // SAFETY: the transform's only input stream, and a sample this function owns.
            match unsafe { self.mft.ProcessInput(0, &sample, 0) } {
                Ok(()) => break,
                Err(error) if error.code() == MF_E_NOTACCEPTING => {
                    outputs.extend(self.pending(read, rechoose)?);
                }
                Err(error) => return Err(host(&error)),
            }
        }
        outputs.extend(self.pending(read, rechoose)?);
        Ok(outputs)
    }

    fn drain<T>(
        &mut self,
        read: &mut impl FnMut(&IMFSample) -> Result<T, MediaError>,
        rechoose: &mut impl FnMut(&mut Self) -> Result<(), MediaError>,
    ) -> Result<Vec<T>, MediaError> {
        self.message(MFT_MESSAGE_COMMAND_DRAIN)?;
        let outputs = self.pending(read, rechoose)?;
        self.message(MFT_MESSAGE_NOTIFY_START_OF_STREAM)?;
        Ok(outputs)
    }

    fn flush(&self) -> Result<(), MediaError> {
        self.message(MFT_MESSAGE_COMMAND_FLUSH)?;
        self.message(MFT_MESSAGE_NOTIFY_START_OF_STREAM)
    }

    /// Every output the transform has ready, until it asks for more input.
    fn pending<T>(
        &mut self,
        read: &mut impl FnMut(&IMFSample) -> Result<T, MediaError>,
        rechoose: &mut impl FnMut(&mut Self) -> Result<(), MediaError>,
    ) -> Result<Vec<T>, MediaError> {
        let mut outputs = Vec::new();
        loop {
            let sample = if self.provides_samples {
                None
            } else {
                Some(empty_sample(self.output_bytes)?)
            };
            let mut buffers = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(sample),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            }];
            let mut status = 0_u32;
            // SAFETY: one output buffer for the only output stream, with a sample of the size the
            // transform asked for when it does not provide its own.
            let result = unsafe { self.mft.ProcessOutput(0, &mut buffers, &raw mut status) };
            let [buffer] = buffers;
            let sample = ManuallyDrop::into_inner(buffer.pSample);
            drop(ManuallyDrop::into_inner(buffer.pEvents));
            match result {
                Ok(()) => {
                    let sample = sample
                        .ok_or_else(|| MediaError::Shape("an output with no sample".to_owned()))?;
                    outputs.push(read(&sample)?);
                }
                Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                    return Ok(outputs);
                }
                Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => rechoose(self)?,
                Err(error) => return Err(host(&error)),
            }
        }
    }
}

/// The bytes of an output sample, handed to `read` while its buffer is locked.
fn with_bytes<T>(
    sample: &IMFSample,
    read: impl FnOnce(&[u8]) -> Result<T, MediaError>,
) -> Result<T, MediaError> {
    // SAFETY: the contiguous buffer is owned by the returned interface.
    let buffer = unsafe { sample.ConvertToContiguousBuffer() }.map_err(|error| host(&error))?;
    let mut data: *mut u8 = std::ptr::null_mut();
    let mut length = 0_u32;
    // SAFETY: locking a buffer this function holds; `data` and `length` are written by it, and
    // the buffer is unlocked below before it is released.
    unsafe { buffer.Lock(&raw mut data, None, Some(&raw mut length)) }
        .map_err(|error| host(&error))?;
    let bytes: &[u8] = if data.is_null() {
        &[]
    } else {
        // SAFETY: Lock answered `length` readable bytes at `data`, valid until Unlock.
        unsafe { std::slice::from_raw_parts(data, length as usize) }
    };
    let result = read(bytes);
    // SAFETY: the buffer was locked above and `bytes` is not used after this.
    unsafe { buffer.Unlock() }.map_err(|error| host(&error))?;
    result
}

fn sample_time(sample: &IMFSample) -> Result<i64, MediaError> {
    // SAFETY: reading the time of a sample the transform delivered.
    unsafe { sample.GetSampleTime() }.map_err(|error| host(&error))
}

/// One video decoder transform with an NV12 output type set.
#[derive(Debug)]
pub(crate) struct Decoder {
    transform: Transform,
    /// Visible size and the stride and plane height of the output buffers, from the output type.
    shape: Shape,
}

/// What the current output type says about each picture's buffer.
#[derive(Clone, Copy, Debug, Default)]
struct Shape {
    /// Visible area: the minimum display aperture where the type names one, else the frame.
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    /// Bytes from one luma row to the next.
    stride: u32,
    /// Luma rows in the buffer before the chroma plane begins.
    rows: u32,
}

impl Decoder {
    pub(crate) fn open(codec: Codec) -> Result<Self, MediaError> {
        start()?;
        let route = Route::video(codec);
        let input = media_type(route.major, route.input)?;
        let mut transform = Transform::open(route, &input, MediaError::NoDecoder(codec))?;
        let chosen = transform.choose_output(|_| true)?;
        let shape = shape(&chosen)?;
        transform.begin()?;
        Ok(Self { transform, shape })
    }

    fn rechoose(transform: &mut Transform, shape: &mut Shape) -> Result<(), MediaError> {
        let chosen = transform.choose_output(|_| true)?;
        *shape = self::shape(&chosen)?;
        Ok(())
    }

    pub(crate) fn decode(
        &mut self,
        unit: &[u8],
        timestamp: i64,
    ) -> Result<Vec<Picture>, MediaError> {
        let shape = std::cell::Cell::new(self.shape);
        let pictures = self.transform.feed(
            unit,
            timestamp,
            &mut |sample| picture(sample, shape.get()),
            &mut |transform| {
                let mut current = shape.get();
                Self::rechoose(transform, &mut current)?;
                shape.set(current);
                Ok(())
            },
        )?;
        self.shape = shape.get();
        Ok(pictures)
    }

    pub(crate) fn drain(&mut self) -> Result<Vec<Picture>, MediaError> {
        let shape = std::cell::Cell::new(self.shape);
        let pictures = self.transform.drain(
            &mut |sample| picture(sample, shape.get()),
            &mut |transform| {
                let mut current = shape.get();
                Self::rechoose(transform, &mut current)?;
                shape.set(current);
                Ok(())
            },
        )?;
        self.shape = shape.get();
        Ok(pictures)
    }

    pub(crate) fn flush(&mut self) -> Result<(), MediaError> {
        self.transform.flush()
    }
}

/// Copies the visible area of an output sample's NV12 buffer into a packed picture.
fn picture(sample: &IMFSample, shape: Shape) -> Result<Picture, MediaError> {
    let timestamp = sample_time(sample)?;
    let nv12 = with_bytes(sample, |bytes| pack(bytes, shape))?;
    Ok(Picture {
        width: shape.width,
        height: shape.height,
        timestamp,
        nv12,
    })
}

/// The AAC-LC decoder transform, read as 16-bit interleaved PCM.
#[derive(Debug)]
pub(crate) struct AacDecoder {
    transform: Transform,
    channels: u16,
    sample_rate: u32,
}

/// `MF_MT_AAC_PAYLOAD_TYPE` 0: each input sample is one raw AAC frame, with no ADTS or LOAS
/// framing.
const RAW_PAYLOAD: u32 = 0;

/// HEAACWAVEINFO's `wAudioProfileLevelIndication` 0xFE: no audio profile is specified (Microsoft
/// Learn, "HEAACWAVEINFO structure").
const NO_PROFILE: u16 = 0xfe;

impl AacDecoder {
    pub(crate) fn open(channels: u16, sample_rate: u32, config: &[u8]) -> Result<Self, MediaError> {
        start()?;
        let route = Route::aac();
        let input = media_type(route.major, route.input)?;
        set_u32(&input, &MF_MT_AUDIO_NUM_CHANNELS, u32::from(channels))?;
        set_u32(&input, &MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate)?;
        set_u32(&input, &MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
        set_u32(&input, &MF_MT_AAC_PAYLOAD_TYPE, RAW_PAYLOAD)?;
        // The HEAACWAVEINFO fields after WAVEFORMATEX: payload type, profile, structure type and
        // two reserved words, then the AudioSpecificConfig.
        let mut user = Vec::with_capacity(12 + config.len());
        user.extend_from_slice(&(RAW_PAYLOAD as u16).to_le_bytes());
        user.extend_from_slice(&NO_PROFILE.to_le_bytes());
        user.extend_from_slice(&[0; 8]);
        user.extend_from_slice(config);
        // SAFETY: setting a blob attribute on a type this function owns, from a slice that
        // outlives the call.
        unsafe { input.SetBlob(&MF_MT_USER_DATA, &user) }.map_err(|error| host(&error))?;
        let mut transform = Transform::open(route, &input, MediaError::NoAudioDecoder("AAC"))?;
        transform.choose_output(sixteen_bit)?;
        transform.begin()?;
        Ok(Self {
            transform,
            channels,
            sample_rate,
        })
    }

    pub(crate) fn decode(&mut self, frame: &[u8], timestamp: i64) -> Result<Vec<Pcm>, MediaError> {
        let (channels, sample_rate) = (self.channels, self.sample_rate);
        self.transform.feed(
            frame,
            timestamp,
            &mut |sample| pcm(sample, channels, sample_rate),
            &mut |transform| transform.choose_output(sixteen_bit).map(drop),
        )
    }

    pub(crate) fn drain(&mut self) -> Result<Vec<Pcm>, MediaError> {
        let (channels, sample_rate) = (self.channels, self.sample_rate);
        self.transform.drain(
            &mut |sample| pcm(sample, channels, sample_rate),
            &mut |transform| transform.choose_output(sixteen_bit).map(drop),
        )
    }

    pub(crate) fn flush(&mut self) -> Result<(), MediaError> {
        self.transform.flush()
    }
}

fn sixteen_bit(kind: &IMFMediaType) -> bool {
    // SAFETY: reading an attribute of a type the transform returned.
    unsafe { kind.GetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE) }.is_ok_and(|bits| bits == 16)
}

fn pcm(sample: &IMFSample, channels: u16, sample_rate: u32) -> Result<Pcm, MediaError> {
    let timestamp = sample_time(sample)?;
    let samples = with_bytes(sample, |bytes| {
        Ok(bytes
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect())
    })?;
    Ok(Pcm {
        channels,
        sample_rate,
        timestamp,
        samples,
    })
}

/// An input sample holding `unit`, stamped with `timestamp`.
fn input_sample(unit: &[u8], timestamp: i64) -> Result<IMFSample, MediaError> {
    let length = u32::try_from(unit.len())
        .map_err(|_| MediaError::Shape(format!("an access unit of {} bytes", unit.len())))?;
    // SAFETY: allocates an owned buffer of `length` bytes.
    let buffer = unsafe { MFCreateMemoryBuffer(length) }.map_err(|error| host(&error))?;
    let mut data: *mut u8 = std::ptr::null_mut();
    // SAFETY: locking the buffer just made; `data` is written by it, unlocked below.
    unsafe { buffer.Lock(&raw mut data, None, None) }.map_err(|error| host(&error))?;
    if !data.is_null() {
        // SAFETY: the locked buffer holds at least `length` bytes and does not overlap `unit`.
        unsafe { std::ptr::copy_nonoverlapping(unit.as_ptr(), data, unit.len()) };
    }
    // SAFETY: the buffer was locked above.
    unsafe { buffer.Unlock() }.map_err(|error| host(&error))?;
    // SAFETY: setting the length of a buffer of at least that capacity.
    unsafe { buffer.SetCurrentLength(length) }.map_err(|error| host(&error))?;
    // SAFETY: MFCreateSample takes no arguments and returns an owned sample.
    let sample = unsafe { MFCreateSample() }.map_err(|error| host(&error))?;
    // SAFETY: adding a buffer this function owns to a sample it owns.
    unsafe { sample.AddBuffer(&buffer) }.map_err(|error| host(&error))?;
    // SAFETY: setting the time of a sample this function owns.
    unsafe { sample.SetSampleTime(timestamp) }.map_err(|error| host(&error))?;
    Ok(sample)
}

/// An empty output sample with one buffer of `bytes`.
fn empty_sample(bytes: u32) -> Result<IMFSample, MediaError> {
    // SAFETY: allocates an owned buffer.
    let buffer = unsafe { MFCreateMemoryBuffer(bytes) }.map_err(|error| host(&error))?;
    // SAFETY: MFCreateSample takes no arguments and returns an owned sample.
    let sample = unsafe { MFCreateSample() }.map_err(|error| host(&error))?;
    // SAFETY: adding a buffer this function owns to a sample it owns.
    unsafe { sample.AddBuffer(&buffer) }.map_err(|error| host(&error))?;
    Ok(sample)
}

/// The buffer shape an NV12 output type describes.
fn shape(kind: &IMFMediaType) -> Result<Shape, MediaError> {
    // SAFETY: reading an attribute of a type the transform returned.
    let size = unsafe { kind.GetUINT64(&MF_MT_FRAME_SIZE) }.map_err(|error| host(&error))?;
    let (frame_width, frame_height) = ((size >> 32) as u32, size as u32);
    // SAFETY: as above. A type without a stride attribute has rows exactly as wide as the frame.
    let stride = unsafe { kind.GetUINT32(&MF_MT_DEFAULT_STRIDE) }.unwrap_or(frame_width);
    // An MFVideoArea: two MFOffsets ({u16 fract, i16 value}) and a SIZE ({i32 cx, i32 cy}).
    let mut area = [0_u8; size_of::<MFVideoArea>()];
    let mut area_bytes = 0_u32;
    // SAFETY: reading a blob into a buffer of the length passed; the attribute is optional.
    let aperture = unsafe {
        kind.GetBlob(
            &MF_MT_MINIMUM_DISPLAY_APERTURE,
            &mut area,
            Some(&raw mut area_bytes),
        )
    };
    let word = |at: usize| i32::from_le_bytes([area[at], area[at + 1], area[at + 2], area[at + 3]]);
    let half = |at: usize| i32::from(i16::from_le_bytes([area[at], area[at + 1]]));
    let (left, top, width, height) = if aperture.is_ok() && area_bytes as usize == area.len() {
        (half(2), half(6), word(8), word(12))
    } else {
        (0, 0, frame_width as i32, frame_height as i32)
    };
    Ok(Shape {
        left: left as u32,
        top: top as u32,
        width: width as u32,
        height: height as u32,
        stride,
        rows: frame_height,
    })
}

/// The visible area of an NV12 buffer with `shape`'s stride and plane height, packed.
fn pack(bytes: &[u8], shape: Shape) -> Result<Vec<u8>, MediaError> {
    let (stride, rows) = (shape.stride as usize, shape.rows as usize);
    let (left, top) = (shape.left as usize, shape.top as usize);
    let (width, height) = (shape.width as usize, shape.height as usize);
    if bytes.len() < stride * rows * 3 / 2 || left + width > stride || top + height > rows {
        return Err(MediaError::Shape(format!(
            "{} bytes for a {width}x{height} picture at stride {stride} over {rows} rows",
            bytes.len()
        )));
    }
    let mut packed = Vec::with_capacity(width * height * 3 / 2);
    for row in top..top + height {
        packed.extend_from_slice(&bytes[row * stride + left..][..width]);
    }
    let chroma = &bytes[stride * rows..];
    for row in top / 2..(top + height).div_ceil(2) {
        packed.extend_from_slice(&chroma[row * stride + left..][..width]);
    }
    Ok(packed)
}
