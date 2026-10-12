//! `sce::Json::Initializer` and `sce::Json::MemAllocator` as obSCEne measured them, link-imported on
//! hardware (REQ-js02, `reports/hardware/20261007-082134-eboot.obs.log`, `130-layout/json2-init`):
//! the constructor clears byte 0; `initialize` takes its allocator from the parameter block's first
//! word, makes twelve allocations through the allocator's vtable slot 2 - `(this, size, param + 8)` -
//! sets byte 0 to 1 and answers 0, and a second answers `0x80848111`; `terminate` hands every block
//! back through slot 3, clears byte 0 and answers 0. Its own test binary, because the allocator's
//! record is process state.

use orbistoun_core::GUEST_ARG_REGISTERS;
use std::sync::Mutex;

/// Each allocation's size and pass-through word, and each block handed back.
type Seen = (Vec<(u64, u64)>, Vec<u64>);
/// What the allocator saw.
static SEEN: Mutex<Seen> = Mutex::new((Vec::new(), Vec::new()));
/// Where blocks come from, so each address is real memory.
static mut HEAP: [u64; 1024] = [0; 1024];

extern "sysv64" fn allocate(_this: u64, size: u64, pass: u64) -> u64 {
    let mut seen = SEEN.lock().expect("record");
    let used: u64 = seen
        .0
        .iter()
        .map(|(size, _)| size.next_multiple_of(16))
        .sum();
    seen.0.push((size, pass));
    // The heap is only addressed, never borrowed, and each block is inside it.
    let base = std::ptr::addr_of_mut!(HEAP) as usize as u64;
    base + used
}

extern "sysv64" fn deallocate(_this: u64, block: u64, _pass: u64) -> u64 {
    SEEN.lock().expect("record").1.push(block);
    0
}

fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (_, handler) = orbistoun_systemservice::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is implemented"));
    handler(&args)
}

fn at(pointer: u64) -> [u64; GUEST_ARG_REGISTERS] {
    let mut args = [0; GUEST_ARG_REGISTERS];
    args[0] = pointer;
    args
}

const CONSTRUCT: &str = "_ZN3sce4Json11InitializerC1Ev";
const INITIALIZE: &str = "_ZN3sce4Json11Initializer10initializeEPKNS0_13InitParameterE";
const TERMINATE: &str = "_ZN3sce4Json11Initializer9terminateEv";
const ALLOCATOR: &str = "_ZN3sce4Json12MemAllocatorC2Ev";
const PARAM2: &str = "_ZN3sce4Json14InitParameter2C1Ev";
const SET_ALLOCATOR: &str = "_ZN3sce4Json14InitParameter212setAllocatorEPNS0_12MemAllocatorEPv";
const INITIALIZE2: &str = "_ZN3sce4Json11Initializer10initializeEPKNS0_14InitParameter2E";

/// The twelve sizes `initialize` asked for, in order (`arm2-init`, `alloc-size-0`..`-11`).
const MEASURED_SIZES: [u64; 12] = [
    0x18, 0x8, 0x18, 0x8, 0x20, 0x30, 0x8, 0x20, 0x68, 0x20, 0x8, 0x8,
];

