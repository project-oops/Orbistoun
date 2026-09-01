//! Section headers, and the globals a runtime fills in.
//!
//! # Why this exists when nothing else here needs sections
//!
//! Loading a program needs **program** headers; sections are a link-time view and a loader
//! is entitled to ignore them. This project ignored them for a year.
//!
//! One question needs them. The open-toolchain payloads resolve most of their C library at
//! startup and store the answers in named globals in `.bss` - `vsnprintf`, `snprintf`,
//! `strerror`, forty-odd of them - and a run entered past that startup code finds them all
//! null. Those names are in `.symtab`, which is a section, and which no program header
//! points at (D376).
//!
//! So this reads exactly enough to answer *which named globals does this program have, and
//! where*. It is not a general section parser and does not want to be.
//!
//! # What it deliberately does not do
//!
//! Nothing here decides to *write* anything. It answers a list; the loader decides what to do
//! with it, and does so only in a mode that already declares itself not an ordinary run.

use zerocopy::{FromBytes, Immutable, KnownLayout, little_endian};

use crate::{Container, ElfError};

/// One section header, as ELF64 lays it out.
#[derive(Debug, Clone, Copy, FromBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct Elf64SectionHeader {
    /// Offset of this section's name in the section-name string table.
    pub name: little_endian::U32,
    /// What kind of section this is.
    pub sh_type: little_endian::U32,
    /// Flags.
    pub flags: little_endian::U64,
    /// Address this section has once loaded, or zero.
    pub addr: little_endian::U64,
    /// File offset of its contents.
    pub offset: little_endian::U64,
    /// Size in bytes.
    pub size: little_endian::U64,
    /// A section this one refers to - for a symbol table, its string table.
    pub link: little_endian::U32,
    /// Extra information, per type.
    pub info: little_endian::U32,
    /// Address alignment.
    pub addralign: little_endian::U64,
    /// Size of one entry, for sections that hold a table.
    pub entsize: little_endian::U64,
}

/// A full symbol table, which is a section rather than a segment.
pub const SHT_SYMTAB: u32 = 2;

/// A section that occupies no file space - `.bss`.
pub const SHT_NOBITS: u32 = 8;

/// A section that is writable once loaded.
pub const SHF_WRITE: u64 = 0x1;

/// One symbol, as ELF64 lays it out.
#[derive(Debug, Clone, Copy, FromBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct Elf64Symbol {
    /// Offset of the name in the linked string table.
    pub name: little_endian::U32,
    /// Binding and type, packed.
    pub info: u8,
    /// Visibility.
    pub other: u8,
    /// Section this symbol belongs to.
    pub shndx: little_endian::U16,
    /// Its address.
    pub value: little_endian::U64,
    /// Its size in bytes.
    pub size: little_endian::U64,
}

/// A named global a runtime would have filled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedGlobal {
    /// What the program calls it.
    pub name: String,
    /// Where it lives, as the image was linked.
    pub address: u64,
    /// How many bytes it occupies.
    pub size: u64,
}

