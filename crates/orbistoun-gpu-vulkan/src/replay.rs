//! Runs a captured guest dispatch ([`crate::capture`]) again alone, as often as asked: each run's
//! time on the device from its timestamps, whether it lost the device, and a hash of everything it
//! left, so two builds are compared on one dispatch in seconds rather than through a title's run.

use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::capture::{Capture, CapturedBuffer};
use crate::compute::{DispatchError, GuestDispatch};
use crate::compute_buffers::BufferOut;
use crate::compute_images::DispatchImages;

/// What one run of a replayed dispatch left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    /// [`output_hash`] of everything it wrote.
    pub hash: u64,
    /// Whether an access left the window or went through a descriptor not modelled.
    pub escaped: bool,
    /// The loop the module cut for running past its cap, where it cut one.
    pub loop_cut: Option<u32>,
}

/// One run of a replayed dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayRun {
    /// Its time on the device, between its timestamps; `None` where the queue takes none or it
    /// failed.
    pub device_time: Option<Duration>,
    /// Its time on the host, from the call to the read-back, a first run's pipeline build with it.
    pub host_time: Duration,
    /// What it left, or why it failed.
    pub outcome: Result<RunOutput, String>,
}

/// A replayed dispatch's runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    /// The guest program it was captured from, where it was noted with one.
    pub program: Option<u64>,
    /// Workgroups.
    pub groups: [u32; 3],
    /// The bytes the capture holds.
    pub bytes: u64,
    /// Each run, in order. Runs stop at the first that loses the device.
    pub runs: Vec<ReplayRun>,
    /// Whether a run lost the device.
    pub lost: bool,
}

/// Runs the dispatch captured in `dir` `repeat` times, stopping at a lost device, and writes what
/// the last run that answered left into `out` where it is given ([`outputs`]).
///
/// # Errors
///
/// The capture cannot be read, or `out` cannot be written. A run that fails is a run's outcome,
/// not an error.
pub fn replay_dispatch(
    dir: &Path,
    repeat: u32,
    out: Option<&Path>,
) -> Result<Replay, DispatchError> {
    let capture = Capture::read_from(dir)?;
    let (fetched, more_fetched, stored) = capture.dispatch_images();
    let images = DispatchImages {
        fetched,
        more_fetched: &more_fetched,
        stored: &stored,
    };
    let buffers = capture.buffers_in();
    let hash = orbistoun_gpu::content_hash(&capture.module);
    let mut runs = Vec::new();
    let mut lost = false;
    let mut latest = None;
    for _ in 0..repeat.max(1) {
        // Named, so a loss in the replay names the program as the run's would.
        if let Some(program) = capture.program {
            crate::device_lost::note_dispatch(program, capture.groups);
        }
        let _ = crate::compute::take_device_time();
        let started = Instant::now();
        let dispatched = crate::compute::run_guest_dispatch(
            (&capture.module, hash),
            &capture.memory,
            (&capture.push, capture.groups),
            &images,
            &buffers,
        );
        let host_time = started.elapsed();
        let run_lost = crate::device_lost::device_lost()
            || dispatched.as_ref().is_err_and(crate::capture::is_loss);
        runs.push(ReplayRun {
            device_time: crate::compute::take_device_time(),
            host_time,
            outcome: dispatched
                .map(|done| {
                    let answered = RunOutput {
                        hash: output_hash(&done),
                        escaped: done.escaped,
                        loop_cut: done.loop_cut,
                    };
                    latest = Some(done);
                    answered
                })
                .map_err(|e| e.to_string()),
        });
        if run_lost {
            lost = true;
            break;
        }
    }
    if let (Some(out), Some(done)) = (out, &latest) {
        write_outputs(out, &outputs(&capture, done))
            .map_err(|e| DispatchError::Capture(format!("{}: {e}", out.display())))?;
    }
    Ok(Replay {
        program: capture.program,
        groups: capture.groups,
        bytes: capture.bytes(),
        runs,
        lost,
    })
}

