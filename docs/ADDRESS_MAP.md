# The address map

Every fixed base orbistoun places something at, with its owner. Check it before choosing an
address for a new region (D513).

## The gate

`crates/orbistoun-service/tests/address_map.rs` checks this file against the source. It walks
`crates/*/src/**/*.rs`, collects every `const *_BASE: u64` declared at the top level of a file,
and fails when:

- a declared base is missing from the table below
- a base in the table is not declared in the source, or has a different value
- two declared bases lie within four gibibytes of each other

The test reads the first two cells of each table row: the value and the constant name, each
in backticks, exactly as the source declares them.

The gate checks where a region starts, not how long it is. A region that grows past the gap
to the next base collides without failing it.

## The map

| Base | Constant | Owner | What lives there |
|---|---|---|---|
| `0x0000_0000_6000_0000` | `TRIAL_REGION_BASE` | `orbistoun-turn` | A trial region, for a change the loop tries without a person |
| `0x0000_0008_0000_0000` | `CONSOLE_SYSCALL_GADGET_BASE` | `orbistoun-firmware` | The hardware's own libkernel base, where a payload runtime's fallback syscall gadget sits at `+0x4ea`; not chosen by this project |
| `0x0000_00F0_0000_0000` | `FIRMWARE_BASE` | `orbistoun-firmware` | The firmware skeleton, placed so an address in it is recognisable as firmware |
| `0x0000_4000_0000_0000` | `DEFAULT_MODULE_BASE` | `orbistoun-worker` | A module that links at zero |
| `0x0000_4800_0000_0000` | `TITLE_MODULE_BASE` | `orbistoun-worker` | The modules a title ships with itself |
| `0x0000_5E27_0000_0000` | `SENTINEL_BASE` | `orbistoun-abi` | A sentinel block's markers |
| `0x0000_5E28_0000_0000` | `CONTENT_BASE` | `orbistoun-abi` | The markers behind a field |
| `0x0000_5E29_0000_0000` | `UNSERVED_GLOBAL_BASE` | `orbistoun-worker` | Markers for globals nothing implements |
| `0x0000_5E2A_0000_0000` | `POISON_BASE` | `orbistoun-worker` | Where a poisoned handoff field points |
| `0x0000_5E2B_0000_0000` | `DESCRIBED_BASE` | `orbistoun-kernel` | Where a marker in an undescribed structure points |
| `0x0000_5E2C_0000_0000` | `DEFAULT_BASE` | `orbistoun-libc` | The fixed-base heap, when `ORBISTOUN_HEAP_BASE` is unset |
| `0x0000_5E2D_0000_0000` | `GUEST_BLOCK_BASE` | `orbistoun-mem` | Every block handed to the guest as a handle, so handle *n* is the same address every run |
| `0x0000_6000_0000_0000` | `GUEST_STACK_BASE` | `orbistoun-worker` | The guest stack |
| `0x0000_6100_0000_0000` | `THREAD_STACK_BASE` | `orbistoun-kernel` | Guest thread stacks |
| `0x0000_6800_0000_0000` | `REENTRANT_STACK_BASE` | `orbistoun-kernel` | Stacks for reentrant guest calls |
| `0x0000_6900_0000_0000` | `MAIN_TLS_BASE` | `orbistoun-worker` | The main thread's thread-local block |
| `0x0000_6A00_0000_0000` | `THREAD_TLS_BASE` | `orbistoun-worker` | Spawned threads' thread-local blocks |
| `0x0000_6B00_0000_0000` | `POLICY_REGION_BASE` | `orbistoun-service` | Regions handed to a guest by policy |
| `0x0000_7000_0000_0000` | `SUGGESTED_BASE` | `orbistoun-thunk` | The thunk table |
| `0x0000_7200_0000_0000` | `SUGGESTED_DATA_BASE` | `orbistoun-thunk` | Storage for imports that name data, not functions |
| `0x0000_7400_0000_0000` | `MAPPING_BASE` | `orbistoun-kernel` | Guest-requested mappings, when the guest expresses no preference |
| `0xffff_ffff_8c29_0000` | `KERNEL_DATA_BASE` | `orbistoun-fs` | Not a region orbistoun reserves: a hardware kernel address, in the guest's own vocabulary |

## Placement conventions

**The `0x0000_5E2*_0000_0000` family** holds regions of orbistoun's own invention: things a
guest never asked for, which exist so that a wrong pointer is recognisable when it surfaces.
Its members are spaced four gibibytes apart, and the next free slot follows the highest one in
the table. A region the guest asks for by name belongs elsewhere.

**Tebibyte spacing** applies to every other base. The spacing is far more room than any region
uses, and it is what keeps the overlap check meaningful.

`CONSOLE_SYSCALL_GADGET_BASE` is the exception. It is the hardware's libkernel base, quoted
because a payload runtime that cannot look up its syscall gadget issues `callq *0x8000004ea`
for every system call, and nothing orbistoun hands it changes that address. It sits off the tebibyte grid and still
clears the four-gibibyte check.

## Reservation failures at `POLICY_REGION_BASE`

A run reports failed reservations at `0x6b0000000000`:

```text
orbistoun: 14 reservation(s) failed, first at 0x6b0000000000,
           last: base=0x6b0000000000 len=0x400000 - conflict - the address was already reserved
```

These are expected. A policy region plants its base into a guest argument, the guest's
allocator reserves ranges at that base, orbistoun already holds it, so the hint fails and the
fallback in `sceKernelReserveVirtualRange` supplies an address (D488). The reasoning is written
above the constant in `orbistoun-service`.
