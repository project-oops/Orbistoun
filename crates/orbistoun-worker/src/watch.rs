//! Snapshotting a region of guest memory and reporting what the guest changed in it.
//!
//! The cheap half of "what did the guest actually do to this structure?". A watchpoint
//! answers *which byte, when, and from which instruction* and costs a debug register plus
//! a per-platform API; this answers *which bytes ended up different* for one memcpy and no
//! platform code at all - and for "did anything ever fill this slot in?" that is the whole
//! answer (D223).

/// A region of guest memory as it was before the guest ran.
///
/// # Why a snapshot rather than a watchpoint
///
/// A hardware watchpoint says *which byte was touched, when, and by which instruction* -
/// and costs a debug register, a per-platform API to set it, and an exception per access.
/// A snapshot says *which bytes ended up different*, for one memcpy and no platform code.
///
/// For the question actually being asked at the wall - "did anything ever fill this slot
/// in?" - the second is the whole answer, and the first is a more expensive way to reach
/// it. The watchpoint earns its keep when *when* and *who* matter; this is what to reach
/// for first (D223).
static SNAPSHOT: std::sync::OnceLock<(u64, Vec<u8>)> = std::sync::OnceLock::new();

/// Copies a region of guest memory, to be compared against later.
///
/// Silent when the region cannot be read: a watch is a question somebody asked about an
/// address they typed, and an address that is not mapped is an ordinary mistake rather
/// than a reason to end the run. The comparison then reports that nothing was captured.
pub fn snapshot(base: u64, len: u64) {
    let (Ok(at), Ok(len)) = (usize::try_from(base), usize::try_from(len)) else {
        return;
    };
    // SAFETY: the caller supplies an address in the guest's own mapping, which is the
    // identity mapping this process made, and the region is read before the guest starts -
    // so nothing else is writing it. An address outside it faults here exactly as it would
    // have faulted in the guest, and the fault reporter names it.
    let bytes = unsafe {
        std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(at), len).to_vec()
    };
    let _ = SNAPSHOT.set((base, bytes));
}

/// What changed in the watched region, as lines a person reads.
///
/// Reported per eight-byte word rather than per byte, because guest structures are made of
/// words and a byte-level diff of a pointer being written is eight lines saying the same
/// thing.
///
/// **A word that did not change is as interesting as one that did**, which is why the
/// unchanged count is reported rather than only the differences: the question at the wall
/// is which slot nobody filled in.
pub fn changes() -> Vec<String> {
    let Some((base, before)) = SNAPSHOT.get() else {
        return Vec::new();
    };
    let Ok(at) = usize::try_from(*base) else {
        return Vec::new();
    };
    // SAFETY: the same region snapshot read, still mapped - the guest has faulted or run
    // out of time, but its address space is this process's and is intact until it exits.
    let after = unsafe {
        std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(at), before.len())
    };

    let mut lines = Vec::new();
    let mut unchanged = 0_usize;
    for (index, (was, now)) in before.chunks(8).zip(after.chunks(8)).enumerate() {
        if was == now {
            unchanged += 1;
            continue;
        }
        let offset = index * 8;
        lines.push(format!(
            "  {:#x}  {} -> {}",
            base.saturating_add(offset as u64),
            word(was),
            word(now)
        ));
    }
    if lines.is_empty() {
        lines.push(format!(
            "  nothing in {:#x}+{:#x} changed while the guest ran",
            base,
            before.len()
        ));
    } else {
        lines.push(format!("  ({unchanged} word(s) unchanged)"));
    }
    lines
}

/// Eight bytes as a little-endian word, or as bytes when they are not a whole one.
fn word(bytes: &[u8]) -> String {
    if bytes.len() == 8 {
        let mut value = [0_u8; 8];
        value.copy_from_slice(bytes);
        return format!("{:#018x}", u64::from_le_bytes(value));
    }
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
