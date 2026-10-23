//! Searches and saves guest memory when a run ends (`ORBISTOUN_FIND`, `ORBISTOUN_SAVE`).
//!
//! What a title decrypts or builds for itself - an archive's table of contents after it is
//! mounted, a script program after it is loaded - exists only in its memory. A search for a name
//! says where it landed, and a save keeps the bytes for reading offline, so a run hands over what
//! no file on disk shows.

use std::fmt::Write as _;

/// The guest's part of the address space: its image from `0x4000_0000_0000` and the memory it
/// maps above, below where the host keeps its own image.
#[cfg(windows)]
const GUEST: (u64, u64) = (0x4000_0000_0000, 0x7f00_0000_0000);
/// How many places one search reports.
const MOST_HITS: usize = 64;
/// How much of a region is read at once.
const CHUNK: usize = 1 << 20;

/// Runs the searches and saves the run asked for, and logs what they found.
pub(crate) fn report() {
    if let Some(raw) = orbistoun_env::FIND.get() {
        // `<text>+<len>` also saves `len` bytes from each place the text is found.
        let wanted: Vec<(&str, Option<u64>)> = raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|item| match item.rsplit_once('+') {
                Some((text, len)) if number(len).is_some() => (text, number(len)),
                _ => (item, None),
            })
            .collect();
        if !wanted.is_empty() {
            let needles: Vec<&str> = wanted.iter().map(|w| w.0).collect();
            let found = find(&needles);
            for ((_, len), places) in wanted.iter().zip(&found) {
                if let Some(len) = len {
                    for &at in places {
                        save(&format!("{at:#x}+{len:#x}"));
                    }
                }
            }
        }
    }
    if let Some(raw) = orbistoun_env::SAVE.get() {
        save(&raw);
    }
}

/// Every place each of `needles` occurs in the guest's readable memory, logged and answered.
fn find(needles: &[&str]) -> Vec<Vec<u64>> {
    let regions = regions();
    let mut hits: Vec<Vec<u64>> = vec![Vec::new(); needles.len()];
    let mut buffer = vec![0u8; CHUNK];
    let longest = needles.iter().map(|n| n.len()).max().unwrap_or(0);
    for &(base, len) in &regions {
        let mut at = base;
        let end = base + len;
        while at < end {
            let take = usize::try_from((end - at).min(CHUNK as u64)).unwrap_or(CHUNK);
            if !copy_out(at, &mut buffer[..take]) {
                break;
            }
            for (index, needle) in needles.iter().enumerate() {
                if hits[index].len() >= MOST_HITS {
                    continue;
                }
                for offset in occurrences(&buffer[..take], needle.as_bytes()) {
                    hits[index].push(at + offset as u64);
                    if hits[index].len() >= MOST_HITS {
                        break;
                    }
                }
            }
            // Overlapped by the longest needle less one, so one split across chunks is found.
            let step = take.saturating_sub(longest.saturating_sub(1)).max(1);
            at += step as u64;
        }
    }
    let total: u64 = regions.iter().map(|r| r.1).sum();
    let mut said = format!(
        "searched {} readable region(s), {} MiB of guest memory:",
        regions.len(),
        total >> 20
    );
    for (needle, found) in needles.iter().zip(&hits) {
        let _ = write!(said, "\n  {needle:?}: {} place(s)", found.len());
        for &address in found {
            let _ = write!(said, "\n    {address:#x}  {}", context(address));
        }
    }
    tracing::info!("{said}");
    hits
}

/// Writes each `<addr>+<len>` to `mem-<addr>.bin` where the run keeps its frames.
fn save(raw: &str) {
    let dir = orbistoun_env::FRAME_DIR
        .get()
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from);
    for item in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let Some((at, len)) = located(item) else {
            tracing::warn!("{item:?} is not [*]<addr>[@<offset>]+<len> - not saved");
            continue;
        };
        let Ok(size) = usize::try_from(len) else {
            continue;
        };
        let mut bytes = vec![0u8; size];
        if !copy_out(at, &mut bytes) {
            tracing::warn!("{at:#x}+{len:#x} is not readable guest memory - not saved");
            continue;
        }
        // A small region is read in the log as well: a flag or a pointer needs no file.
        if bytes.len() <= 0x40 {
            let words: Vec<String> = bytes
                .chunks(8)
                .map(|w| {
                    w.iter().rev().fold(String::new(), |mut hex, b| {
                        let _ = write!(hex, "{b:02x}");
                        hex
                    })
                })
                .collect();
            tracing::info!("{item} = {at:#x}: {}", words.join(" "));
        }
        let path = dir.join(format!("mem-{at:x}.bin"));
        match std::fs::write(&path, &bytes) {
            Ok(()) => tracing::info!("saved {at:#x}+{len:#x} to {}", path.display()),
            Err(e) => tracing::warn!("{at:#x}+{len:#x} could not be saved: {e}"),
        }
    }
}

