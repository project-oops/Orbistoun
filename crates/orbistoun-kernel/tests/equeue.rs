//! Creating an event queue, and registering events against one that exists.
//!
//! # What sent this here
//!
//! `sceKernelCreateEqueue` was unimplemented, so **its out-parameter was never written** and the
//! guest's handle kept whatever it held. Every later call against that queue was then handed a
//! value orbistoun never issued - which is the same shape as D507's out-parameter finding and
//! D509's, and is invisible from the call that causes it (D524).
//!
//! # What these assert, and what they cannot
//!
//! They assert the **handle round trip**: a created queue answers to the handle it wrote, and a
//! handle nobody was given does not. That is the property that makes a registration mean
//! something.
//!
//! They cannot assert that anything is ever *delivered*. Nothing delivers: PPSA02664 calls
//! `sceKernelWaitEqueue` zero times since the flip count stopped lying (D516), so a queue has no
//! reader and storage for one would be an abstraction ahead of its caller.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

fn implementation(name: &str) -> GuestFn {
    orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(
            || panic!("{name} is not implemented, so no guest can reach it"),
            |(_, f)| *f,
        )
}

/// A NUL-terminated guest string at a real address.
struct Text {
    /// Never read, and required: it owns the bytes `at` points into.
    _storage: Vec<u8>,
    at: u64,
}

impl Text {
    fn new(text: &str) -> Self {
        let mut storage = text.as_bytes().to_vec();
        storage.push(0);
        let at = storage.as_mut_ptr().expose_provenance() as u64;
        Self {
            _storage: storage,
            at,
        }
    }
}

/// A created queue answers to the handle it wrote; one nobody created is refused.
#[test]
fn a_queue_answers_to_the_handle_it_wrote_and_no_other() {
    let name = Text::new("a test queue");
    let mut handle = 0_u64;
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[0] = std::ptr::from_mut(&mut handle) as usize as u64;
    regs[1] = name.at;

    assert_eq!(
        implementation("sceKernelCreateEqueue")(&regs),
        0,
        "creating a queue succeeds"
    );
    assert_ne!(
        handle, 0,
        concat!(
            "and the handle is written through the out-parameter - leaving it unwritten is what ",
            "handed every later call a value orbistoun never issued"
        )
    );

    let mut add = [0_u64; GUEST_ARG_REGISTERS];
    add[0] = handle;
    add[1] = 0x1;
    assert_eq!(
        implementation("sceKernelAddUserEventEdge")(&add),
        0,
        "an event registers against the queue that handle names"
    );

    // A handle nobody was given. Distinct from the one above by construction, and large enough
    // that no counter reaches it.
    let mut stranger = [0_u64; GUEST_ARG_REGISTERS];
    stranger[0] = 0xDEAD_BEEF_0000_0001;
    stranger[1] = 0x1;
    let refused = implementation("sceKernelAddUserEventEdge")(&stranger);
    assert_ne!(
        refused, 0,
        concat!(
            "a queue nobody created must not accept a registration - reporting success would ",
            "promise delivery from a queue that does not exist"
        )
    );
    assert_eq!(
        refused,
        u64::from(orbistoun_core::GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw()),
        concat!(
            "and it refuses with the vendor code the event-flag family answers, which obSCEne ",
            "measured - not the placeholder a guest would fail to recognise (D125)"
        )
    );
    drop(name);
}

/// A null out-parameter is refused rather than written through.
#[test]
fn a_null_out_parameter_is_refused() {
    let name = Text::new("another queue");
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[0] = 0;
    regs[1] = name.at;
    assert_ne!(
        implementation("sceKernelCreateEqueue")(&regs),
        0,
        "there is nowhere to put the handle, so the call cannot have succeeded"
    );
    drop(name);
}
