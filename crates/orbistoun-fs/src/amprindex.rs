//! The file index a title ships so the asynchronous file path can be served.
//!
//! Some title directories hold `ampr_emu.index` beside a replacement `libSceAmpr.sprx` that
//! implements every `sceAmpr*` function in guest code and calls the three `sceKernelApr*`
//! functions in `libkernel`; this file is the table those are answered from. It is guest
//! material at rest, `static` evidence in `docs/PROVENANCE.md` terms.
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
//! begin where the entries end. The four header words between `+0x18` and `+0x2c` are not
//! understood and not read.

/// What the file begins with.
const MAGIC: &[u8] = b"AMPRIDX3";

/// The version this parser understands.
///
/// Any other value is refused: a format changed under the same magic would parse into
/// plausible sizes that are wrong.
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
/// A truncated or unknown-version index parses to `None`, so a caller answers "no" instead
/// of answering from half a table.
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
    /// A synthetic index shows only that the parser is self-consistent.
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

    /// Anything that is not this format is refused, not salvaged.
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