/// What a replayed dispatch left, as files named as its capture names its inputs, so an output is
/// compared with the input of the same name, or with a later dispatch's: `memory.bin`, each
/// buffer that has its own as `buffer-<slot>.bin` - an alias's words are its holder's - and each
/// stored image as `stored-<n>.bin`. A buffer whose stores the device marked has them as
/// `buffer-<slot>.written.bin`, a word for each word, non-zero where it stored.
pub fn outputs(capture: &Capture<'_>, done: &GuestDispatch) -> Vec<(String, Vec<u32>)> {
    let mut files = vec![("memory.bin".to_owned(), done.memory.clone())];
    for (slot, (before, after)) in capture.buffers.iter().zip(&done.buffers).enumerate() {
        let length = match before {
            CapturedBuffer::Words(words) => words.len(),
            CapturedBuffer::Unread(words) => *words,
            CapturedBuffer::Alias(..) => continue,
        };
        let (words, written) = buffer_after(after, length);
        files.push((format!("buffer-{slot}.bin"), words));
        if let Some(written) = written {
            files.push((format!("buffer-{slot}.written.bin"), written));
        }
    }
    for (index, texels) in done.stored.iter().enumerate() {
        files.push((format!("stored-{index}.bin"), texels.clone()));
    }
    files
}

/// A buffer's `length` words after a dispatch, whichever form the device answered them in, and its
/// write mask where the device kept one.
fn buffer_after(after: &BufferOut, length: usize) -> (Vec<u32>, Option<Vec<u32>>) {
    if let Some((at, values)) = &after.stored {
        let mut words = vec![0; length];
        let mut written = vec![0; length];
        for (&index, &value) in at.iter().zip(values) {
            if let Some(word) = words.get_mut(index as usize) {
                *word = value;
                written[index as usize] = 1;
            }
        }
        return (words, Some(written));
    }
    if let Some(value) = after.whole {
        return (vec![value; length], Some(vec![1; length]));
    }
    let written = (!after.written.is_empty()).then(|| after.written.clone());
    (after.words.clone(), written)
}

/// Writes `files` into `dir`, made if missing.
fn write_outputs(dir: &Path, files: &[(String, Vec<u32>)]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, words) in files {
        std::fs::write(
            dir.join(name),
            zerocopy::IntoBytes::as_bytes(words.as_slice()),
        )?;
    }
    Ok(())
}

/// One hash of everything a dispatch left: guest memory, each stored image and each buffer, and
/// its escape and cut-loop flags.
pub fn output_hash(done: &GuestDispatch) -> u64 {
    let mut parts = vec![
        orbistoun_gpu::content_hash(&done.memory),
        u64::from(done.escaped),
        done.loop_cut.map_or(u64::MAX, u64::from),
    ];
    parts.extend(
        done.stored
            .iter()
            .map(|image| orbistoun_gpu::content_hash(image)),
    );
    for buffer in &done.buffers {
        parts.push(orbistoun_gpu::content_hash(&buffer.words));
        parts.push(orbistoun_gpu::content_hash(&buffer.written));
        if let Some((at, values)) = &buffer.stored {
            parts.push(orbistoun_gpu::content_hash(at));
            parts.push(orbistoun_gpu::content_hash(values));
        }
        parts.push(buffer.whole.map_or(u64::MAX, u64::from));
    }
    let words: Vec<u32> = parts
        .iter()
        .flat_map(|part| [*part as u32, (part >> 32) as u32])
        .collect();
    orbistoun_gpu::content_hash(&words)
}

