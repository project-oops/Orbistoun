//! Audio output on a host device: the mixer behind the guest's output ports.
//!
//! `orbistoun-audio` serves the guest's ports and hands each chunk of samples to a sink at guest
//! semantics - a port, its channel count, 16-bit samples at [`GUEST_RATE`]. This crate is one
//! such sink. Each port queues what it is handed; the device's callback mixes the queues into
//! whatever format and rate the device runs at. A port's output call then waits on its queue
//! rather than on a sleep, so the guest is paced by the device's own clock and the two cannot
//! drift apart.
//!
//! The device is opened through `cpal`, on Windows only for now; elsewhere [`open`] says this
//! host plays nothing, and the ports go on pacing themselves by the sample rate. The mixer is
//! independent of any device and is tested apart from one.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The rate every guest port plays at, the one `sceAudioOutOpen` serves.
pub const GUEST_RATE: u32 = 48_000;

/// Frames a port may have queued before its output call waits: about 64 ms at the guest rate.
/// Comfortably more than a sleep's error on a host whose timer ticks every 15.6 ms, so a late
/// wake-up does not empty the queue.
const TARGET: usize = 3072;

/// Frames a port must have queued before it is heard, after starting or running dry, so the
/// device does not alternate one chunk with silence while the guest catches up.
const PREROLL: usize = 2048;

/// The most a port holds, half a second; past it the oldest frames are dropped. Reached only
/// when the device has stopped taking samples.
const MAX_QUEUED: usize = GUEST_RATE as usize / 2;

/// What the device was opened as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opened {
    /// Its sample rate, which the mixer resamples the guest rate to.
    pub rate: u32,
    /// Its channel count; the guest's left and right go to the first two.
    pub channels: u16,
}

/// One port's queued frames, as left-right pairs.
#[derive(Debug, Default)]
struct Queue {
    frames: VecDeque<[i16; 2]>,
    /// Whether the port is being heard: set at [`PREROLL`], cleared when it runs dry.
    primed: bool,
}

/// Every port's queue, and where between two guest frames the device's output stands.
#[derive(Debug)]
struct Mixer {
    ports: BTreeMap<u32, Queue>,
    /// Guest frames, in units of the device rate, owed since the last frame was taken: the
    /// guest rate is added per device frame and a frame taken per device rate.
    phase: u64,
}

impl Mixer {
    const fn new() -> Self {
        Self {
            ports: BTreeMap::new(),
            phase: 0,
        }
    }

    /// Queues one chunk of interleaved samples on a port and answers how many frames it holds.
    ///
    /// A mono chunk plays on both sides; past two channels only the first two are heard.
    fn push(&mut self, port: u32, channels: u8, samples: &[i16]) -> usize {
        let queue = self.ports.entry(port).or_default();
        if channels == 0 {
            return queue.frames.len();
        }
        for frame in samples.chunks_exact(usize::from(channels)) {
            queue
                .frames
                .push_back([frame[0], *frame.get(1).unwrap_or(&frame[0])]);
        }
        let over = queue.frames.len().saturating_sub(MAX_QUEUED);
        queue.frames.drain(..over);
        queue.frames.len()
    }

    fn close(&mut self, port: u32) {
        self.ports.remove(&port);
    }

    /// Fills `out`, interleaved for a device of `channels` channels at `rate`, with every heard
    /// port summed. Resampling is nearest-frame: each guest frame is held for as many device
    /// frames as its share of the time. A port with nothing queued adds silence.
    #[cfg_attr(
        not(windows),
        allow(
            dead_code,
            reason = "called by the device, which opens on Windows only; the tests call it everywhere"
        )
    )]
    fn fill(&mut self, out: &mut [f32], channels: usize, rate: u32) {
        out.fill(0.0);
        if channels == 0 || rate == 0 {
            return;
        }
        for frame in out.chunks_exact_mut(channels) {
            let (mut left, mut right) = (0_i32, 0_i32);
            for queue in self.ports.values_mut() {
                if queue.frames.len() >= PREROLL {
                    queue.primed = true;
                }
                if let (true, Some(&[l, r])) = (queue.primed, queue.frames.front()) {
                    left += i32::from(l);
                    right += i32::from(r);
                }
            }
            // Clipped to the sample range, so a loud sum saturates rather than wraps.
            let level = |sum: i32| {
                let clipped =
                    i16::try_from(sum.clamp(i16::MIN.into(), i16::MAX.into())).unwrap_or_default();
                f32::from(clipped) / 32768.0
            };
            if channels == 1 {
                frame[0] = level((left + right) / 2);
            } else {
                frame[0] = level(left);
                frame[1] = level(right);
            }
            self.phase += u64::from(GUEST_RATE);
            while self.phase >= u64::from(rate) {
                self.phase -= u64::from(rate);
                for queue in self.ports.values_mut().filter(|q| q.primed) {
                    queue.frames.pop_front();
                    queue.primed = !queue.frames.is_empty();
                }
            }
        }
    }
}

