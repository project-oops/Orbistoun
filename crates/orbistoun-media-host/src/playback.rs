//! Playing an MP4 file: its streams described, and its pictures and sound decoded ahead on threads
//! of their own, queued in presentation order with their times.
//!
//! A media player decodes on its own threads and the title pulls finished frames, so this crate
//! does the same: one thread per stream, each holding its own decoder, each a few frames ahead of
//! the reader and blocked when the queue is full. Times are milliseconds from the start of the
//! presentation. A picture's is its composition time less the track's first edit (ISO/IEC
//! 14496-12 8.6.6). Sound is timed from its stream's first sample and handed out whole, the
//! encoder's priming included, as the console's player hands it out (REQ-20261020T1135Z-vd01:
//! 48 AAC frames from 0 ms for a one-second clip).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use crate::mp4::{self, Format, Mp4Error, Track, TrackKind};
use crate::{AacDecoder, Codec, Decoder, MediaError, Pcm, Picture};

/// Frames a stream's thread decodes ahead of the reader.
const AHEAD: usize = 8;

/// Why a file could not be played.
#[derive(Debug, thiserror::Error)]
pub enum PlaybackError {
    /// The file could not be read as an MP4.
    #[error(transparent)]
    Container(#[from] Mp4Error),
    /// A stream's decoder could not be opened.
    #[error(transparent)]
    Media(#[from] MediaError),
}

/// What one stream of a file carries.
#[derive(Clone, Debug, PartialEq)]
pub enum StreamKind {
    /// Pictures.
    Video {
        /// The coding standard.
        codec: Codec,
        /// Width in pixels, as the container gives it.
        width: u16,
        /// Height in pixels, as the container gives it.
        height: u16,
        /// Samples per second over the track: its sample count over its duration.
        frame_rate: f64,
    },
    /// Sound.
    Audio {
        /// Channel count.
        channels: u16,
        /// Sample frames per second.
        sample_rate: u32,
    },
    /// A track this crate does not decode, by its sample entry's code.
    Other([u8; 4]),
}

/// One stream of a file.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamInfo {
    /// What it carries.
    pub kind: StreamKind,
    /// The track's duration in milliseconds.
    pub duration_ms: u64,
    /// ISO 639-2/T language code.
    pub language: [u8; 3],
}

/// A decoded picture and when it is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoFrame {
    /// Milliseconds from the start of the presentation.
    pub time_ms: u64,
    /// The picture.
    pub picture: Picture,
}

/// Decoded sound and when its first sample plays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioFrame {
    /// Milliseconds from the start of the presentation to the first sample frame.
    pub time_ms: u64,
    /// The samples.
    pub pcm: Pcm,
}

/// What a stream's thread hands the reader.
enum Delivery<T> {
    Frame(T),
    /// The stream's last frame.
    Last(T),
    /// The stream could not go on; the text is why.
    Failed(String),
}

/// One stream being decoded on its own thread.
struct Lane<T> {
    frames: Receiver<Delivery<T>>,
    /// A delivery taken from the queue to see whether one was there, not yet handed out.
    peeked: Mutex<Option<Delivery<T>>>,
    /// Whether the last frame has been handed out.
    finished: AtomicBool,
    thread: Option<JoinHandle<()>>,
}

impl<T> Lane<T> {
    fn new(frames: Receiver<Delivery<T>>, thread: JoinHandle<()>) -> Self {
        Self {
            frames,
            peeked: Mutex::new(None),
            finished: AtomicBool::new(false),
            thread: Some(thread),
        }
    }

    fn take(&self) -> Option<Delivery<T>> {
        let peeked = self
            .peeked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        peeked.or_else(|| self.frames.try_recv().ok())
    }

    fn next(&self) -> Result<Option<T>, String> {
        match self.take() {
            Some(Delivery::Frame(frame)) => Ok(Some(frame)),
            Some(Delivery::Last(frame)) => {
                self.finished.store(true, Ordering::Relaxed);
                Ok(Some(frame))
            }
            Some(Delivery::Failed(why)) => Err(why),
            None => Ok(None),
        }
    }

