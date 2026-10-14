//! The frame-region transport: a rendered frame's bytes cross to the shim as a file the worker
//! writes and the shim reads, named by an [`Event::Frame`] descriptor (D695).
//!
//! The bytes live in a file under the shared traces directory, never in a message. Each run writes
//! into a directory of its own there, named by its title and run (D865): runs in several sessions
//! share the traces directory, and a frame must always be traceable to the run that drew it.
//! Writing and reading live together so the region's name and length are defined once: the shim
//! reads through [`read_frame`] rather than reimplementing it, and cannot disagree with the writer.

use orbistoun_proto::{Event, FrameFormat};
use orbistoun_report::RunId;
use std::path::{Component, Path, PathBuf};

/// The runs of one title whose frames are kept; an older run's directory goes when a run starts.
/// A run leaves up to a ring of frames, 8 MB each at 1080p and 32 MB at 2160p.
pub const KEPT_RUNS: usize = 4;

/// The name of the region file a frame's bytes are written to, within its run's directory.
#[must_use]
pub fn region_name(sequence: u64) -> String {
    format!("frame-{sequence}.bin")
}

/// Where the run `run` of `module` writes its frame regions: `frames/<title>/<run>/` under
/// `traces`, the title named as its trace is (D865).
#[must_use]
pub fn run_dir(traces: &Path, module: &str, run: &RunId) -> PathBuf {
    let trace = orbistoun_report::trace::trace_file_name(module);
    traces
        .join("frames")
        .join(trace.trim_end_matches(".json"))
        .join(run.as_str())
}

/// Removes the frame directories of the title `run` belongs to, all but the newest [`KEPT_RUNS`]
/// (run ids sort chronologically). Best effort: a directory that cannot be removed is left.
pub fn prune_runs(run: &Path) {
    let Some(title) = run.parent() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(title) else {
        return;
    };
    let mut runs: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    runs.sort();
    let surplus = runs.len().saturating_sub(KEPT_RUNS);
    for old in runs.iter().take(surplus).filter(|old| old.as_path() != run) {
        if let Err(e) = std::fs::remove_dir_all(old) {
            tracing::debug!("an old run's frames at {} were kept: {e}", old.display());
        }
    }
}

/// Writes `bytes` as the frame for `sequence` into `dir`, and returns the descriptor that names it
/// by its full path.
///
/// The bytes are the bulk; the returned [`Event::Frame`] is the small message that crosses the
/// protocol beside them. `dir` is made if it is missing, so a run whose directory another run
/// pruned keeps writing.
///
/// # Errors
///
/// If the region file cannot be written.
pub fn write_frame(
    dir: &Path,
    sequence: u64,
    width: u32,
    height: u32,
    format: FrameFormat,
    bytes: &[u8],
) -> std::io::Result<Event> {
    let path = dir.join(region_name(sequence));
    std::fs::write(&path, bytes).or_else(|e| {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e);
        }
        std::fs::create_dir_all(dir)?;
        std::fs::write(&path, bytes)
    })?;
    Ok(Event::Frame {
        width,
        height,
        format,
        sequence,
        region: std::path::absolute(&path)?.to_string_lossy().into_owned(),
    })
}

/// Reads the bytes of the region a [`Event::Frame`] descriptor names, which must lie within `dir`.
///
/// The inverse of [`write_frame`].
///
/// # Errors
///
/// If `event` is not a [`Event::Frame`], if its `region` is not within `dir` (a descriptor arriving
/// over the wire must not be able to name a path outside the traces directory), or if the region
/// is missing or unreadable.
pub fn read_frame(dir: &Path, event: &Event) -> std::io::Result<Vec<u8>> {
    let Event::Frame { region, .. } = event else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a frame event",
        ));
    };
    // A message may have come over the wire, so a `region` outside `dir`, or with a parent
    // component, is refused rather than resolved: it could otherwise name any file the process can
    // read.
    let dir = std::path::absolute(dir)?;
    let path = dir.join(region);
    let within = path
        .strip_prefix(&dir)
        .is_ok_and(|rest| rest.components().all(|c| matches!(c, Component::Normal(_))));
    if !within {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "frame region is not within the traces directory",
        ));
    }
    std::fs::read(path)
}

#[cfg(test)]
mod tests {
    use orbistoun_proto::{Event, FrameFormat};
    use orbistoun_report::RunId;

    /// A written frame reads its bytes back, and a corrupted region does not, so the reader is
    /// shown to read the file rather than echo the descriptor.
    #[test]
    fn a_written_frame_reads_its_bytes_back_and_a_corrupted_region_does_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bytes: Vec<u8> = (0..=255_u8).cycle().take(4096).collect();

        let event =
            super::write_frame(dir.path(), 7, 32, 32, FrameFormat::Rgba8, &bytes).expect("write");
        let named = dir
            .path()
            .join("frame-7.bin")
            .to_string_lossy()
            .into_owned();
        assert!(
            matches!(&event, Event::Frame { sequence: 7, region, .. } if *region == named),
            "the descriptor names its region and nothing more: {event:?}"
        );

