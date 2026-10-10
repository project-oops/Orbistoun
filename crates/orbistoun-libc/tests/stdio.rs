//! Diagnostics, process queries, the C++ allocation operators, and the file layer.
//!
//! `puts`, `printf` and `fprintf` are how a program about to give up says why (D170).
//! `strerror` and `sysctl` answer documented failures, so a guest takes its own error path and
//! reports it. `abort` and `exit` end the process through `orbistoun_core::stop` and are not
//! called here, since they would end the test binary. The mount table and the
//! unknown-`sysctl` report are process-wide, so those tests use their own prefixes and assert
//! "at least".

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// What C's stdio answers on failure: `EOF`, widened to the register the guest reads.
const EOF: u64 = u64::MAX;
/// What the `sysctl` family answers on failure: `-1` in a 32-bit register.
const FAILED: u64 = 0xFFFF_FFFF;

/// A writable guest buffer at a real address.
struct Buf {
    storage: Vec<u8>,
    at: u64,
}

impl Buf {
    fn zeroed(size: usize) -> Self {
        Self::new(vec![0; size])
    }

    fn text(s: &str) -> Self {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        Self::new(v)
    }

    /// Four-byte words, as a `sysctl` name array is.
    fn words(values: &[u32]) -> Self {
        let mut v = Vec::new();
        for w in values {
            v.extend_from_slice(&w.to_le_bytes());
        }
        Self::new(v)
    }

    fn new(mut storage: Vec<u8>) -> Self {
        let at = storage.as_mut_ptr().expose_provenance() as u64;
        Self { storage, at }
    }

    fn at(&self) -> u64 {
        self.at
    }

    fn bytes(&self) -> &[u8] {
        &self.storage
    }
}

/// The implementation registered under `name`.
fn implementation(name: &str) -> GuestFn {
    orbistoun_libc::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(
            || panic!("{name} is not implemented, so nothing can call it"),
            |(_, f)| *f,
        )
}

/// Calls one, poisoning the argument registers it does not use.
fn call(name: &str, args: &[u64]) -> u64 {
    let mut regs = [0xDEAD_BEEF_DEAD_BEEF_u64; GUEST_ARG_REGISTERS];
    for (slot, value) in regs.iter_mut().zip(args) {
        *slot = *value;
    }
    implementation(name)(&regs)
}

/// Reads a NUL-terminated string back out of guest memory.
fn read_string(at: u64) -> String {
    let mut out = Vec::new();
    for offset in 0..1024_u64 {
        // SAFETY: an address this library returned, pointing at storage it owns for the life of
        // this thread, under the identity mapping.
        let byte = unsafe {
            std::ptr::read(std::ptr::with_exposed_provenance::<u8>(
                (at + offset) as usize,
            ))
        };
        if byte == 0 {
            break;
        }
        out.push(byte);
    }
    String::from_utf8(out).expect("these messages are ASCII")
}

// Saying why.

/// `strerror` answers a pointer to per-thread storage this library owns, so two threads'
/// messages do not overwrite each other.
#[test]
fn strerror_answers_thread_local_storage_rather_than_a_value() {
    let message = call("strerror", &[2]);
    assert_ne!(message, 0, "there must be storage behind it");

    // FreeBSD's text, as Mesa printed it for a missing file in Craft on a console (2026-10-06).
    assert_eq!(read_string(message), "No such file or directory");

    // A different code gives a different message through the same address.
    let again = call("strerror", &[7]);
    assert_eq!(again, message, "one buffer per thread, reused");
    assert_eq!(read_string(again), "Argument list too long");

    // A number the table does not hold says so, and names the number.
    let unknown = read_string(call("strerror", &[9999]));
    assert!(unknown.contains("9999"), "{unknown:?}");

    let theirs = std::thread::spawn(|| call("strerror", &[2]))
        .join()
        .expect("the thread runs");
    assert_ne!(
        theirs, message,
        "two threads reporting failures must not overwrite each other"
    );
}

/// `puts` writes its argument verbatim, not as a format, and reports what it wrote.
#[test]
fn puts_does_not_treat_its_argument_as_a_format() {
    let plain = Buf::text("starting up");
    assert_eq!(
        call("puts", &[plain.at()]),
        12,
        "eleven bytes and a newline"
    );

    // Through the renderer this would be an unsupported conversion and answer zero.
    let percent = Buf::text("100% done");
    assert_eq!(call("puts", &[percent.at()]), 10);

    assert_eq!(call("puts", &[0]), 0, "a null string writes nothing");
}