/// How long a port holding `queued` frames waits after a chunk of `chunk` frames: until it is
/// back to [`TARGET`], and never longer than two chunks, so a stalled device slows a guest
/// rather than stopping it.
fn wait_for(queued: usize, chunk: usize) -> Duration {
    let frames = queued.saturating_sub(TARGET).min(chunk.saturating_mul(2));
    frames_to_duration(frames)
}

fn frames_to_duration(frames: usize) -> Duration {
    Duration::from_micros(frames as u64 * 1_000_000 / u64::from(GUEST_RATE))
}

static MIXER: Mutex<Mixer> = Mutex::new(Mixer::new());

/// Set when the device reports an error, after which ports pace themselves by the sample rate.
static FAILED: AtomicBool = AtomicBool::new(false);

fn mixer() -> std::sync::MutexGuard<'static, Mixer> {
    MIXER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Queues a port's chunk for the device and answers how long the port's output call waits.
///
/// Once the device has failed, the chunk is dropped and the wait is the time it takes to play,
/// as with no device at all.
pub fn play(port: u32, channels: u8, samples: &[i16]) -> Duration {
    let chunk = samples.len() / usize::from(channels.max(1));
    if FAILED.load(Ordering::Relaxed) {
        return frames_to_duration(chunk);
    }
    let queued = mixer().push(port, channels, samples);
    wait_for(queued, chunk)
}

/// Forgets a closed port and what it still had queued.
pub fn close(port: u32) {
    mixer().close(port);
}

/// Opens the host's default output device and starts it playing the mixer.
///
/// The device lives on a thread of its own for the rest of the process. On a host with no
/// device, or one this build cannot drive, the error says why.
///
/// # Errors
///
/// The reason no device opened.
#[cfg(windows)]
pub fn open() -> Result<Opened, String> {
    let (tell, told) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("orbistoun-audio".to_owned())
        .spawn(move || match device::start() {
            Ok((stream, opened)) => {
                let _ = tell.send(Ok(opened));
                // The stream plays for as long as it exists.
                let _stream = stream;
                loop {
                    std::thread::park();
                }
            }
            Err(e) => {
                let _ = tell.send(Err(e));
            }
        })
        .map_err(|e| format!("no audio thread: {e}"))?;
    told.recv()
        .unwrap_or_else(|_| Err("the audio thread stopped before it answered".to_owned()))
}

/// Opens nothing: this host's build has no audio device support.
///
/// # Errors
///
/// Always, saying so.
#[cfg(not(windows))]
pub fn open() -> Result<Opened, String> {
    Err("this build plays audio on Windows only".to_owned())
}

#[cfg(windows)]
mod device {
    use super::{FAILED, GUEST_RATE, Opened, mixer};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::atomic::Ordering;

    /// The default output device, playing the mixer: at the guest rate where the device offers
    /// it, at its own rate otherwise.
    pub(super) fn start() -> Result<(cpal::Stream, Opened), String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("the host has no output device")?;
        let default = device
            .default_output_config()
            .map_err(|e| format!("the output device has no format: {e}"))?;
        let guest_rate = cpal::SampleRate(GUEST_RATE);
        let chosen = device
            .supported_output_configs()
            .ok()
            .and_then(|mut ranges| {
                ranges.find(|range| {
                    range.channels() == default.channels()
                        && range.sample_format() == default.sample_format()
                        && (range.min_sample_rate()..=range.max_sample_rate()).contains(&guest_rate)
                })
            })
            .map_or(default, |range| range.with_sample_rate(guest_rate));
        let config = chosen.config();
        let opened = Opened {
            rate: config.sample_rate.0,
            channels: config.channels,
        };
        let channels = usize::from(config.channels);
        let rate = config.sample_rate.0;
        let failed = |e: cpal::StreamError| {
            eprintln!("orbistoun: the audio device stopped: {e}");
            FAILED.store(true, Ordering::Relaxed);
        };
        let stream = match chosen.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config,
                move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    mixer().fill(out, channels, rate);
                },
                failed,
                None,
            ),
            cpal::SampleFormat::I16 => {
                let mut mixed = Vec::new();
                device.build_output_stream(
                    &config,
                    move |out: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        mixed.resize(out.len(), 0.0_f32);
                        mixer().fill(&mut mixed, channels, rate);
                        for (to, &from) in out.iter_mut().zip(&mixed) {
                            *to = (from * 32767.0) as i16;
                        }
                    },
                    failed,
                    None,
                )
            }
            other => {
                return Err(format!(
                    "the output device plays {other}, which is not mixed"
                ));
            }
        }
        .map_err(|e| format!("the output device did not open: {e}"))?;
        stream
            .play()
            .map_err(|e| format!("the output device did not start: {e}"))?;
        Ok((stream, opened))
    }
}