    fn ended(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Whether every frame has been handed out: the last one taken, or the thread gone with
    /// nothing left behind it.
    fn drained(&self) -> bool {
        if self.finished.load(Ordering::Relaxed) {
            return true;
        }
        let mut peeked = self.peeked.lock().unwrap_or_else(PoisonError::into_inner);
        if peeked.is_some() {
            return false;
        }
        match self.frames.try_recv() {
            Ok(delivery) => {
                *peeked = Some(delivery);
                false
            }
            Err(TryRecvError::Disconnected) => true,
            Err(TryRecvError::Empty) => false,
        }
    }
}

/// A file being played.
pub struct Playback {
    streams: Vec<StreamInfo>,
    video: Option<Lane<VideoFrame>>,
    audio: Option<Lane<AudioFrame>>,
    stop: Arc<AtomicBool>,
}

impl std::fmt::Debug for Playback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Playback")
            .field("streams", &self.streams)
            .finish_non_exhaustive()
    }
}

fn ms(time: i64, timescale: u32) -> u64 {
    if time <= 0 || timescale == 0 {
        0
    } else {
        (time as u128 * 1000 / u128::from(timescale)) as u64
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a frame rate: durations and sample counts far below 2^52"
)]
fn describe(track: &Track) -> StreamInfo {
    let seconds = track.duration as f64 / f64::from(track.timescale.max(1));
    let kind = match &track.format {
        Format::Avc { width, height, .. } | Format::Hevc { width, height, .. } => {
            StreamKind::Video {
                codec: if matches!(track.format, Format::Avc { .. }) {
                    Codec::H264
                } else {
                    Codec::Hevc
                },
                width: *width,
                height: *height,
                frame_rate: if seconds > 0.0 {
                    track.samples.len() as f64 / seconds
                } else {
                    0.0
                },
            }
        }
        Format::Aac {
            channels,
            sample_rate,
            ..
        } => StreamKind::Audio {
            channels: *channels,
            sample_rate: *sample_rate,
        },
        Format::Other(code) => StreamKind::Other(*code),
    };
    StreamInfo {
        kind,
        duration_ms: ms(track.duration as i64, track.timescale),
        language: track.language,
    }
}

impl Playback {
    /// Reads `file`'s streams and starts decoding the first video and the first audio stream.
    /// A file that is not an MP4, and a stream whose decoder this host lacks, are refused here.
    pub fn open(file: &Arc<[u8]>) -> Result<Self, PlaybackError> {
        let movie = mp4::parse(file)?;
        let streams = movie.tracks.iter().map(describe).collect();
        let stop = Arc::new(AtomicBool::new(false));
        let video = movie
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Video)
            .cloned()
            .map(|track| start_video(Arc::clone(file), track, Arc::clone(&stop)))
            .transpose()?;
        let audio = movie
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Audio)
            .cloned()
            .map(|track| start_audio(Arc::clone(file), track, Arc::clone(&stop)))
            .transpose()?;
        Ok(Self {
            streams,
            video,
            audio,
            stop,
        })
    }

    /// Every stream of the file, in track order.
    pub fn streams(&self) -> &[StreamInfo] {
        &self.streams
    }

    /// The next decoded picture, if one is ready. `Err` when the video stream could not go on.
    pub fn next_video(&self) -> Result<Option<VideoFrame>, String> {
        self.video.as_ref().map_or(Ok(None), Lane::next)
    }

    /// The next decoded sound, if some is ready.
    pub fn next_audio(&self) -> Result<Option<AudioFrame>, String> {
        self.audio.as_ref().map_or(Ok(None), Lane::next)
    }

    /// Whether every stream has delivered its last frame to the queue.
    pub fn decoded(&self) -> bool {
        self.video.as_ref().is_none_or(Lane::ended) && self.audio.as_ref().is_none_or(Lane::ended)
    }

    /// Whether every picture has been decoded and handed out: true as soon as the last one has
    /// been taken, before another is asked for.
    pub fn pictures_ended(&self) -> bool {
        self.video.as_ref().is_none_or(Lane::drained)
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Unblock a thread waiting on a full queue by dropping the receivers, then join.
        let threads = [
            self.video.take().and_then(|lane| {
                drop(lane.frames);
                lane.thread
            }),
            self.audio.take().and_then(|lane| {
                drop(lane.frames);
                lane.thread
            }),
        ];
        for thread in threads.into_iter().flatten() {
            let _ = thread.join();
        }
    }
}

/// Sends one delivery; `false` when the reader is gone or playback stopped.
fn send<T>(sender: &SyncSender<Delivery<T>>, stop: &AtomicBool, delivery: Delivery<T>) -> bool {
    !stop.load(Ordering::Relaxed) && sender.send(delivery).is_ok()
}

