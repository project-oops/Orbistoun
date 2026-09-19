//! The frame-region transport: a rendered frame's bytes cross to the shim as a file the worker
//! writes and the shim reads, named by an [`Event::Frame`] descriptor (D695).
//!
//! D695 decided the route - bytes in a shared region named by a small descriptor, never pixels in a
//! message (D035) - and deliberately left the bulk mechanism open, asking for the simplest thing that
//! works. That is a file in a shared frames directory; D035 keeps it swappable if a measured need
//! ever calls for shared memory.
//!
//! **Write and read live together on purpose.** The region's layout - its name, and that the file
//! holds exactly the frame's bytes - is defined once, by the side that writes it. Two places
//! computing the name or the length differently is how a reader comes to disagree with the writer and
//! the mismatch reads as a corrupt frame forever (D084). So the shim reads through [`read_frame`]
//! here rather than reimplementing it.

use orbistoun_proto::{Event, FrameFormat};
use std::path::Path;

/// The name of the region file a frame's bytes are written to.
///
/// A bare name, not a path: the descriptor carries it and the shim resolves it against the same
/// directory, so a message never carries an absolute path either (D035).
#[must_use]
pub fn region_name(sequence: u64) -> String {
    format!("frame-{sequence}.bin")
}

/// Writes `bytes` as the frame for `sequence` into `dir`, and returns the descriptor that names it.
///
/// The bytes are the bulk; the returned [`Event::Frame`] is the small message that crosses the
/// protocol beside them.
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
    let region = region_name(sequence);
    std::fs::write(dir.join(&region), bytes)?;
    Ok(Event::Frame {
        width,
        height,
        format,
        sequence,
        region,
    })
}

/// Reads the bytes of the region a [`Event::Frame`] descriptor names, from `dir`.
///
/// The inverse of [`write_frame`].
///
/// # Errors
///
/// If `event` is not a [`Event::Frame`], if its `region` is not a bare name (a descriptor arriving
/// over the wire must not be able to name a path outside the frames directory), or if the region is
/// missing or unreadable.
pub fn read_frame(dir: &Path, event: &Event) -> std::io::Result<Vec<u8>> {
    let Event::Frame { region, .. } = event else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a frame event",
        ));
    };
    // The region is a name, not a path. A message is data, and this one may have come over the wire,
    // so a `region` with a separator or a parent component is refused rather than resolved - it could
    // otherwise read any file the process can (a traversal the descriptor has no business naming).
    if region.contains(['/', '\\']) || Path::new(region).components().count() != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "frame region is not a bare name",
        ));
    }
    std::fs::read(dir.join(region))
}

#[cfg(test)]
mod tests {
    use orbistoun_proto::{Event, FrameFormat};

    /// **A written frame reads its bytes back, and a corrupted region does not.**
    ///
    /// The whole of the frame crossing (D695): the worker writes bytes into a region and names it in
    /// a descriptor, and the shim reads exactly those bytes back through the same code. The negative
    /// half is load-bearing - overwriting the region must change what reads back, or the test would
    /// pass without the reader ever touching the file (principle 3).
    #[test]
    fn a_written_frame_reads_its_bytes_back_and_a_corrupted_region_does_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bytes: Vec<u8> = (0..=255_u8).cycle().take(4096).collect();

        let event =
            super::write_frame(dir.path(), 7, 32, 32, FrameFormat::Rgba8, &bytes).expect("write");
        assert!(
            matches!(&event, Event::Frame { sequence: 7, region, .. } if region == "frame-7.bin"),
            "the descriptor names its region and nothing more: {event:?}"
        );

        // Round-trip: the shim reads exactly what the worker wrote.
        assert_eq!(super::read_frame(dir.path(), &event).expect("read"), bytes);

        // Watched failing against a corrupted region: overwrite the bytes and they no longer match,
        // which proves the reader reads the region rather than echoing the descriptor.
        std::fs::write(dir.path().join("frame-7.bin"), b"corrupt").expect("corrupt");
        assert_ne!(
            super::read_frame(dir.path(), &event).expect("read"),
            bytes,
            "a corrupted region must not read back as the frame"
        );
    }

    /// **A descriptor whose region is not a bare name is refused, not resolved.**
    ///
    /// The descriptor is data and may arrive over the wire, so a `region` that walks out of the
    /// frames directory must be refused before it names a file the frame had no business naming.
    #[test]
    fn a_region_name_that_escapes_the_directory_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        for region in ["../secret", "sub/frame.bin", "/etc/passwd"] {
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
