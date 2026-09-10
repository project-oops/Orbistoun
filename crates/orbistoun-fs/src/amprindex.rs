//! The index a dumped title ships so the asynchronous file path can be served.
//!
//! # What this is, and why reading it is ordinary
//!
//! PPSA03416's directory holds `ampr_emu.index` beside a replacement `fakelib/libSceAmpr.sprx`.
//! The replacement exports every `sceAmpr*` name the title imports and implements them **in
//! guest code** - which is why orbistoun never records a call to one, and why the command
//! buffer arrives already built. What that guest code then calls is the three `sceKernelApr*`
//! functions in `libkernel`, and this file is the table it expects them to be answered from
//! (D591, D592).
//!
//! It is **guest material at rest** - a file in a title directory, nothing executed, nothing
//! disassembled - which `docs/PROVENANCE.md` calls `static` evidence, the same category as a
//! module's own import table.
//!
//! # The format, checked rather than assumed
//!
//! ```text
//! +0x00  "AMPRIDX3"
//! +0x08  u32  version, 3
//! +0x0c  u32  entry stride, 24
//! +0x10  u64  entry count
//! +0x30  entries begin
//! ```
//!
//! An entry is `{u32 name_offset, u32 name_length, u64 size, u64 modified}`, and the names
//! begin where the entries end. Decoded against the directory five times before being written
//! down: `/app0/debug.log` at 0, `eboot.bin` at 27,744,015, the replacement library at 218,678,
//! `keystone` at 96, and `/app0/Media/globalgamemanagers` at 224,748 - every one matching what
//! is on disk.
//!
//! Four header words between `+0x18` and `+0x2c` are **not** understood and are not read. Saying
//! so is cheaper than inventing a meaning for them.

/// What the file begins with.
const MAGIC: &[u8] = b"AMPRIDX3";

/// The version this parser understands.
///
/// Refused rather than attempted for any other value: a format that changed under the same
/// magic would parse into plausible-looking rubbish, and a wrong size handed to a guest is
/// exactly the answer principle 3 exists to refuse.
const VERSION: u32 = 3;

/// Where the entries start.
const ENTRIES_AT: usize = 0x30;

/// How long one entry is, and the value the header must agree with.
const STRIDE: usize = 24;

/// One file the index describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its position in the table, which is the only identifier the format carries.
    pub id: u64,
    /// The guest path, as the index spells it.
    pub path: String,
    /// How many bytes the file holds.
    pub size: u64,
}

/// Reads an index out of bytes, or nothing if they are not one.
///
/// **Refuses rather than salvages.** A truncated or unknown-version index parses to `None`, so a
/// caller answers "no" instead of answering from half a table.
#[must_use]
pub fn parse(bytes: &[u8]) -> Option<Vec<Entry>> {
    if bytes.len() < ENTRIES_AT || !bytes.starts_with(MAGIC) {
        return None;
    }
    if word32(bytes, 8)? != VERSION || word32(bytes, 0x0c)? as usize != STRIDE {
        return None;
    }
    let count = usize::try_from(word64(bytes, 0x10)?).ok()?;
    let names_at = ENTRIES_AT.checked_add(count.checked_mul(STRIDE)?)?;
    if bytes.len() < names_at {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let at = ENTRIES_AT + index * STRIDE;
        let starts = names_at.checked_add(word32(bytes, at)? as usize)?;
        let name_len = word32(bytes, at + 4)? as usize;
        let end = starts.checked_add(name_len)?;
        if end > bytes.len() {
            return None;
        }
        out.push(Entry {
            id: index as u64,
            path: String::from_utf8_lossy(&bytes[starts..end]).into_owned(),
            size: word64(bytes, at + 8)?,
        });
    }
    Some(out)
}

/// A little-endian 32-bit word, or nothing if it is not there.
fn word32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// A little-endian 64-bit word, or nothing if it is not there.
fn word64(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    /// Builds an index in the format the module note documents.
    ///
    /// A synthetic index can only show the parser is self-consistent. What pins the format to
    /// reality is the five entries checked against a real title directory, recorded in the
    /// module note - a test cannot do that part and should not pretend to.
    fn an_index(paths: &[(&str, u64)]) -> Vec<u8> {
        let mut out = vec![0_u8; super::ENTRIES_AT];
        out[..8].copy_from_slice(super::MAGIC);
        out[8..12].copy_from_slice(&super::VERSION.to_le_bytes());
        out[0x0c..0x10].copy_from_slice(&(super::STRIDE as u32).to_le_bytes());
        out[0x10..0x18].copy_from_slice(&(paths.len() as u64).to_le_bytes());
        let mut names = Vec::new();
        for (path, size) in paths {
            out.extend_from_slice(&(names.len() as u32).to_le_bytes());
            out.extend_from_slice(&(path.len() as u32).to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&0_u64.to_le_bytes());
            names.extend_from_slice(path.as_bytes());
        }
        out.extend_from_slice(&names);
        out
    }

    /// Every entry comes back with its path, its size, and its position as an identifier.
    #[test]
    fn an_index_reads_back_what_it_holds() {
        let bytes = an_index(&[
            ("/app0/debug.log", 0),
            ("/app0/Media/globalgamemanagers", 224_748),
        ]);
        let entries = super::parse(&bytes).expect("a well-formed index parses");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "/app0/debug.log");
        assert_eq!(entries[0].size, 0);
        assert_eq!(entries[0].id, 0, "the position is the identifier");
        assert_eq!(entries[1].path, "/app0/Media/globalgamemanagers");
        assert_eq!(entries[1].size, 224_748);
        assert_eq!(entries[1].id, 1);
    }

    /// **Anything that is not this format is refused, not salvaged.**
    ///
    /// Watched failing: without the version check, an index claiming version 4 parses into
    /// entries whose sizes are whatever the new layout put where the old one kept a length -
    /// and a wrong size handed to a guest is the plausible answer principle 3 refuses.
    #[test]
    fn only_this_format_is_read() {
        let mut wrong_magic = an_index(&[("/a", 1)]);
        wrong_magic[0] = b'X';
        assert!(
            super::parse(&wrong_magic).is_none(),
            "magic was not checked"
        );

        let mut wrong_version = an_index(&[("/a", 1)]);
        wrong_version[8] = 4;
        assert!(
            super::parse(&wrong_version).is_none(),
            "a different version was parsed as this one"
        );

        let mut wrong_stride = an_index(&[("/a", 1)]);
        wrong_stride[0x0c] = 32;
        assert!(
            super::parse(&wrong_stride).is_none(),
            "a different entry size was parsed as this one"
        );

        assert!(super::parse(b"").is_none(), "an empty file parsed");
        assert!(super::parse(b"AMPRIDX3").is_none(), "a header alone parsed");
    }

    /// A truncated index answers nothing rather than the entries it managed to read.
    #[test]
    fn a_truncated_index_is_refused_whole() {
        let bytes = an_index(&[("/app0/one", 1), ("/app0/two", 2)]);
        for cut in [super::ENTRIES_AT + 1, bytes.len() - 1] {
            assert!(
                super::parse(&bytes[..cut]).is_none(),
                "a truncated index parsed at {cut}"
            );
        }
    }
}