/// `printf` reports how much it rendered, and refuses what it cannot render.
#[test]
fn printf_reports_what_it_rendered_and_refuses_what_it_cannot() {
    let format = Buf::text("value %d\n");
    assert_eq!(call("printf", &[format.at(), 42]), 9);

    let floating = Buf::text("%f\n");
    assert_eq!(
        call("printf", &[floating.at(), 1]),
        0,
        "a floating-point argument never arrived in an integer register"
    );
    assert_eq!(call("printf", &[0]), 0, "a null format renders nothing");
}

/// `fprintf` drops the stream and renders the rest: the format is the second argument.
#[test]
fn fprintf_drops_the_stream_and_renders_the_rest() {
    let format = Buf::text("%s=%d\n");
    let name = Buf::text("fps");
    let stream = 0x1234_5678;

    assert_eq!(call("fprintf", &[stream, format.at(), name.at(), 60]), 7);

    // Whatever the stream is, the answer is the same.
    assert_eq!(call("fprintf", &[0, format.at(), name.at(), 60]), 7);
}

// Asking the system.

/// `getpid` answers the process the guest is actually running in.
#[test]
fn getpid_answers_the_real_process() {
    let reported = call("getpid", &[]);
    assert_eq!(reported, u64::from(std::process::id()));
    assert_ne!(reported, 0, "no real process is zero");
}

/// `sysctl` refuses what it does not know, with the documented failure, since a success
/// without a length leaves the caller an uninitialised size.
#[test]
fn sysctl_refuses_a_name_it_does_not_know() {
    let mib = Buf::words(&[1, 14]);
    let mut length: u64 = 0;
    let slot = std::ptr::from_mut(&mut length).expose_provenance() as u64;

    assert_eq!(call("sysctl", &[mib.at(), 2, 0, slot, 0, 0]), FAILED);
    assert_eq!(length, 0, "and wrote no length it could not know");
}

/// A name array that could not have come from a real process is refused before it is read.
#[test]
fn sysctl_refuses_a_name_array_it_should_not_walk() {
    let mib = Buf::words(&[1, 14]);
    assert_eq!(call("sysctl", &[0, 2, 0, 0, 0, 0]), FAILED, "null name");
    assert_eq!(
        call("sysctl", &[mib.at(), 0, 0, 0, 0, 0]),
        FAILED,
        "no components"
    );
    assert_eq!(
        call("sysctl", &[mib.at(), 25, 0, 0, 0, 0]),
        FAILED,
        "past CTL_MAXNAME"
    );
    assert_eq!(call("sysctl", &[mib.at(), u64::MAX, 0, 0, 0, 0]), FAILED);
}

/// The harvested ABI constants are read from the table, and a name nothing harvested is
/// absent rather than defaulted (D352).
#[test]
fn an_abi_constant_is_looked_up_and_a_missing_one_is_absent() {
    assert!(
        orbistoun_hle::constants::abi_constant("errno", "ENOENT").is_some(),
        "sysctl's own answer is read from this table"
    );
    assert_eq!(
        orbistoun_hle::constants::abi_constant("errno", "ENOTAREALERRNO"),
        None
    );
    assert_eq!(
        orbistoun_hle::constants::abi_constant("not_a_section", "ENOENT"),
        None
    );
}

// The C++ operators.

/// `operator new` and `operator delete` are the same heap as `malloc` and `free`, so blocks
/// may cross between them.
#[test]
fn the_cxx_operators_share_the_heap_with_malloc() {
    let block = call("_Znwm", &[128]);
    assert_ne!(block, 0);
    call("memset", &[block, 0x7E, 128]);

    // Freed through the C name.
    call("free", &[block]);

    // And the other way round: allocated by `malloc`, released by the sized delete, whose size
    // argument is ignored in favour of the header.
    let other = call("malloc", &[64]);
    assert_ne!(other, 0);
    assert_eq!(
        call("_ZdlPvm", &[other, 999_999]),
        0,
        "the wrong size is ignored"
    );
}

// Files.

/// A handle naming nothing is refused by every call that takes one, with `EOF` rather than a
/// value that looks like a size.
#[test]
fn a_handle_naming_nothing_is_refused_by_everything() {
    let bogus = 0x7FFF_0001;
    let dest = Buf::zeroed(64);

    assert_eq!(call("fclose", &[bogus]), EOF);
    assert_eq!(call("ftell", &[bogus]), EOF);
    assert_eq!(call("fseek", &[bogus, 0, 0]), EOF);
    assert_eq!(call("fread", &[dest.at(), 1, 64, bogus]), 0);
    assert_eq!(dest.bytes(), &[0; 64], "and read nothing into the buffer");

    // An unrecognised `whence` is refused before the handle is even consulted.
    assert_eq!(call("fseek", &[bogus, 0, 99]), EOF);
}

