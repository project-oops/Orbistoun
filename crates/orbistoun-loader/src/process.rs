//! The process entry image: what a program finds when it starts.
//!
//! A program's first instruction expects the stack to hold its arguments, its environment
//! and the auxiliary vector, and reads them before calling anything. The layout is the
//! System V AMD64 ABI's initial process stack and the auxiliary vector types are the ELF
//! gABI's, the documented convention for a FreeBSD-derived kernel. Whether the vendor's
//! runtime entry follows that convention is unpublished, so this module builds the standard
//! image and leaves how it is presented to [`EntrySettings`] (D159).

/// Auxiliary vector types, from the ELF generic ABI.
///
/// Published values, shared across every System V ELF platform. Only the entries a starting
/// program reads are listed.
pub mod aux {
    /// End of the vector. Every auxiliary vector is terminated by this.
    pub const AT_NULL: u64 = 0;
    /// Ignore this entry.
    pub const AT_IGNORE: u64 = 1;
    /// Address of the program headers in the loaded image.
    pub const AT_PHDR: u64 = 3;
    /// Size of one program header entry.
    pub const AT_PHENT: u64 = 4;
    /// Number of program headers.
    pub const AT_PHNUM: u64 = 5;
    /// System page size.
    pub const AT_PAGESZ: u64 = 6;
    /// Base address the interpreter was loaded at.
    pub const AT_BASE: u64 = 7;
    /// Flags.
    pub const AT_FLAGS: u64 = 8;
    /// The program's own entry point.
    pub const AT_ENTRY: u64 = 9;
}

/// Stack alignment at the entry point.
///
/// The System V ABI requires `rsp` sixteen-byte aligned at the entry point itself, where an
/// ordinary function finds it eight past alignment because a call pushed a return address.
/// That difference is why a process cannot simply be called; see [`Convention`].
pub const ENTRY_ALIGN: u64 = 16;

/// One auxiliary vector entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuxEntry {
    /// Which value this is.
    pub kind: u64,
    /// The value.
    pub value: u64,
}

/// How control is transferred to the entry point.
///
/// What the vendor's entry point expects is unpublished and the guest is the only oracle,
/// so both readings are available as configuration (D159).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Convention {
    /// Jump, with `rsp` pointing at the argument count.
    ///
    /// The System V process convention, as a FreeBSD `_start` documents. Nothing returns
    /// from it; a program leaves by calling exit.
    Process,
    /// Call it as an ordinary function.
    ///
    /// The default, established by measurement (D159). Entering by jump leaves `rsp`
    /// sixteen-byte aligned at the first instruction, as for a process; entering by call
    /// leaves it eight past, as for a function. The guest carries whichever it was given
    /// through every frame, and its calls back into orbistoun conform to the ABI's stack
    /// alignment only when it is entered by call.
    #[default]
    Function,
}

/// What the entry point is given in its first argument register.
///
/// The entry point reads this register; what it expects to find is unpublished, so it is
/// a setting (D159).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryArgument {
    /// The address of the process image - where `rsp` points.
    ///
    /// The default. A runtime whose entry takes a pointer to its startup information gets
    /// exactly this, and it is the only candidate derived from something.
    #[default]
    ImageAddress,
    /// A zeroed block that is not the image.
    ///
    /// Every field reads as zero, so a pointer field is null and a guest that checks it
    /// takes its own error path.
    ZeroedBlock,
    /// Nothing at all.
    ///
    /// Keeps "does the entry need this register" answerable; the guest faults immediately
    /// under it.
    Zero,
    /// A block whose every slot holds a different, identifiable marker.
    ///
    /// A diagnostic: the address the guest faults on names the slot it read (D365). A run
    /// under it is not an ordinary run and is not compared with one.
    Sentinels,
    /// `argc` and `argv`, as an ordinary C `main` takes them.
    ///
    /// For entering at `main` rather than at the declared entry (D343). `main` is a C
    /// function with a documented signature, not a process entry point, so it gets a count
    /// and a vector rather than a process-argument block. Both come from the process image.
    MainArguments,
    /// A block whose every slot points at a function that returns zero.
    ///
    /// The companion to [`Self::Sentinels`]: shows how far the guest gets when every field
    /// it asks for answers harmlessly (D365). A diagnostic, not an ordinary run.
    Answering,
    /// A block whose every slot points at a function that says how it was called.
    ///
    /// Every slot answers zero and prints the slot number and the guest's first three
    /// arguments, showing what was passed (D365). A diagnostic, not an ordinary run.
    Reporting,
    /// The structure a payload's runtime is handed, as far as it is known.
    ///
    /// Field zero holds the name resolver, which a payload calls with the string
    /// `sceKernelDlsym`; every later field keeps a marker (D365). A run under it is
    /// ordinary until the guest reaches a marker field, which the fault address names.
    Handoff,
}

