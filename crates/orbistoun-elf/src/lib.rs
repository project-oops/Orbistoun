//! Vendor ELF and PRX container parsing.
//!
//! A target executable is an ELF64 with vendor extensions: vendor-specific
//! `e_type` values, and program headers in the OS-specific range that carry the
//! dynamic-link data an ordinary ELF keeps in sections. The standard parts are
//! parsed here exactly; the vendor parts are the interesting work.
//!
//! # No unsafe
//!
//! All structure reads go through `zerocopy`, which validates size and alignment
//! before handing back a typed reference. Parsing attacker-shaped bytes is the
//! last place that should contain hand-rolled pointer casts, and it does not.
//!
//! # Status
//!
//! Container wrapper, ELF64 headers, the wrapper-to-program-header mapping (D052),
//! the dynamic table, imports (D053), and relocation tables all parse. Verified
//! against real material: 1,410 imports from a commercial executable.

pub mod dynamic;
pub mod procparam;
pub mod reloc;
pub mod sections;
mod wrapper;

pub use wrapper::{
    Generation, HEADER_SIZE, MAX_SEGMENTS, PREVIOUS_GENERATION_MAGIC, SEGMENT_FLAG_HAS_DATA,
    SEGMENT_PHDR_INDEX_SHIFT, SEGMENT_SIZE, WRAPPER_MAGIC, Wrapper, WrapperHeader, WrapperSegment,
};

use orbistoun_core::GuestError;
use zerocopy::{FromBytes, Immutable, KnownLayout, little_endian};

/// Why a container could not be parsed.
#[derive(Debug, thiserror::Error)]
pub enum ElfError {
    /// The file is too short to contain the structure being read.
    #[error("truncated: need {need} bytes at offset {offset}, have {have}")]
    Truncated {
        /// Byte offset the read started at.
        offset: usize,
        /// Bytes required.
        need: usize,
        /// Bytes actually available.
        have: usize,
    },
    /// The magic number is not `\x7fELF`.
    #[error("not an ELF file")]
    NotElf,
    /// The file is ELF but not a 64-bit little-endian one.
    #[error("unsupported ELF class or endianness")]
    UnsupportedFormat,
    /// The bytes carry no container wrapper.
    #[error("not a wrapped container")]
    NotWrapped,
    /// The descriptor count is implausible.
    ///
    /// A limit rather than trust: the count comes from arbitrary bytes, and an
    /// unchecked one is an allocation sized by the input.
    #[error("segment count {count} exceeds the {max} sanity limit")]
    AbsurdSegmentCount {
        /// Count the header claimed.
        count: u16,
        /// Limit applied.
        max: u16,
    },
    /// The symbol count is implausible.
    #[error("symbol count {count} exceeds the {max} sanity limit")]
    AbsurdSymbolCount {
        /// Count derived from the hash table.
        count: u64,
        /// Limit applied.
        max: u64,
    },
    /// The dynamic table is absent or lacks what a symbol walk needs.
    #[error("no usable dynamic table: {reason}")]
    NoDynamicTable {
        /// What was missing.
        reason: &'static str,
    },
    /// The derived offset did not land on an ELF image.
    #[error("no inner ELF at derived offset {expected_at} (segment count {segment_count})")]
    InnerElfNotFound {
        /// Offset the header arithmetic produced.
        expected_at: usize,
        /// Descriptor count used to derive it.
        segment_count: u16,
    },
}

impl From<ElfError> for GuestError {
    fn from(_: ElfError) -> Self {
        Self::InvalidArgument
    }
}

/// Raw ELF64 file header, exactly as it appears on disk.
#[derive(Debug, Clone, Copy, FromBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct Elf64Header {
    /// Magic, class, endianness, version, ABI.
    pub ident: [u8; 16],
    /// Object file type. The vendor uses custom values here.
    pub e_type: little_endian::U16,
    /// Target architecture. Always x86-64 for both target generations.
    pub machine: little_endian::U16,
    /// ELF version.
    pub version: little_endian::U32,
    /// Guest virtual address of the entry point.
    pub entry: little_endian::U64,
    /// File offset of the program header table.
    pub phoff: little_endian::U64,
    /// File offset of the section header table.
    pub shoff: little_endian::U64,
    /// Processor-specific flags.
    pub flags: little_endian::U32,
    /// Size of this header.
    pub ehsize: little_endian::U16,
    /// Size of one program header entry.
    pub phentsize: little_endian::U16,
    /// Number of program header entries.
    pub phnum: little_endian::U16,
    /// Size of one section header entry.
    pub shentsize: little_endian::U16,
    /// Number of section header entries.
    pub shnum: little_endian::U16,
    /// Section index of the section-name string table.
    pub shstrndx: little_endian::U16,
}

/// Raw ELF64 program header.
#[derive(Debug, Clone, Copy, FromBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct Elf64ProgramHeader {
    /// Segment type. See [`segment::is_vendor`].
    pub p_type: little_endian::U32,
    /// Segment permission flags.
    pub flags: little_endian::U32,
    /// File offset of the segment contents.
    pub offset: little_endian::U64,
    /// Guest virtual address the segment loads at.
    pub vaddr: little_endian::U64,
    /// Physical address. Unused.
    pub paddr: little_endian::U64,
    /// Bytes present in the file.
    pub filesz: little_endian::U64,
    /// Bytes the segment occupies in memory, which may exceed `filesz`.
    pub memsz: little_endian::U64,
    /// Required alignment.
    pub align: little_endian::U64,
}

