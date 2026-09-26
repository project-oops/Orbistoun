//! Applying each segment's declared access once its bytes are in place.
//!
//! An image is placed as one read-write span and re-protected here after relocation, so
//! guest text is writable only while it is built. Segment boundaries are not page
//! boundaries: a page shared by two segments gets the union of their access, since
//! protecting segments in turn would strip what the earlier one needs. [`page_protections`]
//! computes that per page and merges the result into runs.

use std::collections::BTreeMap;

use orbistoun_mem::Protection;

use crate::{Image, LoadError, PlacedSegment};

/// A contiguous range that ends up with one protection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtectionRun {
    /// First address in the run.
    pub base: u64,
    /// Length in bytes, always whole pages.
    pub len: u64,
    /// Access applied to it.
    pub protection: Protection,
}

/// What a protection pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProtectionTally {
    /// Distinct runs applied.
    pub runs: usize,
    /// Bytes made executable.
    pub executable: u64,
    /// Bytes left writable.
    pub writable: u64,
    /// Bytes that ended up both writable and executable.
    ///
    /// Not an error: a guest may ask for it, and a silent downgrade would fault somewhere
    /// unrelated. Counted because it is a hazard.
    pub writable_and_executable: u64,
}

/// Computes the protection every page should end up with.
///
/// Pure, so the sharing rule is testable with nothing mapped (D016). Runs come back in
/// ascending address order with adjacent equal ones merged, so a large image takes one call
/// per region rather than one per page.
pub fn page_protections(segments: &[PlacedSegment], page: u64) -> Vec<ProtectionRun> {
    if page == 0 {
        return Vec::new();
    }
    let mut pages: BTreeMap<u64, Protection> = BTreeMap::new();
    for seg in segments {
        let memsz = seg.memsz();
        if memsz == 0 {
            continue;
        }
        let first = seg.address / page;
        let last = seg.address.saturating_add(memsz - 1) / page;
        for index in first..=last {
            pages
                .entry(index)
                .and_modify(|p| *p = p.union(seg.protection()))
                .or_insert_with(|| seg.protection());
        }
    }

    let mut runs: Vec<ProtectionRun> = Vec::new();
    for (index, protection) in pages {
        let base = index.saturating_mul(page);
        match runs.last_mut() {
            Some(run)
                if run.protection == protection && run.base.saturating_add(run.len) == base =>
            {
                run.len = run.len.saturating_add(page);
            }
            _ => runs.push(ProtectionRun {
                base,
                len: page,
                protection,
            }),
        }
    }
    runs
}

/// Applies every segment's declared access to a placed, relocated image.
///
/// Call after relocation: earlier, text is read-only while relocations still write to it.
pub fn apply(image: &mut Image, page: u64) -> Result<ProtectionTally, LoadError> {
    let runs = page_protections(image.segments(), page);
    let mut tally = ProtectionTally::default();
    for run in runs {
        image
            .space_mut()
            .protect(run.base, run.len, run.protection)?;
        tally.runs += 1;
        if run.protection.execute {
            tally.executable = tally.executable.saturating_add(run.len);
        }
        if run.protection.write {
            tally.writable = tally.writable.saturating_add(run.len);
        }
        if run.protection.is_writable_and_executable() {
            tally.writable_and_executable = tally.writable_and_executable.saturating_add(run.len);
        }
    }
    Ok(tally)
}

#[cfg(test)]
mod tests {
    use super::{ProtectionRun, page_protections};
    use crate::PlacedSegment;
    use orbistoun_mem::Protection;

    /// ELF `p_flags`: execute is 1, write is 2, read is 4.
    const RX: u32 = 0x4 | 0x1;
    /// Read plus write.
    const RW: u32 = 0x4 | 0x2;
    /// Read only.
    const R: u32 = 0x4;

    fn seg(address: u64, memsz: u64, flags: u32) -> PlacedSegment {
        PlacedSegment {
            index: 0,
            address,
            copied: memsz,
            zeroed: 0,
            flags,
        }
    }

    /// A text segment ends read-execute and not writable.
    #[test]
    fn a_text_segment_becomes_read_execute_and_stops_being_writable() {
        let runs = page_protections(&[seg(0x1000, 0x2000, RX)], 0x1000);
        assert_eq!(
            runs,
            vec![ProtectionRun {
                base: 0x1000,
                len: 0x2000,
                protection: Protection::READ_EXECUTE,
            }]
        );
    }

    /// A page shared by two segments gets the union of their access.
    #[test]
    fn two_segments_sharing_a_page_get_the_union_not_the_last_one() {
        // Text ends partway through a page that data begins in.
        let runs = page_protections(&[seg(0x1000, 0x800, RX), seg(0x1800, 0x800, RW)], 0x1000);
        assert_eq!(runs.len(), 1, "one shared page");
        let p = runs[0].protection;
        assert!(p.read && p.write && p.execute, "the page must satisfy both");
    }

    /// Adjacent pages with equal protection merge into one run.
    #[test]
    fn adjacent_pages_with_equal_protection_merge_into_one_run() {
        let runs = page_protections(&[seg(0, 0x10_000, R)], 0x1000);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].len, 0x10_000);
    }

    /// A gap between segments breaks a run.
    #[test]
    fn a_gap_between_segments_breaks_the_run() {
        // Merging across a hole would re-protect pages no segment claims.
        let runs = page_protections(&[seg(0x1000, 0x1000, R), seg(0x9000, 0x1000, R)], 0x1000);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].base, 0x1000);
        assert_eq!(runs[1].base, 0x9000);
    }

    /// Runs are in ascending address order whatever the program header order.
    #[test]
    fn runs_come_back_in_ascending_address_order() {
        let runs = page_protections(&[seg(0x8000, 0x1000, R), seg(0x1000, 0x1000, RX)], 0x1000);
        assert!(runs.windows(2).all(|w| w[0].base < w[1].base));
    }

    /// A segment ending mid-page protects that whole page.
    #[test]
    fn a_segment_ending_mid_page_still_protects_that_whole_page() {
        // Otherwise the tail page keeps the read-write it was populated with.
        let runs = page_protections(&[seg(0x1000, 0x1001, RX)], 0x1000);
        assert_eq!(runs[0].len, 0x2000, "the partial second page counts");
    }

    /// A write-execute segment is honoured and counted, not downgraded.
    #[test]
    fn a_write_execute_segment_is_honoured_rather_than_downgraded() {
        let runs = page_protections(&[seg(0x1000, 0x1000, RW | 0x1)], 0x1000);
        assert!(runs[0].protection.is_writable_and_executable());
    }

    /// No segments yield no runs.
    #[test]
    fn an_empty_segment_list_yields_no_runs() {
        assert!(page_protections(&[], 0x1000).is_empty());
    }
}
