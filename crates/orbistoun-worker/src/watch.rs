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
static SNAPSHOT: std::sync::OnceLock<(u64, Option<Vec<u8>>)> = std::sync::OnceLock::new();

/// How long a watched region is, kept for the case where there was nothing to snapshot.
///
/// The length is the caller's question either way, and a region that did not exist at entry
/// has no captured bytes to take it from.
static WATCHED_LEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Copies a region of guest memory, to be compared against later.
///
/// # A region that is not there yet is a question, not a mistake
///
/// This read the address unconditionally, and its own documentation said it did not: *"silent
/// when the region cannot be read"*, above a raw dereference that faulted. An address the guest
/// maps at runtime does not exist at entry, so watching the structure a call was handed - the
/// thing the diagnostic is for - **killed the run before the guest started**, with a fault
/// report naming the emulator's own read.
///
/// A doc promising what the branch below it never did is the same failure as a message naming a
/// cause nothing measured, one layer down (principle 3, D385). Both halves are fixed rather
/// than the comment: a region absent at entry is recorded as absent, and [`changes`] reports
/// what appeared there instead of a diff it cannot make (D580).
pub fn snapshot(base: u64, len: u64) {
    let (Ok(at), Ok(size)) = (usize::try_from(base), usize::try_from(len)) else {
        return;
    };
    WATCHED_LEN.store(len, std::sync::atomic::Ordering::Relaxed);
    if !orbistoun_thunk::readable_span(base, len) {
        // Asked for and not there yet, which is a finding rather than an error.
        let _ = SNAPSHOT.set((base, None));
        return;
    }
    // SAFETY: `readable_span` has just said the whole of `[base, base + len)` is inside a
    // span this process mapped, and the region is read before the guest starts - so nothing
    // else is writing it.
    let bytes = unsafe {
        std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(at), size).to_vec()
    };
    let _ = SNAPSHOT.set((base, Some(bytes)));
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
    let len = match before {
        Some(bytes) => bytes.len() as u64,
        None => WATCHED_LEN.load(std::sync::atomic::Ordering::Relaxed),
    };
    // Whether it can be read *now*, which for a region the guest mapped while it ran is a
    // different answer from the one at entry - and is the answer that decides what can be
    // said about it.
    // **"Published as readable", not "mapped", because that is the question that was asked.**
    // The two came apart for months: every mapping a guest made at runtime was absent from the
    // published list, so a pointer into an ordinary heap structure read as though the address
    // were wrong. The tool can establish that nobody told it about the span and cannot
    // establish that nothing is there (D580).
    if !orbistoun_thunk::readable_span(*base, len) {
        return vec![format!(
            "  {base:#x}+{len:#x} is in no span this run published as readable"
        )];
    }
    let Ok(size) = usize::try_from(len) else {
        return Vec::new();
    };
    // SAFETY: `readable_span` has just said the whole window is inside a span this process
    // mapped. The guest has faulted or run out of time, but its address space is this
    // process's and is intact until it exits.
    let after =
        unsafe { std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(at), size) };

    // **Appeared while the guest ran, so there is no diff to report and the contents are the
    // finding.** This is the case the structures worth watching are almost always in: a
    // command buffer, a descriptor, anything an allocator handed the guest is mapped after
    // entry, and a diagnostic that answered "nothing was captured" for all of them was
    // answering the wrong question (D580).
    let Some(before) = before else {
        let mut lines = vec![format!(
            "  {base:#x}+{len:#x} did not exist when the guest started"
        )];
        lines.extend(after.chunks(8).enumerate().map(|(index, now)| {
            format!(
                "  {:#x}  {}",
                base.saturating_add((index * 8) as u64),
                word(now)
            )
        }));
        return lines;
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
        // **And what it holds, because "nothing changed" is only half an answer.** A region that
        // existed at entry got a diff and nothing else, so a watch could say a structure was
        // untouched and not what was in it - and for anything the loader placed, which is every
        // module the title ships, the contents are the question. Reading guest material at rest
        // is `static` evidence and always was; the tool simply would not show it (D593).
        lines.push(format!(
            "  nothing in {:#x}+{:#x} changed while the guest ran; it holds",
            base,
            before.len()
        ));
        lines.extend(after.chunks(8).enumerate().map(|(index, now)| {
            format!(
                "  {:#x}  {}",
                base.saturating_add((index * 8) as u64),
                word(now)
            )
        }));
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

#[cfg(test)]
mod tests {
    /// **A region that is not there yet must not be dereferenced.**
    ///
    /// This is the failure the module documented and did not have: the doc said *"silent when
    /// the region cannot be read"* above a raw dereference, so watching the structure a call
    /// was handed killed the run before the guest started. Asserted against the filter rather
    /// than by faulting, because a test that reproduced the bug would take the suite with it.
    #[test]
    fn an_address_nothing_published_is_refused_rather_than_read() {
        // Nothing has published anything in this process, which is the state `snapshot` runs
        // in for an address the guest has not mapped yet.
        assert!(
            !orbistoun_thunk::readable_span(0x7400_0089_D210, 0x40),
            "an unpublished address must not be readable, or `snapshot` dereferences it"
        );
        assert!(
            !orbistoun_thunk::readable_span(u64::MAX, 8),
            "a window that overflows is not inside anything"
        );
    }

    /// A watch nobody asked for says nothing, rather than reporting an empty region.
    #[test]
    fn no_watch_is_no_output() {
        assert!(
            super::changes().is_empty(),
            "a run with no watch must not print a region"
        );
    }
}