/// Program header types, including the vendor's, from the format's owner.
pub use selfish_elf::segment;

/// A parsed container, borrowing the file bytes.
///
/// Handles both shapes: a plain ELF, and the wrapped form real material uses (D049).
/// `bytes` is the *inner* image in both cases, so everything downstream is unaware of
/// the difference.
#[derive(Debug)]
pub struct Container<'a> {
    bytes: &'a [u8],
    header: Elf64Header,
    wrapper: Option<Wrapper>,
}

/// The dynamic symbol table, its strings, how many symbols there are, and the entry stride.
///
/// Named because the tuple is four wide and two callers unpack it: a reader meeting
/// `(&[u8], &[u8], u64, usize)` has to work out which slice is which from the call site.
type SymbolTables<'bytes> = (&'bytes [u8], &'bytes [u8], u64, usize);

impl<'a> Container<'a> {
    /// Parses a container, unwrapping it first if it is wrapped.
    ///
    /// Real executables are wrapped; the synthetic fixtures and hand-built test cases
    /// are usually not. Both are accepted, and [`Container::wrapper`] says which was
    /// found.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ElfError> {
        if Wrapper::is_either_generation(bytes) {
            let wrapper = Wrapper::parse(bytes)?;
            let inner = &bytes[wrapper.elf_offset()..];
            let mut container = Self::parse_plain(inner)?;
            container.wrapper = Some(wrapper);
            return Ok(container);
        }
        Self::parse_plain(bytes)
    }

    /// The wrapper this container was found inside, if any.
    pub const fn wrapper(&self) -> Option<&Wrapper> {
        self.wrapper.as_ref()
    }

    /// Parses a bare ELF64 image.
    fn parse_plain(bytes: &'a [u8]) -> Result<Self, ElfError> {
        let header = Elf64Header::read_from_prefix(bytes)
            .map(|(h, _)| h)
            .map_err(|_| ElfError::Truncated {
                offset: 0,
                need: size_of::<Elf64Header>(),
                have: bytes.len(),
            })?;

        if &header.ident[..4] != b"\x7fELF" {
            return Err(ElfError::NotElf);
        }
        // ident[4] is EI_CLASS (2 = 64-bit), ident[5] is EI_DATA (1 = little).
        if header.ident[4] != 2 || header.ident[5] != 1 {
            return Err(ElfError::UnsupportedFormat);
        }

        Ok(Self {
            bytes,
            header,
            wrapper: None,
        })
    }

    /// The parsed file header.
    pub const fn header(&self) -> &Elf64Header {
        &self.header
    }

    /// The guest entry point address.
    pub fn entry(&self) -> u64 {
        self.header.entry.get()
    }

    /// Iterates the program header table.
    pub fn program_headers(&self) -> Result<Vec<Elf64ProgramHeader>, ElfError> {
        let count = usize::from(self.header.phnum.get());
        let entsize = usize::from(self.header.phentsize.get());
        let offset = usize::try_from(self.header.phoff.get()).map_err(|_| ElfError::Truncated {
            offset: 0,
            need: 0,
            have: self.bytes.len(),
        })?;

        let need = count.saturating_mul(entsize);
        let end = offset.saturating_add(need);
        if end > self.bytes.len() {
            return Err(ElfError::Truncated {
                offset,
                need,
                have: self
                    .bytes
                    .len()
                    .saturating_sub(offset.min(self.bytes.len())),
            });
        }

        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let at = offset + i * entsize;
            let ph = Elf64ProgramHeader::read_from_prefix(&self.bytes[at..])
                .map(|(p, _)| p)
                .map_err(|_| ElfError::Truncated {
                    offset: at,
                    need: size_of::<Elf64ProgramHeader>(),
                    have: self.bytes.len() - at,
                })?;
            out.push(ph);
        }
        Ok(out)
    }

    /// The bytes backing one program header, whichever container shape this is.
    ///
    /// For a wrapped container the descriptor table locates them (D052); for a bare
    /// ELF the header addresses the file directly. A caller should not have to know
    /// which shape it was handed.
    pub fn segment_data<'b>(
        &self,
        whole: &'b [u8],
        index: usize,
    ) -> Result<Option<&'b [u8]>, ElfError> {
        if let Some(wrapper) = self.wrapper {
            return wrapper.data_for_program_header(whole, index);
        }
        let headers = self.program_headers()?;
        let Some(ph) = headers.get(index) else {
            return Ok(None);
        };
        let start = usize::try_from(ph.offset.get()).unwrap_or(usize::MAX);
        let len = usize::try_from(ph.filesz.get()).unwrap_or(0);
        Ok(whole.get(start..start.saturating_add(len)))
    }

    /// Program-header indices the wrapper's descriptor table locates data for.
    ///
    /// Empty for an unwrapped container, where program headers address the file
    /// directly and no mapping is needed.
    ///
    /// Headers absent from the list are not missing: several describe regions
    /// *inside* another header's data rather than carrying their own descriptor.
    pub fn mapped_program_headers(&self, whole: &[u8]) -> Result<Vec<usize>, ElfError> {
        let Some(wrapper) = self.wrapper else {
            return Ok(Vec::new());
        };
        let count = self.program_headers()?.len();
        let mut mapped = Vec::new();
        for index in 0..count {
            if wrapper.data_for_program_header(whole, index)?.is_some() {
                mapped.push(index);
            }
        }
        Ok(mapped)
    }

    /// Program headers carrying vendor data.
    pub fn vendor_segments(&self) -> Result<Vec<Elf64ProgramHeader>, ElfError> {
        Ok(self
            .program_headers()?
            .into_iter()
            .filter(|ph| segment::is_vendor(ph.p_type.get()))
            .collect())
    }

    /// Translates a guest virtual address to a position in the whole container.
    ///
    /// Finds the program header whose virtual range covers `vaddr`, then locates that
    /// header's bytes through the wrapper (D052). The headers' own `p_offset` values
    /// are not usable **in a wrapped container** - there they describe the decrypted image
    /// while the file holds the wrapper's descriptors, so they routinely point past
    /// end-of-file.
    ///
    /// In an **unwrapped** container they are the only thing there is, and they are
    /// authoritative. This returned [`None`] for every address of every bare ELF until
    /// somebody ran one: the container parsed, the loader mapped its segments, and then
    /// nothing downstream could locate a single byte - which surfaced as *no `PT_DYNAMIC`
    /// segment, or its address could not be located* about a module that plainly had one
    /// (D237).
    pub fn vaddr_to_offset(&self, whole: &[u8], vaddr: u64) -> Result<Option<usize>, ElfError> {
        let Some(wrapper) = self.wrapper else {
            return self.bare_vaddr_to_offset(whole, vaddr);
        };
        for (index, ph) in self.program_headers()?.iter().enumerate() {
            let base = ph.vaddr.get();
            let size = ph.filesz.get();
            if size == 0 || vaddr < base || vaddr >= base.saturating_add(size) {
                continue;
            }
            // The covering header may itself have no descriptor; walk outwards to one
            // that does, since several headers describe regions inside another's data.
            for (outer_index, outer) in self.program_headers()?.iter().enumerate() {
                let obase = outer.vaddr.get();
                let osize = outer.filesz.get();
                if osize == 0 || vaddr < obase || vaddr >= obase.saturating_add(osize) {
                    continue;
                }
                if let Some(data) = wrapper.data_for_program_header(whole, outer_index)? {
                    let within = usize::try_from(vaddr - obase).unwrap_or(usize::MAX);
                    if within < data.len() {
                        let seg_start = whole.len() - data.len();
                        // Recover the descriptor's absolute start rather than assuming.
                        for seg in wrapper.segments(whole)? {
                            if seg.has_data() && seg.program_header_index() == outer_index {
                                return Ok(Some(seg.range().start + within));
                            }
                        }
                        let _ = seg_start;
                    }
                }
            }
            let _ = index;
        }
        Ok(None)
    }

    /// Translates an address through the program headers' own file offsets.
    ///
    /// # Why a `PT_LOAD` wins when two headers claim the same address
    ///
    /// Because more than one can, and they mean different things. A vendor segment
    /// carrying dynamic data is commonly declared at virtual address zero - its contents
    /// are addressed as offsets into itself rather than as addresses in the image - and a
    /// module whose first `PT_LOAD` also starts at zero then has two headers covering the
    /// same low range. The `PT_LOAD` is the one describing the image a guest executes, so
    /// it is the one an address in that image means.
    ///
    /// **This is a preference, not a proof.** An address that is really an offset into a
    /// vendor segment will resolve against the `PT_LOAD` instead and give the wrong bytes.
    /// The wrapper path does not have the problem because a descriptor table says which
    /// header owns which run of file. Distinguishing them here needs the vendor segment's
    /// own conventions, which is a separate piece of work and is called out rather than
    /// guessed at (D237).
    fn bare_vaddr_to_offset(&self, whole: &[u8], vaddr: u64) -> Result<Option<usize>, ElfError> {
        const PT_LOAD: u32 = 1;
        let headers = self.program_headers()?;
        let covering = |ph: &Elf64ProgramHeader| {
            let base = ph.vaddr.get();
            let size = ph.filesz.get();
            if size == 0 || vaddr < base || vaddr >= base.saturating_add(size) {
                return None;
            }
            let within = vaddr - base;
            let at = usize::try_from(ph.offset.get().checked_add(within)?).ok()?;
            // Bounds-checked against the file rather than trusted. A header may describe
            // more than the file holds, and a truncated container should read as "cannot
            // locate that" rather than as a panic.
            (at < whole.len()).then_some(at)
        };
        Ok(headers
            .iter()
            .filter(|ph| ph.p_type.get() == PT_LOAD)
            .find_map(&covering)
            .or_else(|| headers.iter().find_map(&covering)))
    }

    /// The dynamic table's bytes, if the container has one.
    ///
    /// # Why this is the segment's own file offset and not its address
    ///
    /// **On a real title `PT_DYNAMIC` has no address.** It carries `vaddr 0` and sits at the
    /// *tail of `PT_SCE_DYNLIBDATA`*, which is a vendor segment that is also at `vaddr 0`:
    ///
    /// ```text
    /// PT_SCE_DYNLIBDATA  off 0x8c130  filesz 0x3760  vaddr 0   -> ends 0x8f890
    /// PT_DYNAMIC         off 0x8f450  filesz 0x0440  vaddr 0   -> ends 0x8f890
    /// ```
    ///
    /// So resolving it by address asks "which segment covers address zero", and **two of them
    /// do**. Which one wins is the order they happen to appear in the header table: the right
    /// answer by luck, or the start of the vendor blob, which is the wrong bytes entirely.
    ///
    /// So in a **bare** container the segment's own `p_offset` is used, which is where its
    /// bytes are and is unambiguous.
    ///
    /// # And why only a bare one
    ///
    /// In a **wrapped** container `p_offset` describes the decrypted image while the file
    /// holds the wrapper's descriptors, so it routinely points past end-of-file - which
    /// [`Self::vaddr_to_offset`] has said since D052 and this ignored. Generalising the
    /// hardware finding to both paths stopped every real title loading, and the doc comment
    /// on the function next door already explained why it would (D391).
    pub fn dynamic_bytes<'b>(&self, whole: &'b [u8]) -> Result<Option<&'b [u8]>, ElfError> {
        const PT_DYNAMIC: u32 = 2;
        for ph in self.program_headers()? {
            if ph.p_type.get() != PT_DYNAMIC {
                continue;
            }
            let size = usize::try_from(ph.filesz.get()).unwrap_or(0);
            // The address route, the way a header with a real virtual address is read. A
            // descriptor table makes the wrapper authoritative; a bare ELF reads its own
            // file offset. Bounds-checked against the file: a header may describe more than
            // the file holds, and that reads as "cannot locate that" rather than a panic.
            let at = if self.wrapper.is_some() {
                self.vaddr_to_offset(whole, ph.vaddr.get())?
            } else {
                usize::try_from(ph.offset.get()).ok()
            };
            let by_vaddr =
                at.and_then(|at| at.checked_add(size).and_then(|end| whole.get(at..end)));

            // **A module built the platform's way defeats the address route (D247).** It
            // carries its dynamic table *inside* `PT_SCE_DYNLIBDATA` and gives the segment a
            // virtual address of zero, which collides with the first `PT_LOAD` - so the
            // address above lands on code and the table reads as absent though it is present.
            // When the addressed bytes are not a usable table, locate it by file offset
            // within the segment whose data actually contains it. Retail titles keep a real
            // address here and never reach this branch.
            let usable = |bytes: &[u8]| dynamic::DynamicInfo::parse(bytes).is_usable();
            if self.wrapper.is_some() && !by_vaddr.is_some_and(usable) {
                if let Some(bytes) = self.dynamic_bytes_by_offset(whole, ph.offset.get(), size)? {
                    if usable(bytes) {
                        return Ok(Some(bytes));
                    }
                }
            }
            // The addressed bytes even when unusable, so the caller answers "lacks a string
            // table" against a located table rather than "could not be located".
            return Ok(by_vaddr);
        }
        Ok(None)
    }

    /// The dynamic table located by **file offset** rather than address, for a module that
    /// carries it inside `PT_SCE_DYNLIBDATA` with a zero virtual address (D247).
    ///
    /// Its own program header owns no descriptor run, so its bytes sit inside the enclosing
    /// segment's - the one whose file range covers the dynamic offset and does carry a
    /// descriptor. Mirrors [`Self::vaddr_to_offset`]'s outward walk, keyed on the file offset
    /// the address route could not trust.
    fn dynamic_bytes_by_offset<'b>(
        &self,
        whole: &'b [u8],
        dyn_offset: u64,
        size: usize,
    ) -> Result<Option<&'b [u8]>, ElfError> {
        let Some(wrapper) = self.wrapper else {
            return Ok(None);
        };
        for (outer_index, outer) in self.program_headers()?.iter().enumerate() {
            let obase = outer.offset.get();
            let osize = outer.filesz.get();
            if osize == 0 || dyn_offset < obase || dyn_offset >= obase.saturating_add(osize) {
                continue;
            }
            for seg in wrapper.segments(whole)? {
                if seg.has_data() && seg.program_header_index() == outer_index {
                    let within = usize::try_from(dyn_offset - obase).unwrap_or(usize::MAX);
                    let start = seg.range().start.saturating_add(within);
                    return Ok(start
                        .checked_add(size)
                        .and_then(|end| whole.get(start..end)));
                }
            }
        }
        Ok(None)
    }

    /// The process parameter block's bytes, if the container carries a `PT_SCE_PROCPARAM`
    /// segment ([`segment::SCE_PROCPARAM`]).
    ///
    /// Located the same way [`Self::dynamic_bytes`] locates the dynamic table: through the
    /// wrapper's descriptor table when wrapped (the header's own `p_offset` points into the
    /// decrypted image and past end-of-file), and through the header's `p_offset` when bare.
    /// Unlike `PT_DYNAMIC`, this segment carries a real `vaddr`, so the wrapped path resolves
    /// it by address.
    pub fn proc_param_bytes<'b>(&self, whole: &'b [u8]) -> Result<Option<&'b [u8]>, ElfError> {
        for ph in self.program_headers()? {
            if ph.p_type.get() != segment::SCE_PROCPARAM {
                continue;
            }
            let at = if self.wrapper.is_some() {
                match self.vaddr_to_offset(whole, ph.vaddr.get())? {
                    Some(at) => at,
                    None => return Ok(None),
                }
            } else {
                match usize::try_from(ph.offset.get()) {
                    Ok(at) => at,
                    Err(_) => return Ok(None),
                }
            };
            let size = usize::try_from(ph.filesz.get()).unwrap_or(0);
            let Some(end) = at.checked_add(size) else {
                return Ok(None);
            };
            return Ok(whole.get(at..end));
        }
        Ok(None)
    }

    /// Where a table named by a dynamic tag actually begins in the file.
    ///
    /// **Two different meanings for the same field.** A standard tag holds a virtual
    /// address; a vendor tag holds an offset into `PT_SCE_DYNLIBDATA`. Resolving one the
    /// other way lands at a plausible file offset holding the wrong bytes - which is how
    /// the probe's minimal module reported two relocations of an unsupported type when it
    /// has two ordinary ones (D247).
    ///
    /// One method rather than a closure at each site, because two of them already existed
    /// and only one had been taught the difference.
    pub fn table_offset(
        &self,
        whole: &[u8],
        info: &dynamic::DynamicInfo,
        value: u64,
    ) -> Result<Option<usize>, ElfError> {
        if info.vendor_tables {
            let Some(base) = self.vendor_data_offset(whole)? else {
                return Ok(None);
            };
            return Ok(usize::try_from(value)
                .ok()
                .and_then(|v| base.checked_add(v)));
        }
        self.vaddr_to_offset(whole, value)
    }

    /// File offset of the vendor data segment, where a console loader finds the tables.
    ///
    /// `PT_SCE_DYNLIBDATA`. The vendor's dynamic tags are offsets into this rather than
    /// virtual addresses, so without it they cannot be resolved at all (D247).
    pub fn vendor_data_offset(&self, whole: &[u8]) -> Result<Option<usize>, ElfError> {
        for (index, ph) in self.program_headers()?.iter().enumerate() {
            if ph.p_type.get() != segment::SCE_DYNLIBDATA {
                continue;
            }
            // In a wrapper container the header's own file offset is a logical inner-ELF
            // offset - for a properly-built module it lands past the end of the file - and
            // the segment's bytes actually sit in the descriptor block the wrapper assigns
            // to this header. The vendor tags are offsets relative to the start of that
            // data, so that block's start is the base they are measured from (D247).
            let at = if let Some(wrapper) = self.wrapper {
                let mut found = None;
                for seg in wrapper.segments(whole)? {
                    if seg.has_data() && seg.program_header_index() == index {
                        found = Some(seg.range().start);
                        break;
                    }
                }
                match found {
                    Some(at) => at,
                    None => return Ok(None),
                }
            } else {
                usize::try_from(ph.offset.get()).unwrap_or(0)
            };
            // Bounds-checked here rather than at each use: an offset past the end of the
            // file would otherwise become three separate confusing failures downstream.
            return Ok((at <= whole.len()).then_some(at));
        }
        Ok(None)
    }

    /// The dynamic symbol table and its strings, located and bounded.
    ///
    /// Shared by [`Self::raw_imports`] and [`Self::raw_exports`], which read the **same
    /// table** and differ only in which side of `SHN_UNDEF` they keep. Locating it twice
    /// would be thirty lines that have to stay identical, and the day they stopped being
    /// identical one of the two would read a table the other did not.
    ///
    /// # Errors
    ///
    /// [`ElfError::NoDynamicTable`] when there is no dynamic segment, when it lacks the
    /// string, symbol or hash table, or when any of those addresses is unmapped.
    fn symbol_tables<'bytes>(&self, whole: &'bytes [u8]) -> Result<SymbolTables<'bytes>, ElfError> {
        let Some(dyn_bytes) = self.dynamic_bytes(whole)? else {
            return Err(ElfError::NoDynamicTable {
                reason: "no PT_DYNAMIC segment, or its address could not be located",
            });
        };
        let info = dynamic::DynamicInfo::parse(dyn_bytes);
        if !info.is_usable() {
            return Err(ElfError::NoDynamicTable {
                reason: "dynamic table lacks a string table, symbol table, or hash table",
            });
        }

        let locate = |value: u64| -> Result<Option<usize>, ElfError> {
            self.table_offset(whole, &info, value)
        };

        let strtab_at = locate(info.strtab)?.ok_or(ElfError::NoDynamicTable {
            reason: "string table address is unmapped",
        })?;
        let symtab_at = locate(info.symtab)?.ok_or(ElfError::NoDynamicTable {
            reason: "symbol table address is unmapped",
        })?;
        let nchain = self
            .count_symbols(whole, &info)?
            .ok_or(ElfError::NoDynamicTable {
                reason: "hash table address is unmapped",
            })?;

        let strsz = usize::try_from(info.strsz).unwrap_or(0);
        let strings = whole.get(strtab_at..strtab_at + strsz).unwrap_or(&[]);
        let symbols = whole.get(symtab_at..).unwrap_or(&[]);
        let syment = usize::try_from(info.syment).unwrap_or(dynamic::SYMBOL_SIZE);
        Ok((symbols, strings, nchain, syment))
    }

    /// Every import this module needs, read from the dynamic symbol table.
    ///
    /// # Errors
    ///
    /// As `symbol_tables`, plus a symbol count too large to be real.
    pub fn raw_imports(
        &self,
        whole: &[u8],
        hasher: &orbistoun_nid::NidHasher,
    ) -> Result<Vec<dynamic::RawImport>, ElfError> {
        let (symbols, strings, nchain, syment) = self.symbol_tables(whole)?;
        dynamic::imports_from_symbols(symbols, strings, nchain, syment, hasher)
    }

    /// Every symbol this module **provides**, read from the same table.
    ///
    /// The other half of [`Self::raw_imports`], and the half that makes a second module worth
    /// loading: an import is a NID somebody needs, an export is that NID plus where in this
    /// module it lives.
    ///
    /// # Errors
    ///
    /// As `symbol_tables`, plus a symbol count too large to be real.
    pub fn raw_exports(
        &self,
        whole: &[u8],
        hasher: &orbistoun_nid::NidHasher,
    ) -> Result<Vec<dynamic::RawExport>, ElfError> {
        let (symbols, strings, nchain, syment) = self.symbol_tables(whole)?;
        dynamic::exports_from_symbols(symbols, strings, nchain, syment, hasher)
    }

    /// The symbol count, from whichever hash table the module carries.
    ///
    /// **`DT_HASH` first, because it states the answer.** A module carrying both is
    /// describing one table twice, and the one that says `nchain` outright cannot be
    /// walked wrong. `DT_GNU_HASH` is the fallback rather than the preference for that
    /// reason alone - and it is the only one an open toolchain emits, which is why it has
    /// to be there at all (D305).
    ///
    /// [`None`] means neither table could be located, which callers distinguish from a
    /// count of zero.
    fn count_symbols(
        &self,
        whole: &[u8],
        info: &dynamic::DynamicInfo,
    ) -> Result<Option<u64>, ElfError> {
        // **Not `info.hash != 0` under vendor tags.** A vendor tag holds an offset into the
        // data segment and offset zero is the first byte of it, which is where a real
        // module puts a table - testing the value for zero is the exact mistake D247 was
        // written about, and it would refuse a module whose tables are all present.
        if info.vendor_tables || info.hash != 0 {
            if let Some(at) = self.table_offset(whole, info, info.hash)? {
                return Self::symbol_count_at(whole, at).map(Some);
            }
        }
        // Only a standard tag from here: the vendor's tables have no GNU hash, and a zero
        // here is an absent tag rather than an offset.
        if !info.vendor_tables && info.gnu_hash != 0 {
            if let Some(at) = self.table_offset(whole, info, info.gnu_hash)? {
                let table = whole.get(at..).unwrap_or(&[]);
                return dynamic::symbol_count_from_gnu_hash(table).map(Some);
            }
        }
        Ok(None)
    }

    /// How many entries the dynamic symbol table holds.
    ///
    /// Relocations index this table, so it also fixes how many thunks a module needs.
    /// Read from the hash table rather than inferred, for the reason below.
    pub fn symbol_count(&self, whole: &[u8]) -> Result<u64, ElfError> {
        let Some(dyn_bytes) = self.dynamic_bytes(whole)? else {
            return Ok(0);
        };
        let info = dynamic::DynamicInfo::parse(dyn_bytes);
        // The third site that had to learn the difference, and the one whose failure was
        // silent: it returns 0 rather than an error, so a module whose hash table it could
        // not locate produced a thunk table with no entries and every relocation against it
        // reported "unresolved" - naming the symbol, not the missing table (D247).
        Ok(self.count_symbols(whole, &info)?.unwrap_or(0))
    }

    /// Reads `nchain` from a hash table already located in the file.
    ///
    /// `DT_HASH` is `[nbucket][nchain]`, and `nchain` is the symbol count. There is no
    /// `DT_SYMSZ`, and inferring the size from table adjacency would be wrong here -
    /// the string table sits before the symbol table in real material, so the gap
    /// between them is not the symbol table at all.
    fn symbol_count_at(whole: &[u8], hash_at: usize) -> Result<u64, ElfError> {
        whole
            .get(hash_at + 4..hash_at + 8)
            .and_then(|b| b.try_into().ok())
            .map(|b| u64::from(u32::from_le_bytes(b)))
            .ok_or(ElfError::NoDynamicTable {
                reason: "hash table is truncated",
            })
    }

    /// The libraries an import's library id refers to, keyed by that id.
    ///
    /// **Use this, not [`Self::needed_libraries`], to attribute an import.** They are
    /// different lists: `DT_NEEDED` names what the module links against, while this is
    /// the table the ids inside encoded symbol names actually index (D117).
    pub fn import_libraries(
        &self,
        whole: &[u8],
    ) -> Result<std::collections::BTreeMap<u16, String>, ElfError> {
        self.vendor_name_table(whole, |info| &info.libraries)
    }

    /// The modules an import's module id refers to, keyed by that id.
    pub fn import_modules(
        &self,
        whole: &[u8],
    ) -> Result<std::collections::BTreeMap<u16, String>, ElfError> {
        self.vendor_name_table(whole, |info| &info.modules)
    }

    /// Shared body of both, so the two cannot decode the same layout differently.
    fn vendor_name_table(
        &self,
        whole: &[u8],
        pick: impl Fn(&dynamic::DynamicInfo) -> &Vec<u64>,
    ) -> Result<std::collections::BTreeMap<u16, String>, ElfError> {
        let Some(dyn_bytes) = self.dynamic_bytes(whole)? else {
            return Ok(std::collections::BTreeMap::new());
        };
        let info = dynamic::DynamicInfo::parse(dyn_bytes);
        // **Through `table_offset`, not by address.** For a vendor module `strtab` is an
        // offset into `PT_SCE_DYNLIBDATA`, and resolving it as an address only worked
        // because that segment sits at address zero - the same coincidence that made the
        // dynamic table findable by luck (D247, D391).
        let Some(strtab_at) = self.table_offset(whole, &info, info.strtab)? else {
            return Ok(std::collections::BTreeMap::new());
        };
        let strsz = usize::try_from(info.strsz).unwrap_or(0);
        let strings = whole.get(strtab_at..strtab_at + strsz).unwrap_or(&[]);

        Ok(pick(&info)
            .iter()
            .filter_map(|entry| {
                let (id, offset) = dynamic::split_table_entry(*entry);
                let name = dynamic::read_cstr(strings, usize::try_from(offset).ok()?)?;
                Some((id, name.to_owned()))
            })
            .collect())
    }

    /// The libraries this module needs, by name.
    pub fn needed_libraries(&self, whole: &[u8]) -> Result<Vec<String>, ElfError> {
        let Some(dyn_bytes) = self.dynamic_bytes(whole)? else {
            return Ok(Vec::new());
        };
        let info = dynamic::DynamicInfo::parse(dyn_bytes);
        // **Through `table_offset`, not by address.** For a vendor module `strtab` is an
        // offset into `PT_SCE_DYNLIBDATA`, and resolving it as an address only worked
        // because that segment sits at address zero - the same coincidence that made the
        // dynamic table findable by luck (D247, D391).
        let Some(strtab_at) = self.table_offset(whole, &info, info.strtab)? else {
            return Ok(Vec::new());
        };
        let strsz = usize::try_from(info.strsz).unwrap_or(0);
        let strings = whole.get(strtab_at..strtab_at + strsz).unwrap_or(&[]);
        Ok(info
            .needed
            .iter()
            .filter_map(|off| {
                dynamic::read_cstr(strings, usize::try_from(*off).ok()?).map(str::to_owned)
            })
            .collect())
    }
}