fn start_video(
    file: Arc<[u8]>,
    track: Track,
    stop: Arc<AtomicBool>,
) -> Result<Lane<VideoFrame>, PlaybackError> {
    let codec = match track.format {
        Format::Hevc { .. } => Codec::Hevc,
        _ => Codec::H264,
    };
    // Opened here as well as on the thread, so a host with no decoder refuses at open.
    drop(Decoder::open(codec)?);
    let (sender, frames) = sync_channel(AHEAD);
    let thread = std::thread::Builder::new()
        .name("media video".to_owned())
        .spawn(move || {
            let mut decoder = match Decoder::open(codec) {
                Ok(decoder) => decoder,
                Err(error) => {
                    let _ = sender.send(Delivery::Failed(error.to_string()));
                    return;
                }
            };
            // One picture is held back, so the last can go out marked as the last.
            let mut held: Option<VideoFrame> = None;
            let mut deliver = |pictures: Vec<Picture>| {
                pictures.into_iter().all(|picture| {
                    let time_ms = ms(picture.timestamp - track.start, track.timescale);
                    match held.replace(VideoFrame { time_ms, picture }) {
                        Some(previous) => send(&sender, &stop, Delivery::Frame(previous)),
                        None => true,
                    }
                })
            };
            for sample in &track.samples {
                let outcome = track
                    .annexb(&file, sample)
                    .map_err(|error| error.to_string())
                    .and_then(|unit| unit.ok_or_else(|| "not a video track".to_owned()))
                    .and_then(|unit| {
                        let time = sample.decode_time as i64 + sample.composition_offset;
                        decoder
                            .decode(&unit, time)
                            .map_err(|error| error.to_string())
                    });
                match outcome {
                    Ok(pictures) => {
                        if !deliver(pictures) {
                            return;
                        }
                    }
                    Err(why) => {
                        let _ = sender.send(Delivery::Failed(why));
                        return;
                    }
                }
            }
            match decoder.drain() {
                Ok(pictures) => {
                    if !deliver(pictures) {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Delivery::Failed(error.to_string()));
                    return;
                }
            }
            if let Some(last) = held {
                send(&sender, &stop, Delivery::Last(last));
            }
        })
        .map_err(|error| MediaError::Host(format!("no decoding thread: {error}")))?;
    Ok(Lane::new(frames, thread))
}

fn start_audio(
    file: Arc<[u8]>,
    track: Track,
    stop: Arc<AtomicBool>,
) -> Result<Lane<AudioFrame>, PlaybackError> {
    let Format::Aac {
        channels,
        sample_rate,
        config,
    } = track.format.clone()
    else {
        return Err(MediaError::NoAudioDecoder("this").into());
    };
    drop(AacDecoder::open(channels, sample_rate, &config)?);
    let (sender, frames) = sync_channel(AHEAD);
    let thread = std::thread::Builder::new()
        .name("media audio".to_owned())
        .spawn(move || {
            let mut decoder = match AacDecoder::open(channels, sample_rate, &config) {
                Ok(decoder) => decoder,
                Err(error) => {
                    let _ = sender.send(Delivery::Failed(error.to_string()));
                    return;
                }
            };
            // Sample frames decoded so far: each decode is timed from the stream's first sample,
            // priming included.
            let mut position = 0_u64;
            let width = usize::from(channels.max(1));
            let mut deliver = |pcms: Vec<Pcm>| {
                for pcm in pcms {
                    let time_ms = position * 1000 / u64::from(sample_rate.max(1));
                    position += (pcm.samples.len() / width) as u64;
                    if pcm.samples.is_empty() {
                        continue;
                    }
                    if !send(&sender, &stop, Delivery::Frame(AudioFrame { time_ms, pcm })) {
                        return false;
                    }
                }
                true
            };
            for sample in &track.samples {
                let frame = usize::try_from(sample.offset)
                    .ok()
                    .and_then(|start| file.get(start..)?.get(..sample.size as usize));
                let outcome = frame
                    .ok_or_else(|| "a sample past the file".to_owned())
                    .and_then(|frame| {
                        decoder
                            .decode(frame, sample.decode_time as i64)
                            .map_err(|error| error.to_string())
                    });
                match outcome {
                    Ok(pcms) => {
                        if !deliver(pcms) {
                            return;
                        }
                    }
                    Err(why) => {
                        let _ = sender.send(Delivery::Failed(why));
                        return;
                    }
                }
            }
            match decoder.drain() {
                Ok(pcms) => {
                    deliver(pcms);
                }
                Err(error) => {
                    let _ = sender.send(Delivery::Failed(error.to_string()));
                }
            }
        })
        .map_err(|error| MediaError::Host(format!("no decoding thread: {error}")))?;
    Ok(Lane::new(frames, thread))
}

