//! The process entry image: what a program finds when it starts.
//!
//! # Why this exists
//!
//! An entry point is not an ordinary function. A program's first instruction expects the
//! stack to already hold its arguments, its environment, and a description of how it was
//! loaded - and it reads them immediately, before it has called anything.
//!
//! This was known and deliberately deferred, on the reasoning that a guest which only has
//! to execute a few instructions does not need its environment. That was wrong, and the
//! measurement that settled it was an accident: giving the entry point a *defined* value
//! in the first argument register instead of an undefined one made two unrelated titles
//! fault at the identical offset. They had been reading through a stray host pointer and
//! getting plausible garbage (D152).
//!
//! # What is citable here, and what is not
//!
//! **Citable:** the layout below is the System V AMD64 ABI's initial process stack, which
//! is a published standard, and the auxiliary vector types are the ELF gABI's. The target
//! kernel is FreeBSD-derived, so this is the documented convention for it rather than a
//! guess - principle 1's best case, and rare in this project.
//!
//! **Not citable:** whether the *vendor's* entry point follows that convention. Its
//! runtime is its own, and no published document describes it. What is established is
//! only that it dereferences the first argument register immediately.
//!
//! So this module builds the standard image faithfully and takes no position on how it is
//! presented. That choice is configuration, and it is an experiment to be run rather than
//! an assumption to be embedded - see `Convention` (D153).

/// Auxiliary vector types, from the ELF generic ABI.
///
/// Published values, shared across every System V ELF platform. Only the entries a
/// starting program actually reads are listed; the rest would be noise.
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
/// Sixteen, and it is stricter than it looks: the System V ABI requires `rsp` to be
/// sixteen-byte aligned *at the entry point itself*, where an ordinary function finds it
/// eight past alignment because a call pushed a return address. That difference is the
/// reason a process cannot simply be `call`ed - see [`Convention`].
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
/// **A configuration choice standing in for an experiment, not a decision.** What the
/// vendor's entry point expects is not published, and the guest is the only oracle - so
/// both readings are available and the one that gets further wins on evidence (D153).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Convention {
    /// Jump, with `rsp` pointing at the argument count.
    ///
    /// The System V *process* convention, and what a FreeBSD `_start` documents. It is no
    /// longer the default: measurement says this target does not use it (D159). Nothing
    /// returns from it - a program leaves by calling exit.
    Process,
    /// Call it as an ordinary function.
    ///
    /// **The default, and it is measured rather than assumed.** Entering by jump leaves
    /// `rsp` sixteen-byte aligned at the first instruction, which is right for a process;
    /// entering by call leaves it eight past, which is right for a function. The guest
    /// then carries whichever it was given through every frame it builds, and the
    /// difference shows up in the stack alignment of the calls it makes back to us:
    ///
    /// | convention | conforming calls |
    /// |---|---|
    /// | `Process` | 2 of 372 |
    /// | `Function` | 372 of 372 |
    ///
    /// So this target's entry point is called, not jumped to. Nothing published says so -
    /// it was established by measuring the guest, which is the oracle this project has
    /// (D159).
    #[default]
    Function,
}

/// What the entry point is given in its first argument register.
///
/// The one thing measurement established is that the entry point *reads* this register.
/// What it expects to find is open, so it is a setting (D153).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryArgument {
    /// The address of the process image - where `rsp` points.
    ///
    /// The default. A runtime whose entry takes a pointer to its startup information
    /// would be handed exactly this, and it is the only candidate that is *derived* from
    /// something rather than invented.
    #[default]
    ImageAddress,
    /// A zeroed block that is not the image.
    ///
    /// Honest about knowing nothing: every field reads as zero, so a pointer field is
    /// null and a guest that checks it takes its own error path.
    ZeroedBlock,
    /// Nothing at all.
    ///
    /// Included so "does it actually need this?" stays answerable. It faults immediately
    /// on both titles measured, which is how the register's significance was found.
    Zero,
    /// A block whose every slot holds a different, identifiable marker.
    ///
    /// **A diagnostic, not a hypothesis about the layout.** The other three answer "does
    /// the entry point read this register"; this one answers **"which field does it
    /// read"**, because the address it faults on says which slot the value came from. One
    /// boot per structure rather than one per candidate offset (D308).
    ///
    /// A run under it is not an ordinary run and must not be compared with one.
    Sentinels,
    /// `argc` and `argv`, as an ordinary C `main` takes them.
    ///
    /// **For entering at `main` rather than at the declared entry** (D343). The other
    /// variants answer "what does a process entry point find in its first register"; this
    /// one answers a different question, because `main` is not a process entry point - it
    /// is a C function with a documented signature, and handing it a process-argument block
    /// as `argc` gives it a wild count to iterate over wild pointers.
    ///
    /// Both come out of the process image that is built anyway, so this invents nothing.
    MainArguments,
    /// A block whose every slot points at a function that returns zero.
    ///
    /// The companion to [`Self::Sentinels`], asking the other half of the question: not
    /// *which* field is used, but **how far the guest gets when every field it asks for
    /// answers harmlessly** (D308). Also a diagnostic, also not an ordinary run.
    Answering,
    /// A block whose every slot points at a function that says how it was called.
    ///
    /// The third question, and the one the other two cannot ask: not *which* field, and not
    /// *how far*, but **what was passed to it**. Every slot answers zero harmlessly and
    /// prints the slot number and the guest's first three arguments as it does (D365).
    ///
    /// Also a diagnostic, also not an ordinary run.
    Reporting,
    /// The structure a payload's runtime is handed, as far as it is known.
    ///
    /// **The only variant here that is partly an answer rather than wholly a question.**
    /// Field zero holds the name resolver, because a payload was measured calling field zero
    /// with the string `sceKernelDlsym`; every field after it keeps a marker, because
    /// nothing has established what any of them hold (D365, D366).
    ///
    /// A run under it *is* an ordinary run for as far as the known half takes it, and
    /// becomes a diagnostic the moment the guest reaches a field that is still a marker -
    /// which the fault address says.
    Handoff,
}

