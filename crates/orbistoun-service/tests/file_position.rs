//! `018-relational/file-position-tracks-reads`, the last measured relation.
//!
//! # Why this is a file of its own
//!
//! It mounts a directory, and the mount table is process-wide - the first test to touch it
//! decides what every other test in the binary sees. `measured_relations.rs` deliberately
//! exercises nothing that needs one, so this goes beside it rather than in it.
//!
//! # Why it was unreachable, and why it is not
//!
//! D545 recorded this relation as uncovered: nothing opens in a bare service test, because
//! `/app0`, a host path and `/dev/stdout` all answer ENOENT when the filesystem has no mounted
//! title. That was true and was not the whole picture - `orbistoun_fs::mount` is public and
//! `descriptor.rs`'s own tests already stand a title up with it. The relation needed a mount,
//! not a running guest (D547).

use orbistoun_core::GUEST_ARG_REGISTERS;

/// What the file holds. Sixteen bytes of distinguishable content, so a read that started in
/// the wrong place is visible in the bytes rather than only in a length.
const CONTENTS: &[u8] = b"0123456789abcdef";

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let found = orbistoun_service::implementation_named(name)
        .unwrap_or_else(|| panic!("{name} is not implemented, so the relation cannot be checked"));
    found(&args)
}

/// **A second read continues where the first stopped.**
///
/// # What this asserts
///
/// That a descriptor carries a position and that reading advances it: two four-byte reads of a
/// sixteen-byte file return `0123` then `4567`, and a third read after `sceKernelLseek` back to
/// zero returns `0123` again. The bytes are checked, not just the counts - a descriptor that
/// re-read the start every time would return the right *length* twice and the wrong content,
/// which is the failure this relation is about.
///
/// The final read is the one that makes it a relation rather than two independent calls: it
/// shows the position is a thing that can be moved, not merely a counter that goes up.
///
/// # What it cannot assert
///
/// **That the console's `0x20` means what this checks.** The measurement is a number whose
/// meaning lives in obSCEne's C source - very likely a byte count, given a sixteen-byte file
/// read twice, but that is a reading and not a citation, so no number here is compared to it.
/// This asserts the property the check's *name* states, which is the D545 rule.
///
/// And nothing about concurrency: one thread, one descriptor. Two threads sharing a descriptor
/// is a different question and this says nothing about it.
#[test]
fn a_second_read_continues_where_the_first_stopped() {
    let root = std::env::temp_dir().join("orbistoun-file-position");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a directory to mount");
    std::fs::write(root.join("data.bin"), CONTENTS).expect("a file to read");
    orbistoun_fs::mount::clear();
    orbistoun_fs::mount::mount(orbistoun_fs::mount::APP_MOUNT, root.clone());

    let path = std::ffi::CString::new("/app0/data.bin").expect("no interior nul");
    let fd = call("sceKernelOpen", [path.as_ptr() as u64, 0, 0, 0, 0, 0]);
    assert!(
        (fd as i64) > 0,
        "the mounted file did not open ({fd:#x}), so nothing below is about file position"
    );

    let mut buffer = [0u8; 4];
    let at = buffer.as_mut_ptr() as u64;
    let mut read_four = |what: &str| {
        buffer = [0; 4];
        let n = call("sceKernelRead", [fd, at, 4, 0, 0, 0]);
        assert_eq!(n, 4, "{what}: asked for four bytes and got {n}");
        buffer
    };

    assert_eq!(&read_four("the first read")[..], b"0123");
    assert_eq!(
        &read_four("the second read")[..],
        b"4567",
        "the second read returned the start of the file again, so the descriptor keeps no \
         position and every read is the first one"
    );

    // Moving the position, which is what makes it a position rather than a counter.
    assert_eq!(
        call("sceKernelLseek", [fd, 0, 0, 0, 0, 0]),
        0,
        "seeking back to the start of a file this test just opened"
    );
    assert_eq!(
        &read_four("the read after seeking back")[..],
        b"0123",
        "a read after seeking to zero did not start at zero"
    );

    assert_eq!(call("sceKernelClose", [fd, 0, 0, 0, 0, 0]), 0);
    orbistoun_fs::mount::clear();
    let _ = std::fs::remove_dir_all(&root);
}