#[test]
fn the_initializer_runs_its_measured_lifecycle_through_the_guests_allocator() {
    // The base allocator constructor writes its vtable pointer and nothing more (`base-obj-after`).
    let mut base = [0xcc_u8; 0x40];
    call(ALLOCATOR, at(base.as_mut_ptr() as usize as u64));
    assert_ne!(base[..8], [0xcc; 8], "a vtable pointer");
    assert_ne!(base[..8], [0; 8], "a real one");
    assert!(base[8..].iter().all(|&b| b == 0xcc), "nothing past it");

    // The guest's subclass: its own vtable over the base's, allocate and deallocate in slots 2, 3.
    let vtable: [u64; 4] = [
        0,
        0,
        allocate as extern "sysv64" fn(u64, u64, u64) -> u64 as usize as u64,
        deallocate as extern "sysv64" fn(u64, u64, u64) -> u64 as usize as u64,
    ];
    let allocator: [u64; 2] = [vtable.as_ptr() as usize as u64, 0];
    // The block as `arm2-init` built it: the constructor, then `setAllocator(allocator,
    // 0x12345678)`, leaving `param-after` - allocator, word, zero, `2` at +0x18 - in 0x28 bytes.
    let mut param = [0xcc_u8; 0x40];
    let block = param.as_mut_ptr() as usize as u64;
    assert_eq!(call(PARAM2, at(block)), 0);
    let mut set = at(block);
    set[1] = allocator.as_ptr() as usize as u64;
    set[2] = 0x1234_5678;
    assert_eq!(call(SET_ALLOCATOR, set), 0);
    let mut expected = [0_u8; 0x28];
    expected[..8].copy_from_slice(&set[1].to_le_bytes());
    expected[8..16].copy_from_slice(&0x1234_5678_u64.to_le_bytes());
    expected[0x18] = 2;
    assert_eq!(param[..0x28], expected, "`param-after`");
    // setFileBufferSize writes its size at +0x10 and nothing else (REQ-np03).
    let mut resized = param;
    let mut args = at(resized.as_mut_ptr() as usize as u64);
    args[1] = 0x1000;
    assert_eq!(
        call("_ZN3sce4Json14InitParameter217setFileBufferSizeEm", args),
        0
    );
    assert_eq!(resized[0x10..0x18], 0x1000_u64.to_le_bytes());
    assert_eq!(resized[..0x10], param[..0x10], "nothing before it");
    assert_eq!(resized[0x18..], param[0x18..], "nor after");
    assert!(
        param[0x28..].iter().all(|&b| b == 0xcc),
        "nothing past 0x28"
    );

    let mut object = [0xcc_u8; 0x40];
    let this = object.as_mut_ptr() as usize as u64;
    call(CONSTRUCT, at(this));
    assert_eq!(
        object[0], 0,
        "the constructor clears byte 0 (`ctor_written 00`)"
    );
    assert!(object[1..].iter().all(|&b| b == 0xcc), "and only byte 0");

    let mut args = at(this);
    args[1] = param.as_ptr() as usize as u64;
    assert_eq!(call(INITIALIZE, args), 0, "`arm2-init` rc 0x0");
    assert_eq!(
        call(INITIALIZE2, args),
        0x8084_8111,
        "the InitParameter2 form is the same initialize (D793)"
    );
    assert_eq!(object[0], 1, "`obj-written 01`");
    assert!(object[1..].iter().all(|&b| b == 0xcc));
    {
        let seen = SEEN.lock().expect("record");
        let sizes: Vec<u64> = seen.0.iter().map(|&(size, _)| size).collect();
        assert_eq!(sizes, MEASURED_SIZES, "twelve allocations, as measured");
        assert!(
            seen.0.iter().all(|&(_, pass)| pass == 0x1234_5678),
            "param + 8 passed through"
        );
        assert!(seen.1.is_empty(), "nothing handed back yet");
    }

    assert_eq!(call(INITIALIZE, args), 0x8084_8111, "`arm3-reinit`");
    assert_eq!(
        SEEN.lock().expect("record").0.len(),
        12,
        "a refused second one allocates nothing"
    );

    assert_eq!(call(TERMINATE, at(this)), 0, "`arm4-term` rc 0x0");
    assert_eq!(object[0], 0, "byte 0 cleared again");
    let seen = SEEN.lock().expect("record");
    assert_eq!(
        seen.1.len(),
        12,
        "every block handed back (`dealloc-calls 0xc`)"
    );
    let mut given: Vec<u64> = seen.1.clone();
    given.sort_unstable();
    let base = given[0];
    assert!(given.windows(2).all(|pair| pair[0] != pair[1]), "each once");
    assert!(
        given.iter().all(|&block| block >= base),
        "the blocks it was given"
    );
}