#[cfg(all(test, windows))]
mod tests {
    use std::sync::Arc;

    use super::{Playback, StreamKind};
    use crate::Codec;

    fn pull_all(playback: &Playback) -> (Vec<super::VideoFrame>, Vec<super::AudioFrame>) {
        let (mut video, mut audio) = (Vec::new(), Vec::new());
        loop {
            let mut moved = false;
            while let Some(frame) = playback.next_video().expect("video decodes") {
                video.push(frame);
                moved = true;
            }
            while let Some(frame) = playback.next_audio().expect("audio decodes") {
                audio.push(frame);
                moved = true;
            }
            if !moved {
                if playback.decoded()
                    && playback.next_video().unwrap().is_none()
                    && playback.next_audio().unwrap().is_none()
                {
                    return (video, audio);
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }

    /// The fixture plays as two streams: 30 pictures a frame period apart from 0 ms, and a
    /// second of stereo sound starting at 0 ms once the encoder's priming is left out.
    #[test]
    fn the_fixture_plays_in_presentation_order() {
        let file: Arc<[u8]> = Arc::from(&include_bytes!("../tests/data/probe_av.mp4")[..]);
        let playback = Playback::open(&file).expect("the fixture opens");
        let streams = playback.streams();
        assert_eq!(streams.len(), 2);
        let StreamKind::Video {
            codec,
            width,
            height,
            frame_rate,
        } = streams[0].kind
        else {
            panic!("{:?}", streams[0]);
        };
        assert_eq!((codec, width, height), (Codec::H264, 320, 240));
        assert!((frame_rate - 30.0).abs() < 0.01, "{frame_rate}");
        assert_eq!(
            streams[1].kind,
            StreamKind::Audio {
                channels: 2,
                sample_rate: 48_000
            }
        );

        let (video, audio) = pull_all(&playback);
        let times: Vec<u64> = video.iter().map(|frame| frame.time_ms).collect();
        let expected: Vec<u64> = (0..30).map(|index| index * 1000 / 30).collect();
        assert_eq!(times, expected);
        assert!(playback.pictures_ended());
        // Every AAC frame, the encoder's priming frame first, 1024 sample frames each, timed from
        // 0 as the console's player times them (vd01: frame 1 at 21 ms).
        let movie = crate::mp4::parse(&file).expect("the fixture parses");
        let frames = movie.tracks[1].samples.len();
        assert_eq!(audio[0].time_ms, 0);
        assert_eq!(audio[1].time_ms, 21);
        let samples: usize = audio.iter().map(|frame| frame.pcm.samples.len() / 2).sum();
        assert_eq!(samples, frames * 1024);
    }

    /// The last picture taken ends the pictures at once, before another is asked for.
    #[test]
    fn the_pictures_end_with_the_last_one_taken() {
        let file: Arc<[u8]> = Arc::from(&include_bytes!("../tests/data/probe_av.mp4")[..]);
        let playback = Playback::open(&file).expect("the fixture opens");
        let mut taken = 0;
        while taken < 30 {
            match playback.next_video().expect("video decodes") {
                Some(_) => {
                    taken += 1;
                    assert_eq!(playback.pictures_ended(), taken == 30, "after {taken}");
                }
                None => std::thread::sleep(std::time::Duration::from_millis(1)),
            }
        }
    }

    /// Dropping a player mid-stream stops its threads rather than leaving them blocked.
    #[test]
    fn dropping_a_playback_stops_it() {
        let file: Arc<[u8]> = Arc::from(&include_bytes!("../tests/data/probe_av.mp4")[..]);
        let playback = Playback::open(&file).expect("the fixture opens");
        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(playback);
    }

    /// Bytes that are not an MP4 are refused at open.
    #[test]
    fn a_file_that_is_not_an_mp4_is_refused() {
        let file: Arc<[u8]> = Arc::from(&b"not a movie"[..]);
        assert!(Playback::open(&file).is_err());
    }
}