#[cfg(test)]
mod tests {
    use super::{GUEST_RATE, MAX_QUEUED, Mixer, PREROLL, TARGET, wait_for};
    use std::time::Duration;

    /// Fills `frames` device frames of two channels at the guest rate.
    fn drain(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        mixer.fill(&mut out, 2, GUEST_RATE);
        out
    }

    /// A port is silent until it has a preroll queued, then plays its frames in order, and a
    /// mono port plays on both sides.
    #[test]
    fn a_port_plays_once_it_has_its_preroll() {
        let mut mixer = Mixer::new();
        mixer.push(1, 1, &[16384; PREROLL - 1]);
        assert!(drain(&mut mixer, 4).iter().all(|&s| s == 0.0), "not yet");
        mixer.push(1, 1, &[-16384]);
        assert_eq!(drain(&mut mixer, 1), [0.5, 0.5]);
        assert_eq!(mixer.ports[&1].frames.len(), PREROLL - 1);
    }

    /// Two ports sum, a stereo port keeps its sides apart, and the sum clips rather than wraps.
    #[test]
    fn ports_mix_and_clip() {
        let mut mixer = Mixer::new();
        let stereo: Vec<i16> = [i16::MAX, -8192].repeat(PREROLL);
        mixer.push(1, 2, &stereo);
        mixer.push(2, 1, &[i16::MAX; PREROLL]);
        let out = drain(&mut mixer, 1);
        assert!(
            (out[0] - 32767.0 / 32768.0).abs() < f32::EPSILON,
            "clipped, not wrapped"
        );
        assert!((out[1] - (32767.0 - 8192.0) / 32768.0).abs() < 1e-6);
    }

    /// A device at half the guest rate takes two guest frames per device frame; one at twice it
    /// holds each guest frame for two.
    #[test]
    fn the_device_rate_decides_how_fast_frames_are_taken() {
        let mut mixer = Mixer::new();
        mixer.push(1, 1, &[0; PREROLL * 2]);
        let mut out = vec![0.0; 10];
        mixer.fill(&mut out, 1, GUEST_RATE / 2);
        assert_eq!(mixer.ports[&1].frames.len(), PREROLL * 2 - 20);
        mixer.fill(&mut out, 1, GUEST_RATE * 2);
        assert_eq!(mixer.ports[&1].frames.len(), PREROLL * 2 - 25);
    }

    /// A port that runs dry waits for a fresh preroll, and a closed port is gone.
    #[test]
    fn a_dry_port_reprimes_and_a_closed_one_is_forgotten() {
        let mut mixer = Mixer::new();
        mixer.push(7, 1, &[100; PREROLL]);
        drain(&mut mixer, PREROLL + 4);
        assert!(!mixer.ports[&7].primed);
        mixer.push(7, 1, &[100; 8]);
        assert!(drain(&mut mixer, 4).iter().all(|&s| s == 0.0));
        mixer.close(7);
        assert!(!mixer.ports.contains_key(&7));
    }

    /// A port holds at most half a second, losing its oldest frames.
    #[test]
    fn a_full_queue_drops_its_oldest_frames() {
        let mut mixer = Mixer::new();
        mixer.push(1, 1, &vec![1; MAX_QUEUED]);
        assert_eq!(mixer.push(1, 1, &[2; 10]), MAX_QUEUED);
        assert_eq!(mixer.ports[&1].frames.back(), Some(&[2, 2]));
    }

    /// An output call waits only for what is queued past the target, and at most two chunks.
    #[test]
    fn a_port_waits_for_its_queue_to_come_back_to_the_target() {
        assert_eq!(wait_for(TARGET, 512), Duration::ZERO);
        assert_eq!(wait_for(TARGET + 480, 512), Duration::from_millis(10));
        assert_eq!(wait_for(MAX_QUEUED, 480), Duration::from_millis(20));
    }
}
