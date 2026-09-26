//! Snapshotting a region of guest memory and reporting what the guest changed in it.
//!
//! A watchpoint answers which byte changed, when, and from which instruction, at the cost of a
//! debug register and platform code. A snapshot answers which bytes ended up different, for one
//! copy and no platform code, which is the whole answer to "did anything fill this slot in?".

/// A region of guest memory as it was before the guest ran, or `None` when it was not readable
/// then.
static SNAPSHOT: std::sync::OnceLock<(u64, Option<Vec<u8>>)> = std::sync::OnceLock::new();

/// How long a watched region is, kept for the case where there was nothing to snapshot.
///
/// A region that did not exist at entry has no captured bytes to take the length from.
static WATCHED_LEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Copies a region of guest memory, to be compared against later.
///
/// A region the guest maps at run time does not exist at entry. It is recorded as absent rather
/// than read, and [`changes`] reports what appeared there instead of a diff.
pub fn snapshot(base: u64, len: u64) {
    let (Ok(at), Ok(size)) = (usize::try_from(base), usize::try_from(len)) else {
        return;
    };
    WATCHED_LEN.store(len, std::sync::atomic::Ordering::Relaxed);
    if !orbistoun_thunk::readable_span(base, len) {
        // Not there yet, which is a finding rather than an error.
        let _ = SNAPSHOT.set((base, None));
        return;
    }
    // SAFETY: `readable_span` says all of `[base, base + len)` is inside a span this process
    // mapped, and it is read before the guest starts, so nothing else is writing it.
    let bytes = unsafe {
        std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(at), size).to_vec()
    };
    let _ = SNAPSHOT.set((base, Some(bytes)));
}

/// What changed in the watched region, as lines a person reads.
///
/// Reported per eight-byte word, because guest structures are made of words. The unchanged
/// count is reported too, since the question is often which slot nobody filled in.
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
    // Whether it can be read now, which differs from entry for a region the guest mapped. The
    // report says "published as readable", not "mapped": the tool knows whether it was told about
    // the span, not whether nothing is there.
    if !orbistoun_thunk::readable_span(*base, len) {
        return vec![format!(
            "  {base:#x}+{len:#x} is in no span this run published as readable"
        )];
    }
    let Ok(size) = usize::try_from(len) else {
        return Vec::new();
    };
    // SAFETY: `readable_span` says the whole window is inside a span this process mapped, and
    // the guest's address space is this process's and intact until it exits.
    let after =
        unsafe { std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(at), size) };

    // Appeared while the guest ran, as anything an allocator handed it does: there is no diff, so
    // the contents are the finding.
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
        // Nothing changed, so the contents are shown: for anything the loader placed they are the
        // question.
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
    /// An address nothing published is refused rather than dereferenced; asserted against the
    /// filter, since a fault would take the suite with it.
    #[test]
    fn an_address_nothing_published_is_refused_rather_than_read() {
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
