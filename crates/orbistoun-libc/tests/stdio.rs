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

    let text = read_string(message);
    assert!(!text.is_empty(), "and something readable in it");
    assert!(
        text.contains('2'),
        "the message should name the number it was asked about: {text:?}"
    );

    // There is no message table, and the text says so.
    assert!(text.contains("no message table"), "{text:?}");

    // A different code gives a different message through the same address.
    let again = call("strerror", &[7]);
    assert_eq!(again, message, "one buffer per thread, reused");
    assert!(read_string(again).contains('7'));

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

/// A path under no mount opens nothing, and answers null rather than a code.
#[test]
fn a_path_under_no_mount_opens_nothing() {
    let path = Buf::text("/nowhere0/definitely-not-here.bin");
    assert_eq!(call("fopen", &[path.at(), 0]), 0);
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

    assert_eq!(call("ferror", &[stream]), 0, "nothing went wrong");
    assert_eq!(call("fflush", &[stream]), 0);

    assert_eq!(call("fclose", &[stream]), 0, "and it closes once");
    assert_eq!(call("fclose", &[stream]), EOF, "but not twice");
}