/// A read of nothing reads nothing, and a size that cannot be expressed is refused rather
/// than truncated.
#[test]
fn a_read_that_cannot_be_expressed_is_refused() {
    let dest = Buf::zeroed(16);
    assert_eq!(call("fread", &[0, 1, 16, 1]), 0, "nowhere to put it");
    assert_eq!(
        call("fread", &[dest.at(), 0, 16, 1]),
        0,
        "elements of no size"
    );
    assert_eq!(call("fread", &[dest.at(), 16, 0, 1]), 0, "no elements");
    assert_eq!(
        call("fread", &[dest.at(), u64::MAX, 2, 1]),
        0,
        "a product that does not fit"
    );
}

/// A path under no mount opens nothing, and answers null rather than a code, with `errno`
/// `ENOENT`: FreeBSD's `open(2)` for a missing path, and what Mesa printed for
/// `/usr/share/libdrm/amdgpu.ids` in Craft on a console (2026-10-06).
#[test]
fn a_path_under_no_mount_opens_nothing() {
    let path = Buf::text("/nowhere0/definitely-not-here.bin");
    orbistoun_core::errno::set(22);
    assert_eq!(call("fopen", &[path.at(), 0]), 0);
    assert_eq!(orbistoun_core::errno::get(), 2, "ENOENT, not a stale errno");
    assert_eq!(call("fopen", &[0, 0]), 0, "a null path opens nothing");
}

/// The whole file cycle over a real file, in one test.
///
/// The mount table is process-wide, so this uses a prefix nothing else uses and never clears
/// the table.
#[test]
fn a_mounted_file_can_be_opened_read_and_positioned() {
    let dir = std::env::temp_dir().join("orbistoun-libc-stdio-test");
    std::fs::create_dir_all(&dir).expect("a temporary directory");
    std::fs::write(dir.join("sample.bin"), b"0123456789").expect("a sample file");
    orbistoun_fs::mount::mount("/stdiotest", dir);

    let path = Buf::text("/stdiotest/sample.bin");
    let stream = call("fopen", &[path.at(), 0]);
    assert_ne!(stream, 0, "a mounted, existing file opens");

    // Reading answers whole elements, not bytes.
    let dest = Buf::zeroed(16);
    assert_eq!(call("fread", &[dest.at(), 2, 3, stream]), 3);
    assert_eq!(&dest.bytes()[..6], b"012345");
    assert_eq!(call("ftell", &[stream]), 6);

    // A partial final element is not reported as a whole one.
    assert_eq!(
        call("fread", &[dest.at(), 3, 4, stream]),
        1,
        "four left, three per element"
    );

    call("rewind", &[stream]);
    assert_eq!(call("ftell", &[stream]), 0);

    assert_eq!(call("fseek", &[stream, 4, 0]), 0, "seek from the start");
    assert_eq!(call("ftell", &[stream]), 4);
    assert_eq!(
        call("fseek", &[stream, 2, 1]),
        0,
        "seek from the current position"
    );
    assert_eq!(call("ftell", &[stream]), 6);
    let back_two = (-2_i64) as u64;
    assert_eq!(
        call("fseek", &[stream, back_two, 2]),
        0,
        "seek from the end"
    );
    assert_eq!(call("ftell", &[stream]), 8);

    // The `off_t` spellings: the same calls, since `off_t` and `long` are both 64 bits here.
    assert_eq!(call("fseeko", &[stream, 3, 0]), 0, "seeko from the start");
    assert_eq!(call("ftello", &[stream]), 3);
    assert_eq!(call("fseeko", &[stream, 0, 99]), EOF, "an unknown whence");
    assert_eq!(call("ftello", &[stream]), 3, "leaves the position");

    assert_eq!(call("ferror", &[stream]), 0, "nothing went wrong");
    assert_eq!(call("fflush", &[stream]), 0);

    assert_eq!(call("fclose", &[stream]), 0, "and it closes once");
    assert_eq!(call("fclose", &[stream]), EOF, "but not twice");
}

/// `fgetc` reads one byte as an unsigned char and answers `EOF` at the end; `fputc` writes one and
/// answers it (ISO/IEC 9899 7.21.7.1, 7.21.7.3).
#[test]
fn a_byte_at_a_time_reads_and_writes_as_c_specifies() {
    let dir = std::env::temp_dir().join("orbistoun-libc-stdio-bytes");
    std::fs::create_dir_all(&dir).expect("a temporary directory");
    std::fs::write(dir.join("two.bin"), [0x41, 0xff]).expect("a sample file");
    orbistoun_fs::mount::mount("/bytetest", dir.clone());

    let path = Buf::text("/bytetest/two.bin");
    let stream = call("fopen", &[path.at(), 0]);
    assert_ne!(stream, 0, "opens");
    assert_eq!(call("fgetc", &[stream]), 0x41);
    assert_eq!(call("fgetc", &[stream]), 0xff, "an unsigned char, not -1");
    assert_eq!(call("fgetc", &[stream]), u64::MAX, "EOF at the end");
    assert_eq!(call("fclose", &[stream]), 0);
    assert_eq!(call("fgetc", &[0]), u64::MAX, "no stream is EOF");
    assert_eq!(
        call("fputc", &[0x141, 0]),
        0x41,
        "the byte written, as an unsigned char"
    );
}