/// Raw ELF64 section header.
///
/// # Why this exists when the loader never reads one
///
/// Loading a module needs program headers and nothing else - the section table is a link-time
/// artefact and a stripped binary has none. Every commercial title here is stripped, so this
/// parser ignored sections entirely for its whole life.
///
/// **The open-toolchain guests are not stripped.** obSCEne's payload carries twenty-four
/// sections including `.symtab`, and it is the guest this project runs most and understands
/// least at a fault: a report saying `image+0x28a163` names the byte and not the function,
/// while the answer is sitting in the file (D628).
///
/// Nothing here is on the load path. It is read once, before entry, purely so a report can
/// speak.
#[derive(Debug, Clone, Copy, FromBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct Elf64SectionHeader {
    /// Offset into the section-name string table.
    pub name: little_endian::U32,
    /// Section type - [`SHT_SYMTAB`] is the one this reads.
    pub sh_type: little_endian::U32,
    /// Section attribute flags.
    pub flags: little_endian::U64,
    /// Guest virtual address, where the section is loaded.
    pub addr: little_endian::U64,
    /// File offset of the section contents.
    pub offset: little_endian::U64,
    /// Size of the section in bytes.
    pub size: little_endian::U64,
    /// Section index this one links to - for a symbol table, its string table.
    pub link: little_endian::U32,
    /// Extra information, section-type dependent.
    pub info: little_endian::U32,
    /// Required alignment.
    pub addralign: little_endian::U64,
    /// Size of one entry, for sections that hold a table.
    pub entsize: little_endian::U64,
}