impl Container<'_> {
    /// The section headers, or an empty list when the file has none.
    ///
    /// A stripped image legitimately has none, and that is not an error: it is a program
    /// this cannot answer the question about.
    ///
    /// # Errors
    ///
    /// When the table runs past the end of the file.
    pub fn section_headers(&self, whole: &[u8]) -> Result<Vec<Elf64SectionHeader>, ElfError> {
        let inner = self.inner_bytes(whole);
        let offset = self.header().shoff.get();
        let count = self.header().shnum.get() as usize;
        let size = self.header().shentsize.get() as usize;
        if offset == 0 || count == 0 || size < size_of::<Elf64SectionHeader>() {
            return Ok(Vec::new());
        }
        let Ok(start) = usize::try_from(offset) else {
            return Ok(Vec::new());
        };

        let mut out = Vec::with_capacity(count);
        for index in 0..count {
            let at = start + index * size;
            let end = at + size_of::<Elf64SectionHeader>();
            if end > inner.len() {
                return Err(ElfError::Truncated {
                    offset: at,
                    need: size_of::<Elf64SectionHeader>(),
                    have: inner.len().saturating_sub(at),
                });
            }
            let (header, _) = Elf64SectionHeader::read_from_prefix(&inner[at..])
                .map_err(|_| ElfError::UnsupportedFormat)?;
            out.push(header);
        }
        Ok(out)
    }

    /// Named globals this program keeps in writable, zero-filled storage.
    ///
    /// **`.bss` objects with names**, which is precisely the set a startup routine fills in.
    /// A program with no symbol table answers an empty list, which is a true answer about a
    /// stripped image rather than a failure.
    ///
    /// Duplicate names are kept, because they are real: `klogsrv` has four separate slots
    /// called `strcpy`, and filling one and not the others would leave three nulls behind.
    ///
    /// # Errors
    ///
    /// When a section table or a symbol table runs past the end of the file.
    pub fn named_globals(&self, whole: &[u8]) -> Result<Vec<NamedGlobal>, ElfError> {
        let inner = self.inner_bytes(whole);
        let sections = self.section_headers(whole)?;

        // Which sections are writable and occupy no file space - `.bss` and anything shaped
        // like it. A runtime fills these; a `.data` global already has its value.
        let zero_filled: Vec<usize> = sections
            .iter()
            .enumerate()
            .filter(|(_, s)| s.sh_type.get() == SHT_NOBITS && s.flags.get() & SHF_WRITE != 0)
            .map(|(index, _)| index)
            .collect();
        if zero_filled.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for table in sections.iter().filter(|s| s.sh_type.get() == SHT_SYMTAB) {
            let entry = table.entsize.get() as usize;
            if entry < size_of::<Elf64Symbol>() {
                continue;
            }
            let Some(strings) = sections.get(table.link.get() as usize) else {
                continue;
            };
            let Ok(strings_at) = usize::try_from(strings.offset.get()) else {
                continue;
            };
            let strings_end = strings_at.saturating_add(strings.size.get() as usize);
            if strings_end > inner.len() {
                continue;
            }
            let strings = &inner[strings_at..strings_end];

            let Ok(table_at) = usize::try_from(table.offset.get()) else {
                continue;
            };
            let count = (table.size.get() as usize) / entry;
            for index in 0..count {
                let at = table_at + index * entry;
                if at + size_of::<Elf64Symbol>() > inner.len() {
                    break;
                }
                let Ok((symbol, _)) = Elf64Symbol::read_from_prefix(&inner[at..]) else {
                    break;
                };
                // Low four bits of `info` are the type; one is an object.
                if symbol.info & 0xF != 1 {
                    continue;
                }
                if !zero_filled.contains(&(symbol.shndx.get() as usize)) {
                    continue;
                }
                let Some(name) = read_name(strings, symbol.name.get() as usize) else {
                    continue;
                };
                if name.is_empty() {
                    continue;
                }
                out.push(NamedGlobal {
                    name,
                    address: symbol.value.get(),
                    size: symbol.size.get(),
                });
            }
        }
        Ok(out)
    }

    /// The bytes of the ELF itself, past any wrapper.
    fn inner_bytes<'b>(&self, whole: &'b [u8]) -> &'b [u8] {
        self.wrapper()
            .map_or(whole, |w| &whole[w.elf_offset().min(whole.len())..])
    }
}

/// A NUL-terminated name out of a string table.
fn read_name(strings: &[u8], at: usize) -> Option<String> {
    let rest = strings.get(at..)?;
    let end = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
    String::from_utf8(rest[..end].to_vec()).ok()
}

#[cfg(test)]
mod tests {
    /// A file with no section table answers an empty list rather than failing.
    ///
    /// A stripped image is a real thing to be handed, and "this program has no named
    /// globals" is a true answer about one.
    #[test]
    fn a_file_without_sections_answers_nothing_rather_than_failing() {
        let mut bytes = vec![0_u8; 64];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        let container = crate::Container::parse(&bytes).expect("a bare header");
        assert!(
            container
                .section_headers(&bytes)
                .expect("no table")
                .is_empty()
        );
        assert!(
            container
                .named_globals(&bytes)
                .expect("no symbols")
                .is_empty()
        );
    }
}