/// Where `[*]<addr>[@<offset>]+<len>` points and how long it is: `*` reads the eight-byte pointer
/// stored at `addr` (an object the guest allocates lands somewhere new each run; the global that
/// holds it does not), then `@` adds the offset. `None` when malformed or the pointer unreadable.
fn located(item: &str) -> Option<(u64, u64)> {
    let (place, len) = item.rsplit_once('+')?;
    let len = number(len)?;
    let (place, offset) = match place.split_once('@') {
        Some((place, offset)) => (place, number(offset)?),
        None => (place, 0),
    };
    let base = match place.trim().strip_prefix('*') {
        Some(pointer) => {
            let mut word = [0u8; 8];
            if !copy_out(number(pointer)?, &mut word) {
                return None;
            }
            u64::from_le_bytes(word)
        }
        None => number(place)?,
    };
    Some((base.checked_add(offset)?, len))
}

/// The offsets where `needle` occurs in `haystack`.
fn occurrences(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    let Some((&first, rest)) = needle.split_first() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(position) = haystack[from..].iter().position(|&b| b == first) {
        let at = from + position;
        if haystack[at + 1..].starts_with(rest) {
            found.push(at);
        }
        from = at + 1;
    }
    found
}

/// The bytes around `address`, printable ones as themselves and the rest as dots.
fn context(address: u64) -> String {
    let start = address.saturating_sub(16);
    let mut bytes = [0u8; 64];
    if !copy_out(start, &mut bytes) {
        return String::new();
    }
    bytes
        .iter()
        .map(|&b| {
            if (0x20..0x7f).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect()
}

/// A number, decimal or hexadecimal.
fn number(raw: &str) -> Option<u64> {
    let text = raw.trim();
    match text.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// Copies guest memory at `at` into `out`; `false`, copying nothing, when the host does not have
/// all of it committed and readable.
fn copy_out(at: u64, out: &mut [u8]) -> bool {
    if !readable(at, out.len() as u64) {
        return false;
    }
    let Ok(address) = usize::try_from(at) else {
        return false;
    };
    // SAFETY: `readable` just found every page of the range committed and readable in this
    // process; the guest may still write it, which a diagnostic copy tolerates.
    unsafe {
        std::ptr::copy_nonoverlapping(
            std::ptr::with_exposed_provenance::<u8>(address),
            out.as_mut_ptr(),
            out.len(),
        );
    }
    true
}

/// The committed, readable regions of the guest's part of the address space.
#[cfg(windows)]
fn regions() -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut at = GUEST.0;
    while at < GUEST.1 {
        let Some((base, len, ok)) = query(at) else {
            break;
        };
        if ok {
            out.push((
                base.max(GUEST.0),
                (base + len).min(GUEST.1) - base.max(GUEST.0),
            ));
        }
        at = base + len.max(0x1000);
    }
    out
}

#[cfg(not(windows))]
fn regions() -> Vec<(u64, u64)> {
    Vec::new()
}

/// Whether every page of `[at, at + len)` is committed and readable.
#[cfg(windows)]
fn readable(at: u64, len: u64) -> bool {
    let end = at.saturating_add(len);
    let mut probe = at;
    while probe < end {
        match query(probe) {
            Some((base, size, true)) => probe = base + size,
            _ => return false,
        }
    }
    true
}

#[cfg(not(windows))]
fn readable(_at: u64, _len: u64) -> bool {
    false
}

/// The region holding `at`: its base, its length, and whether it is committed and readable.
#[cfg(windows)]
fn query(at: u64) -> Option<(u64, u64, bool)> {
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
        PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY,
        VirtualQuery,
    };
    // SAFETY: an all-zero `MEMORY_BASIC_INFORMATION` is a valid value; the query overwrites it.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
    // SAFETY: querying any address is safe; the structure is the size passed and outlives the
    // call.
    let written = unsafe {
        VirtualQuery(
            std::ptr::with_exposed_provenance::<core::ffi::c_void>(usize::try_from(at).ok()?),
            &raw mut info,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if written == 0 {
        return None;
    }
    let base = info.BaseAddress as u64;
    let len = info.RegionSize as u64;
    let readable = PAGE_READONLY
        | PAGE_READWRITE
        | PAGE_WRITECOPY
        | PAGE_EXECUTE_READ
        | PAGE_EXECUTE_READWRITE
        | PAGE_EXECUTE_WRITECOPY;
    let ok =
        info.State == MEM_COMMIT && info.Protect & readable != 0 && info.Protect & PAGE_GUARD == 0;
    Some((base, len, ok))
}

#[cfg(test)]
mod tests {
    /// Every occurrence is found, overlapping ones too, and an empty needle finds nothing.
    #[test]
    fn every_occurrence_is_found() {
        assert_eq!(super::occurrences(b"aabaab", b"ab"), vec![1, 4]);
        assert_eq!(super::occurrences(b"aaaa", b"aa"), vec![0, 1, 2]);
        assert!(super::occurrences(b"abc", b"").is_empty());
    }
}