impl std::fmt::Display for Replay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let named = self.program.map_or_else(
            || "a module".to_owned(),
            |p| format!("the program at {p:#x}"),
        );
        writeln!(
            f,
            "replayed the dispatch of {named} over {:?} groups ({} bytes captured)",
            self.groups, self.bytes
        )?;
        for (index, run) in self.runs.iter().enumerate() {
            let mut line = format!("run {}: ", index + 1);
            let _ = match run.device_time {
                Some(took) => write!(line, "device {:.3} ms", took.as_secs_f64() * 1000.0),
                None => write!(line, "device -"),
            };
            let _ = write!(
                line,
                ", host {:.3} ms",
                run.host_time.as_secs_f64() * 1000.0
            );
            let _ = match &run.outcome {
                Ok(out) => write!(
                    line,
                    ", output {:016x}{}{}",
                    out.hash,
                    if out.escaped { ", escaped" } else { "" },
                    out.loop_cut
                        .map(|at| format!(", cut the loop at {at:#x}"))
                        .unwrap_or_default()
                ),
                Err(e) => write!(line, ", failed: {e}"),
            };
            writeln!(f, "{line}")?;
        }
        if self.lost {
            writeln!(f, "the device was lost")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{output_hash, outputs};
    use crate::capture::{Capture, CapturedBuffer, Reason};
    use crate::compute::GuestDispatch;
    use crate::compute_buffers::BufferOut;
    use std::borrow::Cow;

    /// Each output is a file named as its input is, whole whatever form the device answered it
    /// in: a read buffer's words, an unread buffer's stores spread over its length with their mask,
    /// a cleared one's value everywhere; an alias has none of its own.
    #[test]
    fn outputs_are_whole_and_named_as_their_inputs() {
        let capture = Capture {
            buffers: vec![
                CapturedBuffer::Words(Cow::Owned(vec![1, 2, 3])),
                CapturedBuffer::Unread(4),
                CapturedBuffer::Alias(0, 4),
                CapturedBuffer::Unread(2),
                CapturedBuffer::Unread(2),
            ],
            ..Capture::of(
                (None, Reason::Asked),
                &[],
                &[],
                (&[], [1, 1, 1]),
                &crate::compute_images::DispatchImages::default(),
                &[],
            )
        };
        let plain = |words: Vec<u32>, written: Vec<u32>| BufferOut {
            words,
            written,
            stored: None,
            whole: None,
        };
        let after = GuestDispatch {
            memory: vec![9],
            stored: vec![vec![4, 4]],
            buffers: vec![
                plain(vec![1, 5, 3], Vec::new()),
                BufferOut {
                    stored: Some((vec![1, 3], vec![7, 8])),
                    ..plain(Vec::new(), Vec::new())
                },
                plain(vec![1, 5, 3], Vec::new()),
                BufferOut {
                    whole: Some(6),
                    ..plain(Vec::new(), Vec::new())
                },
                plain(vec![0, 2], vec![0, 1]),
            ],
            ..done()
        };
        assert_eq!(
            outputs(&capture, &after),
            [
                ("memory.bin".to_owned(), vec![9]),
                ("buffer-0.bin".to_owned(), vec![1, 5, 3]),
                ("buffer-1.bin".to_owned(), vec![0, 7, 0, 8]),
                ("buffer-1.written.bin".to_owned(), vec![0, 1, 0, 1]),
                ("buffer-3.bin".to_owned(), vec![6, 6]),
                ("buffer-3.written.bin".to_owned(), vec![1, 1]),
                ("buffer-4.bin".to_owned(), vec![0, 2]),
                ("buffer-4.written.bin".to_owned(), vec![0, 1]),
                ("stored-0.bin".to_owned(), vec![4, 4]),
            ]
        );
    }

    fn done() -> GuestDispatch {
        GuestDispatch {
            memory: vec![1, 2, 3],
            escaped: false,
            loop_cut: None,
            stored: vec![vec![4, 5]],
            buffers: Vec::new(),
        }
    }

    /// The same output hashes the same, and a change to any part of it changes the hash.
    #[test]
    fn the_output_hash_follows_every_part() {
        let base = output_hash(&done());
        assert_eq!(base, output_hash(&done()));
        let mut memory = done();
        memory.memory[2] = 0;
        let mut stored = done();
        stored.stored[0][0] = 0;
        let escaped = GuestDispatch {
            escaped: true,
            ..done()
        };
        let cut = GuestDispatch {
            loop_cut: Some(0),
            ..done()
        };
        for changed in [memory, stored, escaped, cut] {
            assert_ne!(output_hash(&changed), base, "{changed:?}");
        }
    }
}