/// How the process entry is presented to the guest.
///
/// **Deliberately all settings rather than constants.** What is established by
/// measurement is only that the entry point reads its first argument register. Everything
/// else here is a hypothesis, and a hypothesis compiled in is one nobody can test - the
/// guest is the only oracle available, and consulting it costs a relaunch only if these
/// live in a file (principle 5, D153).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct EntrySettings {
    /// Whether the entry point is jumped to or called.
    pub convention: Convention,
    /// What it finds in its first argument register.
    pub argument: EntryArgument,
    /// Environment strings, in `NAME=value` form.
    ///
    /// Empty by default. Nothing is known about what the platform sets, and inventing
    /// plausible variables is how a guest ends up taking a path nobody chose for it.
    pub environment: Vec<String>,
    /// Start somewhere other than the declared entry point, image-relative.
    ///
    /// **A diagnostic, and not a claim about how the platform starts a program.** It exists
    /// for one question: the open-toolchain payloads all die inside their runtime's
    /// `__crt_start`, rejecting a handoff structure nothing here can supply - but `main` is
    /// a real sized symbol in three of them, so if `__crt_start` is merely unpacking that
    /// structure and calling `main`, entering at `main` skips the problem entirely (D326).
    ///
    /// [`None`] is the declared entry. `Some(0)` is a real request: an image's first byte is
    /// a legitimate address, and `klogsrv` puts `main` exactly there.
    ///
    /// Whatever it names still has to be inside an executable segment, and is refused if it
    /// is not - a diagnostic that jumps into data produces a fault about itself.
    pub at: Option<u64>,
    /// Values to put in named fields of the handoff structure, as `[field, value]` pairs.
    ///
    /// # Why this exists rather than another variant
    ///
    /// [`EntryArgument::Handoff`] answers what is *known* - field zero is the resolver - and
    /// leaves every other field as a marker. What is left is a sweep: try a value in a field,
    /// run, see whether the runtime gets further. That is a question per run, and a question
    /// per run must not be a rebuild per run (principle 5).
    ///
    /// So a field can be given a literal here. `[[2, 0]]` puts a null in field two;
    /// `[[2, 140737488355328]]` puts an address in it. Applied over whatever the chosen
    /// argument block produced, so the resolver stays where it is unless field zero is named
    /// explicitly (D375).
    ///
    /// Only meaningful for the handoff block. Naming a field under any other variant does
    /// nothing, because no other variant has fields.
    pub handoff_fields: Vec<[u64; 2]>,
    /// Extra auxiliary vector entries, as `[type, value]` pairs.
    ///
    /// The derivable ones - the entry point, the load base, the page size - are filled in
    /// from the image itself and do not belong here. This is for trying a value the image
    /// cannot supply.
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
/// The layout, from the top down, is the System V one: the strings themselves, then the
/// auxiliary vector, then the environment pointers, then the argument pointers, then the
/// count - so that `rsp` at entry points at the count and everything else is found by
/// walking upward from it.
///
/// Returns `None` if the description does not fit in `available` bytes, rather than
/// building something that runs off the end of the stack.
pub fn build(top: u64, available: u64, description: &Description) -> Option<Image> {
    // The strings first, because everything else points at them and needs their
    // addresses.
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

    // Aligned here rather than anywhere else: this address *is* `rsp` at the entry point,
    // and the System V requirement is on that address specifically.
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
    // The NULL that ends the argument vector. Already zero, but stepped over explicitly
    // so the layout reads in the same order it is documented.
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
    // AT_NULL, 0 - already zero, and the terminator the standard requires.

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

    /// Somewhere plausible, and high enough that nothing underflows.
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

    #[test]
    fn entry_settings_survive_a_round_trip_and_an_empty_file() {
        // These exist to be edited between runs. If they cannot be written and read back,
        // or if a file has to be complete to be valid, then "try the other convention and
        // relaunch" is not actually available - and the guest is the only oracle there is.
        let chosen = super::EntrySettings {
            convention: super::Convention::Function,
            argument: super::EntryArgument::ZeroedBlock,
            // Deliberately `Some(0)` rather than `None`: an image's first byte is a real
            // address two payloads put `main` at, so the round trip has to carry a zero
            // that means "start here" and not "no preference" (D343).
            at: Some(0),
            environment: vec!["LANG=en".to_owned()],
            // The sweep's own setting, round-tripped for the same reason as the rest: a
            // question per run must not be a rebuild per run (D375).
            handoff_fields: vec![[2, 0]],
            extra_auxiliary: vec![[aux::AT_PAGESZ, 4096]],
        };
        let text = toml::to_string(&chosen).expect("serialises");
        let back: super::EntrySettings = toml::from_str(&text).expect("reads back");
        assert_eq!(back, chosen);

        let empty: super::EntrySettings = toml::from_str("").expect("an empty file is valid");
        assert_eq!(empty, super::EntrySettings::default());
    }

    #[test]
    fn the_stack_pointer_is_aligned_as_the_standard_requires() {
        // Sixteen at the entry point itself, which is stricter than what an ordinary
        // function sees. A misaligned stack does not fault immediately - it faults later,
        // inside whatever first uses an aligned vector instruction, which is as far from
        // the cause as a bug can get.
        let image = build(TOP, ROOM, &sample()).expect("fits");
        assert_eq!(image.stack_pointer % ENTRY_ALIGN, 0);
    }

    #[test]
    fn the_count_is_at_the_stack_pointer() {
        // The whole layout is found by walking up from here, so if this is wrong nothing
        // else can be right.
        let image = build(TOP, ROOM, &sample()).expect("fits");
        assert_eq!(read_word(&image, image.stack_pointer), 1);
    }

    #[test]
    fn the_argument_vector_points_at_real_strings_and_ends_with_null() {
        // A pointer into the wrong place gives a program a plausible wrong name; a
        // missing terminator makes it walk off the end of the vector into the
        // environment.
        let image = build(TOP, ROOM, &sample()).expect("fits");
        let argv0 = read_word(&image, image.stack_pointer + 8);
        assert_eq!(read_string(&image, argv0), "/app0/eboot.bin");
        assert_eq!(
            read_word(&image, image.stack_pointer + 16),
            0,
            "the argument vector must be terminated"
        );
    }

    #[test]
    fn the_environment_follows_the_arguments_and_is_also_terminated() {
        let image = build(TOP, ROOM, &sample()).expect("fits");
        // argc, argv[0], NULL, envp[0], NULL, then the auxiliary vector.
        let envp0 = read_word(&image, image.stack_pointer + 24);
        assert_eq!(read_string(&image, envp0), "LANG=en");
        assert_eq!(read_word(&image, image.stack_pointer + 32), 0);
    }

    #[test]
    fn the_auxiliary_vector_is_pairs_and_ends_with_at_null() {
        // A program reads this until it sees AT_NULL. Without the terminator it reads
        // whatever follows as more entries, which is unbounded nonsense.
        let image = build(TOP, ROOM, &sample()).expect("fits");
        let auxv = image.stack_pointer + 40;
        assert_eq!(read_word(&image, auxv), aux::AT_PAGESZ);
        assert_eq!(read_word(&image, auxv + 8), 16384);
        assert_eq!(read_word(&image, auxv + 16), aux::AT_ENTRY);
        assert_eq!(read_word(&image, auxv + 24), 0x4000_0000_0080);
        assert_eq!(read_word(&image, auxv + 32), aux::AT_NULL, "terminated");
        assert_eq!(read_word(&image, auxv + 40), 0);
    }

    #[test]
    fn everything_lives_inside_the_image() {
        // The image is written into the guest's stack. Anything it points at that lies
        // outside would be a pointer into whatever happened to be there.
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

    #[test]
    fn an_image_too_big_for_the_stack_is_refused_rather_than_built() {
        // Building it anyway would write past the end of the stack, through the guard
        // page, into whatever is below - and the fault would name the wrong thing.
        let mut huge = sample();
        huge.environment = (0..4096)
            .map(|i| format!("VAR{i}=padding-padding"))
            .collect();
        assert!(build(TOP, 1024, &huge).is_none());
    }

    #[test]
    fn an_empty_description_still_produces_a_valid_image() {
        // A program with no arguments still reads a count, an empty vector, and a
        // terminated auxiliary vector. Producing nothing would leave it reading the
        // stack's uninitialised contents.
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
