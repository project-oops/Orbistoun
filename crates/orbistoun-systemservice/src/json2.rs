//! `libSceJson2` - the platform's JSON parser, reached by mangled C++ names.
//!
//! The library's start-up is implemented as obSCEne measured it, link-imported on hardware
//! (REQ-js02, `reports/hardware/20261007-082134-eboot.obs.log`, `130-layout/json2-init`):
//! `Initializer` keeps its state in byte 0, and `initialize` takes the memory it needs from the
//! guest's own `MemAllocator`, whose vtable it calls. The blocks are the library's internal state,
//! which nothing here reads; they are held only to hand back at `terminate`. Parsing is not
//! implemented.
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceJson2" {
        "_ZN3sce4Json11Initializer10initializeEPKNS0_13InitParameterE" => 6,
        // (this, param)
        "_ZN3sce4Json11Initializer10initializeEPKNS0_14InitParameter2E" => 2,
        // (this)
        "_ZN3sce4Json14InitParameter2C1Ev" => 1,
        // (this, size)
        "_ZN3sce4Json14InitParameter217setFileBufferSizeEm" => 2,
        // (this, allocator, pass-through word)
        "_ZN3sce4Json14InitParameter212setAllocatorEPNS0_12MemAllocatorEPv" => 3,
        "_ZN3sce4Json11InitializerC1Ev" => 6,
        "_ZN3sce4Json11Initializer9terminateEv" => 6,
        "_ZN3sce4Json12MemAllocatorC2Ev" => 6,
        // (this)
        "_ZN3sce4Json5ValueC1Ev" => 1,
        // (this)
        "_ZN3sce4Json5ValueD1Ev" => 1,
        // (this, type)
        "_ZN3sce4Json5Value3setENS0_9ValueTypeE" => 2,
        // (this, value)
        "_ZN3sce4Json5Value3setEb" => 2,
        // (this, value)
        "_ZN3sce4Json5Value3setEl" => 2,
        // (this, value)
        "_ZN3sce4Json5Value3setEm" => 2,
    }
}

const OK: u64 = 0;

/// `initialize` on an initializer already initialized (`arm3-reinit`).
const ALREADY_INITIALIZED: u64 = 0x8084_8111;

/// The allocations `initialize` makes, in order (`arm2-init`, `alloc-size-0`..`-11`).
const INITIALIZE_SIZES: [u64; 12] = [
    0x18, 0x8, 0x18, 0x8, 0x20, 0x30, 0x8, 0x20, 0x68, 0x20, 0x8, 0x8,
];

/// `MemAllocator`'s vtable slots `initialize` and `terminate` call, by byte offset: the probe's
/// subclass put `allocate` in slot 2 and `deallocate` in slot 3, after the two destructors, and
/// the library called them there.
const ALLOCATE_SLOT: u64 = 2 * 8;
const DEALLOCATE_SLOT: u64 = 3 * 8;

/// The parameter block's word the library passes to every allocation unchanged (`param + 8`, seen
/// as each call's third argument).
const PASS_THROUGH_AT: u64 = 8;

/// What an initialized `Initializer` holds: its allocator, the word passed to it, and the blocks it
/// was given.
struct Session {
    allocator: u64,
    pass: u64,
    blocks: Vec<u64>,
}

fn sessions() -> &'static Mutex<BTreeMap<u64, Session>> {
    static SESSIONS: OnceLock<Mutex<BTreeMap<u64, Session>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// `Initializer::Initializer()`: clears byte 0, the initialized flag, and nothing else
/// (`arm1-ctor`, `ctor_written 00`, extent 1).
pub(crate) fn initializer_construct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's object, whose first byte the constructor owns.
    unsafe { guest::write_bytes(args[0], &[0]) };
    OK
}

/// The vtable slot `slot` bytes into `allocator`'s vtable.
fn virtual_at(allocator: u64, slot: u64) -> Option<u64> {
    // SAFETY: the guest's allocator object and its vtable, both read through the checked accessor.
    let vtable = unsafe { guest::read_u64(allocator) }?;
    // SAFETY: as above.
    unsafe { guest::read_u64(vtable + slot) }.filter(|&entry| entry != 0)
}

/// The bytes an `InitParameter2` holds once constructed: `0x28`, zero but for the word `2` at
/// `+0x18` (`arm2-init`, `param-after`, which the constructor and `setAllocator` left; the rest of
/// the probe's `0xcc` fill past `0x28` untouched).
const INIT_PARAMETER2_BYTES: usize = 0x28;
const INIT_PARAMETER2_WORD_AT: usize = 0x18;
const INIT_PARAMETER2_WORD: u32 = 2;