/// A System V `va_list` whose register half holds `words` from its start: `gp_offset` 0,
/// `fp_offset` past the integer half, the overflow area empty (psABI 3.5.7).
/// A `va_list` and the two areas it points into, kept alive together.
type VaListParts = (Box<[u64; 6]>, Box<[u64; 2]>, Box<[u64; 3]>);

fn va_list(words: &[u64]) -> VaListParts {
    let mut save = Box::new([0_u64; 6]);
    save[..words.len()].copy_from_slice(words);
    let overflow = Box::new([0_u64; 2]);
    let list = Box::new([48_u64 << 32, overflow.as_ptr() as u64, save.as_ptr() as u64]);
    (save, overflow, list)
}

/// `vsprintf` renders through a `va_list` with no bound (ISO/IEC 9899 7.21.6.13), and
/// `vsnprintf_s` with one, terminated (C11 K.3.5.3.12).
#[test]
fn the_va_list_printers_render_their_arguments() {
    let format = Buf::text("%d-%s");
    let word = Buf::text("ok");
    let (_save, _overflow, list) = va_list(&[42, word.at()]);
    let dest = Buf::zeroed(32);
    assert_eq!(
        call("vsprintf", &[dest.at(), format.at(), list.as_ptr() as u64]),
        5
    );
    assert_eq!(read_string(dest.at()), "42-ok");

    let (_save, _overflow, list) = va_list(&[42, word.at()]);
    let small = Buf::zeroed(4);
    assert_eq!(
        call(
            "vsnprintf_s",
            &[small.at(), 4, format.at(), list.as_ptr() as u64]
        ),
        5,
        "the full length"
    );
    assert_eq!(read_string(small.at()), "42-", "truncated and terminated");
}

/// `fopen_s(&stream, path, mode)` (C11 K.3.5.2.1) writes the stream and answers 0, or writes null
/// and answers non-zero; `printf_s` prints as `printf` does and answers the length (K.3.5.3.3).
#[test]
fn the_bounds_checked_stdio_calls_answer_as_annex_k_says() {
    let dir = std::env::temp_dir().join("orbistoun-libc-stdio-annexk");
    std::fs::create_dir_all(&dir).expect("a temporary directory");
    std::fs::write(dir.join("one.bin"), [7]).expect("a sample file");
    orbistoun_fs::mount::mount("/annexk", dir);

    let mut stream = 0xa5a5_u64;
    let out = std::ptr::from_mut(&mut stream) as u64;
    let path = Buf::text("/annexk/one.bin");
    let mode = Buf::text("r");
    assert_eq!(call("fopen_s", &[out, path.at(), mode.at()]), 0);
    assert_ne!(stream, 0, "a stream");
    assert_eq!(call("fgetc", &[stream]), 7);
    let missing = Buf::text("/annexk/none.bin");
    assert_ne!(call("fopen_s", &[out, missing.at(), mode.at()]), 0);
    assert_eq!(stream, 0, "null written on failure");

    let format = Buf::text(
        "%d
",
    );
    assert_eq!(call("printf_s", &[format.at(), 12]), 3);
}

/// `localeconv()` answers the "C" locale's `struct lconv` (ISO/IEC 9899 7.11.2.1): `decimal_point`
/// ".", every other string empty, every `char` member `CHAR_MAX`, in FreeBSD's `<locale.h>` layout
/// - ten string pointers, then fourteen `char`s.
#[test]
fn localeconv_answers_the_c_locale() {
    let at = call("localeconv", &[]);
    assert_ne!(at, 0);
    let pointer = |index: u64| {
        // SAFETY: the structure `localeconv` answered, eight bytes a pointer.
        unsafe { std::ptr::read_unaligned((at + index * 8) as usize as *const u64) }
    };
    assert_eq!(read_string(pointer(0)), ".", "decimal_point");
    for index in 1..10 {
        assert_eq!(read_string(pointer(index)), "", "string member {index}");
    }
    for index in 0..14_u64 {
        // SAFETY: the `char` members after the ten pointers.
        let value = unsafe { std::ptr::read((at + 80 + index) as usize as *const u8) };
        assert_eq!(value, 127, "char member {index} is CHAR_MAX");
    }
}