        assert_eq!(super::read_frame(dir.path(), &event).expect("read"), bytes);

        std::fs::write(dir.path().join("frame-7.bin"), b"corrupt").expect("corrupt");
        assert_ne!(
            super::read_frame(dir.path(), &event).expect("read"),
            bytes,
            "a corrupted region must not read back as the frame"
        );
    }

    /// Two runs writing the same sequence number keep their own frames: concurrent runs share the
    /// traces directory, and one title's frame once overwrote another's there (D865).
    #[test]
    fn two_runs_frames_do_not_collide() {
        let traces = tempfile::tempdir().expect("tempdir");
        let gta = super::run_dir(
            traces.path(),
            "/titles/PPSA04263/eboot.bin",
            &RunId::new(1_760_000_000_000, 0x10),
        );
        let neverball = super::run_dir(
            traces.path(),
            "/titles/NVRB00001/eboot.bin",
            &RunId::new(1_760_000_060_000, 0x20),
        );
        let again = super::run_dir(
            traces.path(),
            "/titles/PPSA04263/eboot.bin",
            &RunId::new(1_760_000_060_000, 0x30),
        );

        let first = vec![1_u8; 16];
        let second = vec![2_u8; 16];
        let third = vec![3_u8; 16];
        let a =
            super::write_frame(&gta, 1_000_000, 2, 2, FrameFormat::Rgba8, &first).expect("write");
        let b = super::write_frame(&neverball, 1_000_000, 2, 2, FrameFormat::Rgba8, &second)
            .expect("write");
        let c =
            super::write_frame(&again, 1_000_000, 2, 2, FrameFormat::Rgba8, &third).expect("write");

        let region = |event: &Event| match event {
            Event::Frame { region, .. } => region.clone(),
            other => panic!("not a frame: {other:?}"),
        };
        assert_ne!(region(&a), region(&b), "two titles' regions differ");
        assert_ne!(region(&a), region(&c), "two runs of one title differ");
        assert_eq!(super::read_frame(traces.path(), &a).expect("read"), first);
        assert_eq!(super::read_frame(traces.path(), &b).expect("read"), second);
        assert_eq!(super::read_frame(traces.path(), &c).expect("read"), third);

        // The region names the file in full, and the path says which title and run drew it.
        let named = region(&a);
        assert!(
            std::path::Path::new(&named).is_absolute(),
            "the region is a full path: {named}"
        );
        assert!(
            named.contains("PPSA04263") && named.contains("1760000000000-0010"),
            "the region names its title and run: {named}"
        );
    }

    /// A title keeps the frames of its last few runs, newest first, and drops the rest (D865).
    #[test]
    fn a_title_keeps_its_last_runs_frames() {
        let traces = tempfile::tempdir().expect("tempdir");
        let module = "/titles/PPSA04263/eboot.bin";
        let runs: Vec<_> = (0..super::KEPT_RUNS as u64 + 2)
            .map(|n| super::run_dir(traces.path(), module, &RunId::new(1_760_000_000_000 + n, 0)))
            .collect();
        for dir in &runs {
            super::write_frame(dir, 1, 1, 1, FrameFormat::Rgba8, &[0; 4]).expect("write");
        }
        // Another title's run is not this title's to prune.
        let other = super::run_dir(
            traces.path(),
            "/titles/NVRB00001/eboot.bin",
            &RunId::new(1, 0),
        );
        super::write_frame(&other, 1, 1, 1, FrameFormat::Rgba8, &[0; 4]).expect("write");

        let newest = runs.last().expect("runs");
        super::prune_runs(newest);

        let (dropped, kept) = runs.split_at(runs.len() - super::KEPT_RUNS);
        assert!(
            dropped.iter().all(|dir| !dir.exists()),
            "older runs dropped"
        );
        assert!(kept.iter().all(|dir| dir.exists()), "the last runs kept");
        assert!(other.exists(), "another title's run kept");
    }

    /// A descriptor whose region is outside the frames directory is refused, not resolved.
    #[test]
    fn a_region_name_that_escapes_the_directory_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = dir
            .path()
            .parent()
            .expect("a parent")
            .join("frame-0.bin")
            .to_string_lossy()
            .into_owned();
        for region in ["../secret", "sub/../../secret", "/etc/passwd", &outside] {
            let event = Event::Frame {
                width: 1,
                height: 1,
                format: FrameFormat::Rgba8,
                sequence: 0,
                region: region.to_owned(),
            };
            assert!(
                super::read_frame(dir.path(), &event).is_err(),
                "{region} must be refused"
            );
        }
    }

    /// A non-frame event has no region to read, and says so rather than guessing.
    #[test]
    fn reading_a_non_frame_event_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(
            super::read_frame(
                dir.path(),
                &Event::Failed {
                    error: "x".to_owned()
                }
            )
            .is_err()
        );
    }
}