/// How the process entry is presented to the guest.
///
/// All settings rather than constants: only the entry point reading its first argument
/// register is established, and the rest is tested by relaunching with a different file
/// (D159).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct EntrySettings {
    /// Whether the entry point is jumped to or called.
    pub convention: Convention,
    /// What it finds in its first argument register.
    pub argument: EntryArgument,
    /// Environment strings, in `NAME=value` form.
    ///
    /// Empty by default. What the platform sets is unknown, and invented variables steer a
    /// guest down a path nobody chose.
    pub environment: Vec<String>,
    /// Start somewhere other than the declared entry point, image-relative.
    ///
    /// A diagnostic, not a claim about how the platform starts a program (D343): entering
    /// at a sized `main` symbol skips runtime startup code whose handoff structure cannot be
    /// derived. [`None`] is the declared entry; `Some(0)` is a real request, since an image's
    /// first byte is a legitimate address. The address must lie in an executable segment.
    pub at: Option<u64>,
    /// Values to put in named fields of the handoff structure, as `[field, value]` pairs.
    ///
    /// [`EntryArgument::Handoff`] fills field zero with the resolver and marks the rest; this
    /// gives other fields a literal per run, without a rebuild. `[[2, 0]]` puts a null in
    /// field two. Applied over the chosen argument block, so the resolver stays unless field
    /// zero is named. Only the handoff block has fields; other variants ignore this.
    pub handoff_fields: Vec<[u64; 2]>,
    /// Extra auxiliary vector entries, as `[type, value]` pairs.
    ///
    /// The derivable entries (entry point, load base, page size) come from the image. This
    /// is for a value the image cannot supply.
    pub extra_auxiliary: Vec<[u64; 2]>,
}

/// Everything a starting program is told about itself.
#[derive(Debug, Clone, Default)]
pub struct Description {
    /// Argument strings. The first is conventionally the program's own path.
    pub arguments: Vec<String>,
    /// Environment strings, each already in `NAME=value` form.
    pub environment: Vec<String>,
    /// Auxiliary vector entries. `AT_NULL` is appended automatically.
    pub auxiliary: Vec<AuxEntry>,
}

/// A built image, ready to be written into guest memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Where the image starts, and what `rsp` must be at the entry point.
    pub stack_pointer: u64,
    /// The bytes, covering `stack_pointer` up to the stack top it was built against.
    pub bytes: Vec<u8>,
}

/// Rounds down to a multiple of `align`.
const fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