/// `InitParameter2::InitParameter2()`: writes the constructed block (`arm2-init`).
pub(crate) fn init_parameter2_construct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mut block = [0_u8; INIT_PARAMETER2_BYTES];
    block[INIT_PARAMETER2_WORD_AT..INIT_PARAMETER2_WORD_AT + 4]
        .copy_from_slice(&INIT_PARAMETER2_WORD.to_le_bytes());
    // SAFETY: the guest's object, by the constructor's contract.
    if unsafe { guest::write_bytes(args[0], &block) } {
        OK
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// `InitParameter2::setAllocator(MemAllocator*, void*)`: the allocator at `+0` and the word every
/// allocation is passed at `+8` (`arm2-init`, `param-after`: the probe's allocator and
/// `0x12345678` there).
pub(crate) fn init_parameter2_set_allocator(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (this, allocator, pass) = (args[0], args[1], args[2]);
    // SAFETY: the guest's object, by the call's contract; written through checked accessors.
    let wrote_allocator = unsafe { guest::write_u64(this, allocator) };
    // SAFETY: as above.
    let wrote_pass = unsafe { guest::write_u64(this + PASS_THROUGH_AT, pass) };
    if wrote_allocator && wrote_pass {
        OK
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// Where `InitParameter2::setFileBufferSize` writes its size (REQ-np03).
const FILE_BUFFER_SIZE_AT: u64 = 0x10;

/// `InitParameter2::setFileBufferSize(size_t)`: writes `size` as a quadword at `+0x10` and nothing
/// else, on a constructed parameter as on a raw block (REQ-np03, `json-set-file-buffer-size`,
/// `report-1791758178.txt` 223-266: `0x1000` there, the constructor's `2` at `+0x18` kept). A
/// setter whose answer the probe did not read; 0 here.
pub(crate) fn init_parameter2_set_file_buffer_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's object, by the call's contract; written through the checked accessor.
    if unsafe { guest::write_u64(args[0] + FILE_BUFFER_SIZE_AT, args[1]) } {
        OK
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// `Initializer::initialize(const InitParameter*)`: the twelve measured allocations through the
/// parameter block's allocator, then byte 0 set and `0`. A second answers `0x80848111` and
/// allocates nothing. An allocation that fails hands back what it got and answers the placeholder:
/// what the library answers then is unmeasured.
pub(crate) fn initializer_initialize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (this, param) = (args[0], args[1]);
    let Ok(mut sessions) = sessions().lock() else {
        return u64::from(GuestError::HostFailed.as_raw());
    };
    if sessions.contains_key(&this) {
        return ALREADY_INITIALIZED;
    }
    // SAFETY: the guest's parameter block, read through the checked accessor.
    let read = |at: u64| unsafe { guest::read_u64(at) };
    let (Some(allocator), Some(pass)) = (read(param), read(param + PASS_THROUGH_AT)) else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let (Some(allocate), Some(deallocate)) = (
        virtual_at(allocator, ALLOCATE_SLOT),
        virtual_at(allocator, DEALLOCATE_SLOT),
    ) else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let mut blocks = Vec::with_capacity(INITIALIZE_SIZES.len());
    for size in INITIALIZE_SIZES {
        // SAFETY: `allocate` is the guest's own function, from the vtable of the allocator it
        // handed over.
        match unsafe { orbistoun_kernel::thread::call_guest(allocate, [allocator, size, pass]) } {
            Some(block) if block != 0 => blocks.push(block),
            _ => {
                hand_back(deallocate, allocator, pass, &blocks);
                return u64::from(GuestError::Unimplemented.as_raw());
            }
        }
    }
    // SAFETY: the guest's object, whose first byte is the flag.
    unsafe { guest::write_bytes(this, &[1]) };
    sessions.insert(
        this,
        Session {
            allocator,
            pass,
            blocks,
        },
    );
    OK
}

/// Hands each block back through the allocator's `deallocate`.
fn hand_back(deallocate: u64, allocator: u64, pass: u64, blocks: &[u64]) {
    for &block in blocks.iter().rev() {
        // SAFETY: the guest's own function, as `allocate` was; `block` is one it handed out.
        unsafe { orbistoun_kernel::thread::call_guest(deallocate, [allocator, block, pass]) };
    }
}

/// `Initializer::terminate()`: every block back through the allocator, byte 0 cleared, `0`
/// (`arm4-term`, `dealloc-calls 0xc`). One not initialized answers the placeholder: unmeasured.
pub(crate) fn initializer_terminate(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let this = args[0];
    let session = sessions().lock().ok().and_then(|mut s| s.remove(&this));
    let Some(session) = session else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    if let Some(deallocate) = virtual_at(session.allocator, DEALLOCATE_SLOT) {
        hand_back(deallocate, session.allocator, session.pass, &session.blocks);
    }
    // SAFETY: the guest's object, whose first byte is the flag.
    unsafe { guest::write_bytes(this, &[0]) };
    OK
}

/// The base `MemAllocator`'s vtable: zeroed guest memory, its four slots - two destructors,
/// `allocate`, `deallocate` - empty as a pure virtual's are, since the guest's subclass replaces
/// the pointer as soon as the base constructor returns. Mapped once.
fn base_vtable() -> Option<u64> {
    static AT: OnceLock<Option<u64>> = OnceLock::new();
    *AT.get_or_init(|| {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[1] = 0x4000;
        args[2] = 0x1; // PROT_READ
        args[3] = 0x1000 | 0x2; // MAP_ANON | MAP_PRIVATE
        args[4] = u64::MAX;
        let at = orbistoun_kernel::mmap(&args);
        (at != u64::MAX && at != 0).then_some(at)
    })
}

/// `MemAllocator::MemAllocator()`: writes the base vtable pointer and nothing more
/// (`MemAllocator`, `base-obj-after`: eight bytes written, the rest untouched).
pub(crate) fn mem_allocator_construct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(vtable) = base_vtable() else {
        return u64::from(GuestError::NoMemory.as_raw());
    };
    // SAFETY: the guest's object, whose first word is its vtable pointer.
    unsafe { guest::write_u64(args[0], vtable) };
    OK
}

/// Where a `Value` keeps its payload: eight bytes, a scalar or a pointer to what it owns
/// (`130-layout/json2-value-string`, every `set` writing from +16).
const VALUE_PAYLOAD_AT: u64 = 16;

/// Where a `Value` keeps its type, a 32-bit word (the same rows: `set(ValueType t)` writes `t`
/// here, +28, and leaves +24 to +27 as they were).
const VALUE_TYPE_AT: u64 = 28;

/// The first `ValueType` whose payload points at memory the library allocated: string 5, array 6
/// and object 7 (`set-type-5` to `-7` write a heap address at +16). How large those blocks are,
/// and so what setting or destroying one allocates and frees, is not measured, and those paths
/// answer the placeholder.
const FIRST_OWNING_TYPE: u32 = 5;

/// `Value::Value()`: a null value - sixteen zero bytes at +0 and type 0 at +28, the payload left
/// as it was (`Value::Value()`, `val-0x80`, REQ-20261013T1735Z-js03). Returns nothing.
pub(crate) fn value_construct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let this = args[0];
    // SAFETY: the guest's object, the thirty-two bytes a Value occupies.
    let head = unsafe { guest::write_bytes(this, &[0; 16]) };
    // SAFETY: as above, its type word.
    let typed = unsafe { guest::write_u32(this + VALUE_TYPE_AT, 0) };
    if head && typed {
        OK
    } else {
        u64::from(GuestError::InvalidArgument.as_raw())
    }
}

/// Sets a `Value`'s payload and type. `payload` is the bytes written from +16: none for null, one
/// for a boolean, eight for a number.
fn value_set(this: u64, value_type: u32, payload: &[u8]) -> u64 {
    // SAFETY: the guest's Value, its payload bytes.
    let set = unsafe { guest::write_bytes(this + VALUE_PAYLOAD_AT, payload) };
    // SAFETY: as above, its type word.
    let typed = unsafe { guest::write_u32(this + VALUE_TYPE_AT, value_type) };
    if set && typed {
        OK
    } else {
        u64::from(GuestError::InvalidArgument.as_raw())
    }
}

/// `Value::set(ValueType)`: the type at +28, and the payload that type starts from - nothing for
/// null, one zero byte for a boolean, eight zero bytes for an integer, unsigned or real
/// (`set-type-0` to `-4`). A string, array or object allocates its payload, which is not modelled.
pub(crate) fn value_set_type(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Ok(value_type) = u32::try_from(args[1]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    match value_type {
        0 => value_set(args[0], 0, &[]),
        1 => value_set(args[0], 1, &[0]),
        2..FIRST_OWNING_TYPE => value_set(args[0], value_type, &[0; 8]),
        _ => u64::from(GuestError::Unimplemented.as_raw()),
    }
}

/// `Value::set(bool)`: the boolean as one byte at +16, type 1 (`set-bool-1`).
pub(crate) fn value_set_bool(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    value_set(args[0], 1, &[u8::from(args[1] & 0xff != 0)])
}

/// `Value::set(long)`: the integer's eight bytes at +16, type 2 (`set-long-neg2`).
pub(crate) fn value_set_integer(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    value_set(args[0], 2, &args[1].to_le_bytes())
}

/// `Value::set(unsigned long)`: its eight bytes at +16, type 3 (`set-ulong`).
pub(crate) fn value_set_unsigned(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    value_set(args[0], 3, &args[1].to_le_bytes())
}

/// `Value::~Value()` on a value that owns nothing: its payload zeroed and its type null, as the
/// destructor left a string-valued one (`Value::~Value()`, `val-after-dtor-0x80`). A value that
/// owns memory - a string, array or object - is freed through the allocator by the library (three
/// deallocations for a string), which is not modelled, so it answers the placeholder and is left
/// as it was.
pub(crate) fn value_destruct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let this = args[0];
    // SAFETY: the guest's Value, its type word.
    let Some(value_type) = (unsafe { guest::read_u32(this + VALUE_TYPE_AT) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    if value_type >= FIRST_OWNING_TYPE {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    value_set(this, 0, &[0; 8])
}

#[cfg(test)]
mod value_tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    const CC: u8 = 0xcc;

    /// A 0x80-byte object filled `0xcc`, as the probe passed one.
    fn object() -> Vec<u8> {
        vec![CC; 0x80]
    }

    fn call(f: fn(&[u64; GUEST_ARG_REGISTERS]) -> u64, object: &mut [u8], arg: u64) -> u64 {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = object.as_mut_ptr() as u64;
        args[1] = arg;
        f(&args)
    }

    /// The bytes from +16 to +32, as the rows print them.
    fn tail(object: &[u8]) -> [u8; 16] {
        object[16..32].try_into().expect("sixteen bytes")
    }

    /// A tail of `0xcc` with these leading payload bytes and this type.
    fn expect(payload: &[u8], value_type: u8) -> [u8; 16] {
        let mut row = [CC; 16];
        row[..payload.len()].copy_from_slice(payload);
        row[12..].copy_from_slice(&[value_type, 0, 0, 0]);
        row
    }

    /// `Value()` writes sixteen zero bytes and type 0, and nothing past +32
    /// (`report-20261014-004800.log`, `Value::Value()`).
    #[test]
    fn a_constructed_value_is_null() {
        let mut value = object();
        assert_eq!(call(super::value_construct, &mut value, 0), 0);
        assert_eq!(value[..16], [0; 16]);
        assert_eq!(tail(&value), expect(&[], 0));
        assert!(value[32..].iter().all(|&b| b == CC));
    }

    /// Each `set` writes its payload from +16 and its type at +28, as the rows show them on a
    /// constructed value.
    #[test]
    fn each_set_writes_its_payload_and_type() {
        type Setter = fn(&[u64; GUEST_ARG_REGISTERS]) -> u64;
        let cases: [(Setter, u64, [u8; 16]); 6] = [
            (super::value_set_type, 0, expect(&[], 0)),
            (super::value_set_type, 1, expect(&[0], 1)),
            (super::value_set_type, 4, expect(&[0; 8], 4)),
            (super::value_set_bool, 1, expect(&[1], 1)),
            (
                super::value_set_integer,
                (-2_i64).cast_unsigned(),
                expect(&(-2_i64).to_le_bytes(), 2),
            ),
            (
                super::value_set_unsigned,
                0x1122_3344_5566_7788,
                expect(&0x1122_3344_5566_7788_u64.to_le_bytes(), 3),
            ),
        ];
        for (index, (set, arg, expected)) in cases.into_iter().enumerate() {
            let mut value = object();
            call(super::value_construct, &mut value, 0);
            assert_eq!(call(set, &mut value, arg), 0, "case {index}");
            assert_eq!(tail(&value), expected, "case {index}");
        }
        let mut owning = object();
        call(super::value_construct, &mut owning, 0);
        assert_ne!(
            call(super::value_set_type, &mut owning, 5),
            0,
            "a string allocates, which is not modelled"
        );
    }

    /// Destroying a value that owns nothing zeroes its payload and leaves it null; one that owns
    /// memory is left as it was.
    #[test]
    fn destroying_a_value_leaves_it_null() {
        let mut value = object();
        call(super::value_construct, &mut value, 0);
        call(super::value_set_integer, &mut value, 7);
        assert_eq!(call(super::value_destruct, &mut value, 0), 0);
        assert_eq!(tail(&value), expect(&[0; 8], 0));
        let mut owning = object();
        owning[28..32].copy_from_slice(&5_u32.to_le_bytes());
        let before = owning.clone();
        assert_ne!(call(super::value_destruct, &mut owning, 0), 0);
        assert_eq!(owning, before);
    }
}