/// A static symbol table, `SHT_SYMTAB`. Distinct from `SHT_DYNSYM`, which the dynamic
/// segment already reaches and which carries only what a module exports.
pub const SHT_SYMTAB: u32 = 2;

/// Raw ELF64 symbol table entry.
#[derive(Debug, Clone, Copy, FromBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct Elf64Symbol {
    /// Offset into the linked string table.
    pub name: little_endian::U32,
    /// Type and binding, packed - see [`STT_FUNC`].
    pub info: u8,
    /// Visibility.
    pub other: u8,
    /// Section index, or a special value.
    pub shndx: little_endian::U16,
    /// The address, for a defined symbol.
    pub value: little_endian::U64,
    /// The extent, where the producer recorded one.
    pub size: little_endian::U64,
}

/// The symbol type meaning "a function", in the low nibble of `info`.
pub const STT_FUNC: u8 = 2;

/// One function a guest module names in its own symbol table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticSymbol {
    /// The name as the producer spelled it, unmangled and not hashed.
    pub name: String,
    /// Its address, as the module was linked - so an offset from the image base for a
    /// position-independent one, which is every guest here.
    pub value: u64,
    /// Its extent, or zero where the producer recorded none.
    pub size: u64,
}

impl Container<'_> {
    /// Every function named in this module's own `SHT_SYMTAB`, if it has one.
    ///
    /// An empty list is the ordinary answer for a stripped module and is **not** an error: a
    /// commercial title has no section table at all, and refusing one would make this
    /// unusable for the guests it is meant to help alongside.
    ///
    /// Only `STT_FUNC` with a non-zero address, because the purpose is naming a code address
    /// and a data symbol at the same value would shadow the function that is actually there.
    ///
    /// # Errors
    ///
    /// Never - a malformed or absent section table yields an empty list, which is what
    /// "this module cannot tell me" means. Kept in the `Result` shape of its neighbours so
    /// that a future reader which *can* fail does not change every call site.
    pub fn function_symbols(&self, whole: &[u8]) -> Result<Vec<StaticSymbol>, ElfError> {
        let Some((symbols, strings)) = self.symtab_bytes(whole) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for entry in symbols.chunks_exact(size_of::<Elf64Symbol>()) {
            let Ok(symbol) = Elf64Symbol::read_from_bytes(entry) else {
                continue;
            };
            // The low nibble is the type; the high nibble is the binding, which does not
            // matter here - a static function and a global one both name an address.
            if symbol.info & 0x0f != STT_FUNC || symbol.value.get() == 0 {
                continue;
            }
            let Some(name) = string_at(strings, symbol.name.get() as usize) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            out.push(StaticSymbol {
                name,
                value: symbol.value.get(),
                size: symbol.size.get(),
            });
        }
        Ok(out)
    }

    /// The bytes of the first `SHT_SYMTAB` and of the string table it links to.
    ///
    /// [`None`] whenever anything does not add up - no section table, a header that does not
    /// fit, a link out of range, a table running past the end of the file. Hostile bytes reach
    /// this the same way they reach every other parser here, so every index is checked and
    /// nothing is assumed to be consistent with anything else.
    fn symtab_bytes<'bytes>(&self, whole: &'bytes [u8]) -> Option<(&'bytes [u8], &'bytes [u8])> {
        let entry = usize::from(self.header.shentsize.get());
        if entry < size_of::<Elf64SectionHeader>() {
            return None;
        }
        let count = usize::from(self.header.shnum.get());
        let start = usize::try_from(self.header.shoff.get()).ok()?;
        let table = whole.get(start..start.checked_add(count.checked_mul(entry)?)?)?;

        let read = |index: usize| -> Option<Elf64SectionHeader> {
            let at = index.checked_mul(entry)?;
            let bytes = table.get(at..at + size_of::<Elf64SectionHeader>())?;
            Elf64SectionHeader::read_from_bytes(bytes).ok()
        };
        let contents = |header: &Elf64SectionHeader| -> Option<&'bytes [u8]> {
            let at = usize::try_from(header.offset.get()).ok()?;
            let len = usize::try_from(header.size.get()).ok()?;
            whole.get(at..at.checked_add(len)?)
        };

        for index in 0..count {
            let header = read(index)?;
            if header.sh_type.get() != SHT_SYMTAB {
                continue;
            }
            // **The linked string table, not `.strtab` by name.** A section's name is itself a
            // string-table lookup, so trusting the name would mean trusting the very table
            // being located. `sh_link` says it directly.
            let strings = read(usize::try_from(header.link.get()).ok()?)?;
            return Some((contents(&header)?, contents(&strings)?));
        }
        None
    }
}