/// Builds the initial process stack below `top`.
///
/// The layout, from the top down, is the System V one: the strings, then the auxiliary
/// vector, the environment pointers, the argument pointers, and the count, so `rsp` at
/// entry points at the count and everything else is found by walking upward.
///
/// Returns `None` if the description does not fit in `available` bytes.
pub fn build(top: u64, available: u64, description: &Description) -> Option<Image> {
    // The strings first, because everything else points at them.
    let mut blobs: Vec<Vec<u8>> = Vec::new();
    for text in description
        .arguments
        .iter()
        .chain(description.environment.iter())
    {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        blobs.push(bytes);
    }
    let strings_len: u64 = blobs.iter().map(|b| b.len() as u64).sum();
    let strings_base = align_down(top.checked_sub(strings_len)?, 8);

    // Each string's address, in the order they were laid out.
    let mut addresses = Vec::with_capacity(blobs.len());
    let mut cursor = strings_base;
    for blob in &blobs {
        addresses.push(cursor);
        cursor += blob.len() as u64;
    }

    // Then the vectors, whose size is fixed by the counts.
    let argc = description.arguments.len() as u64;
    let envc = description.environment.len() as u64;
    let auxc = description.auxiliary.len() as u64 + 1; // the AT_NULL terminator
    let words = 1 + argc + 1 + envc + 1 + auxc * 2;
    let vectors_len = words * 8;

    // This address is `rsp` at the entry point, where System V requires the alignment.
    let stack_pointer = align_down(strings_base.checked_sub(vectors_len)?, ENTRY_ALIGN);
    if top.checked_sub(stack_pointer)? > available {
        return None;
    }

    let mut bytes = vec![0_u8; (top - stack_pointer) as usize];
    let mut put = |address: u64, value: u64| {
        let at = (address - stack_pointer) as usize;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    };

    let mut at = stack_pointer;
    put(at, argc);
    at += 8;
    for address in addresses.iter().take(argc as usize) {
        put(at, *address);
        at += 8;
    }
    // The NULL that ends the argument vector, already zero.
    at += 8;
    for address in addresses.iter().skip(argc as usize) {
        put(at, *address);
        at += 8;
    }
    at += 8;
    for entry in &description.auxiliary {
        put(at, entry.kind);
        put(at + 8, entry.value);
        at += 16;
    }
    // AT_NULL, 0: already zero, and the terminator the standard requires.

    // Finally the strings, copied wholesale.
    for (blob, address) in blobs.iter().zip(addresses) {
        let start = (address - stack_pointer) as usize;
        bytes[start..start + blob.len()].copy_from_slice(blob);
    }

    Some(Image {
        stack_pointer,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::{AuxEntry, Description, ENTRY_ALIGN, aux, build};

    /// High enough that nothing underflows.
    const TOP: u64 = 0x0000_7000_0000_0000;
    /// Room to spare.
    const ROOM: u64 = 64 * 1024;

    fn read_word(image: &super::Image, address: u64) -> u64 {
        let at = (address - image.stack_pointer) as usize;
        u64::from_le_bytes(image.bytes[at..at + 8].try_into().expect("eight bytes"))
    }

    fn read_string(image: &super::Image, address: u64) -> String {
        let at = (address - image.stack_pointer) as usize;
        let end = image.bytes[at..]
            .iter()
            .position(|b| *b == 0)
            .expect("terminated");
        String::from_utf8(image.bytes[at..at + end].to_vec()).expect("text")
    }

    fn sample() -> Description {
        Description {
            arguments: vec!["/app0/eboot.bin".to_owned()],
            environment: vec!["LANG=en".to_owned()],
            auxiliary: vec![
                AuxEntry {
                    kind: aux::AT_PAGESZ,
                    value: 16384,
                },
                AuxEntry {
                    kind: aux::AT_ENTRY,
                    value: 0x4000_0000_0080,
                },
            ],
        }
    }

    /// Entry settings round-trip through a file, and an empty file is valid.
    #[test]
    fn entry_settings_survive_a_round_trip_and_an_empty_file() {
        // These are edited between runs, so a partial file must load.
        let chosen = super::EntrySettings {
            convention: super::Convention::Function,
            argument: super::EntryArgument::ZeroedBlock,
            // `Some(0)` rather than `None`: offset zero means "start here", not "no
            // preference" (D343).
            at: Some(0),
            environment: vec!["LANG=en".to_owned()],
            handoff_fields: vec![[2, 0]],
            extra_auxiliary: vec![[aux::AT_PAGESZ, 4096]],
        };
        let text = toml::to_string(&chosen).expect("serialises");
        let back: super::EntrySettings = toml::from_str(&text).expect("reads back");
        assert_eq!(back, chosen);

        let empty: super::EntrySettings = toml::from_str("").expect("an empty file is valid");
        assert_eq!(empty, super::EntrySettings::default());
    }

    /// `rsp` is sixteen-byte aligned at the entry point.
    #[test]
    fn the_stack_pointer_is_aligned_as_the_standard_requires() {
        // A misaligned stack faults later, in the first aligned vector instruction.
        let image = build(TOP, ROOM, &sample()).expect("fits");
        assert_eq!(image.stack_pointer % ENTRY_ALIGN, 0);
    }

    /// The argument count is at the stack pointer.
    #[test]
    fn the_count_is_at_the_stack_pointer() {
        let image = build(TOP, ROOM, &sample()).expect("fits");
        assert_eq!(read_word(&image, image.stack_pointer), 1);
    }

    /// The argument vector points at the strings and ends with NULL.
    #[test]
    fn the_argument_vector_points_at_real_strings_and_ends_with_null() {
        let image = build(TOP, ROOM, &sample()).expect("fits");
        let argv0 = read_word(&image, image.stack_pointer + 8);
        assert_eq!(read_string(&image, argv0), "/app0/eboot.bin");
        assert_eq!(
            read_word(&image, image.stack_pointer + 16),
            0,
            "the argument vector must be terminated"
        );
    }

    /// The environment vector follows the arguments and is NULL-terminated.
    #[test]
    fn the_environment_follows_the_arguments_and_is_also_terminated() {
        let image = build(TOP, ROOM, &sample()).expect("fits");
        // argc, argv[0], NULL, envp[0], NULL, then the auxiliary vector.
        let envp0 = read_word(&image, image.stack_pointer + 24);
        assert_eq!(read_string(&image, envp0), "LANG=en");
        assert_eq!(read_word(&image, image.stack_pointer + 32), 0);
    }

    /// The auxiliary vector is pairs terminated by `AT_NULL`.
    #[test]
    fn the_auxiliary_vector_is_pairs_and_ends_with_at_null() {
        let image = build(TOP, ROOM, &sample()).expect("fits");
        let auxv = image.stack_pointer + 40;
        assert_eq!(read_word(&image, auxv), aux::AT_PAGESZ);
        assert_eq!(read_word(&image, auxv + 8), 16384);
        assert_eq!(read_word(&image, auxv + 16), aux::AT_ENTRY);
        assert_eq!(read_word(&image, auxv + 24), 0x4000_0000_0080);
        assert_eq!(read_word(&image, auxv + 32), aux::AT_NULL, "terminated");
        assert_eq!(read_word(&image, auxv + 40), 0);
    }

    /// Every pointer in the image points inside the image.
    #[test]
    fn everything_lives_inside_the_image() {
        let image = build(TOP, ROOM, &sample()).expect("fits");
        let end = image.stack_pointer + image.bytes.len() as u64;
        for offset in [8_u64, 24] {
            let pointer = read_word(&image, image.stack_pointer + offset);
            assert!(
                pointer >= image.stack_pointer && pointer < end,
                "{pointer:#x} points outside the image"
            );
        }
    }

    /// An image too big for the available stack is refused.
    #[test]
    fn an_image_too_big_for_the_stack_is_refused_rather_than_built() {
        // Building it would write through the guard page below the stack.
        let mut huge = sample();
        huge.environment = (0..4096)
            .map(|i| format!("VAR{i}=padding-padding"))
            .collect();
        assert!(build(TOP, 1024, &huge).is_none());
    }

    /// An empty description still yields a count, empty vectors and a terminated
    /// auxiliary vector.
    #[test]
    fn an_empty_description_still_produces_a_valid_image() {
        let image = build(TOP, ROOM, &Description::default()).expect("fits");
        assert_eq!(image.stack_pointer % ENTRY_ALIGN, 0);
        assert_eq!(read_word(&image, image.stack_pointer), 0, "no arguments");
        assert_eq!(read_word(&image, image.stack_pointer + 8), 0, "argv NULL");
        assert_eq!(read_word(&image, image.stack_pointer + 16), 0, "envp NULL");
        assert_eq!(
            read_word(&image, image.stack_pointer + 24),
            aux::AT_NULL,
            "and an auxiliary vector that is just its terminator"
        );
    }
}