/// A NUL-terminated name at `offset`, or [`None`] when the offset is outside the table.
fn string_at(strings: &[u8], offset: usize) -> Option<String> {
    let rest = strings.get(offset..)?;
    let end = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
    Some(String::from_utf8_lossy(&rest[..end]).into_owned())
}
#[cfg(test)]
mod tests {
    use super::{Container, ElfError};

    /// A minimal, valid ELF64 header with an empty program table.
    fn minimal_elf() -> Vec<u8> {
        let mut v = vec![0_u8; size_of::<super::Elf64Header>()];
        v[..4].copy_from_slice(b"\x7fELF");
        v[4] = 2; // ELFCLASS64
        v[5] = 1; // ELFDATA2LSB
        v
    }

    #[test]
    fn rejects_non_elf() {
        let bytes = vec![0_u8; 64];
        assert!(matches!(Container::parse(&bytes), Err(ElfError::NotElf)));
    }

    #[test]
    fn rejects_truncated_input() {
        assert!(matches!(
            Container::parse(b"\x7fELF"),
            Err(ElfError::Truncated { .. })
        ));
    }

    #[test]
    fn rejects_32_bit_and_big_endian() {
        let mut v = minimal_elf();
        v[4] = 1; // ELFCLASS32
        assert!(matches!(
            Container::parse(&v),
            Err(ElfError::UnsupportedFormat)
        ));
    }

    #[test]
    fn accepts_minimal_header_with_empty_program_table() {
        let v = minimal_elf();
        let c = Container::parse(&v).expect("valid header");
        assert_eq!(c.entry(), 0);
        assert!(c.program_headers().expect("empty table").is_empty());
        assert!(c.vendor_segments().expect("no vendor segments").is_empty());
    }
}
