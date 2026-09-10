# The address map

Every fixed base orbistoun places something at, in one list, because until D513 there was no
such list and choosing a base meant grepping five crates and hoping.

**This file is checked against the source.** `crates/orbistoun-service/tests/address_map.rs`
walks `crates/*/src/**/*.rs`, finds every `const *_BASE: u64` declared at the top level, and
fails if one is missing here, if one here does not exist, or if two land within four
gibibytes of each other. So it cannot quietly go stale, which is the failure mode a prose
map would otherwise have (D510).

## Why it exists

A fixed-base heap was given `0x7400_0000_0000` on the reasoning that it was clear of the
module base and the thunk table - both checked, both true. It is
`orbistoun_kernel::MAPPING_BASE`, where `sceKernelReserveVirtualRange` puts guest mappings.
Sixteen reservations that would have succeeded were refused and the run fell from 2077 import
calls to 219, faulting in a different module (D513).

Nothing was wrong with the reasoning except its coverage. Two of seven owners were checked
because two were the two anybody remembers.

## The map

| Base | Constant | Owner | What lives there |
|---|---|---|---|
| `0x0000_0000_6000_0000` | `TRIAL_REGION_BASE` | `orbistoun-turn` | A trial region, for a change the loop tries without a person |
| `0x0000_00F0_0000_0000` | `FIRMWARE_BASE` | `orbistoun-firmware` | The firmware skeleton - deliberately recognisable as firmware on sight |
| `0x0000_4000_0000_0000` | `DEFAULT_MODULE_BASE` | `orbistoun-worker` | A module that links at zero |
| `0x0000_4800_0000_0000` | `TITLE_MODULE_BASE` | `orbistoun-worker` | The modules a title ships with itself |
| `0x0000_5E27_0000_0000` | `SENTINEL_BASE` | `orbistoun-abi` | A sentinel block's markers |
| `0x0000_5E28_0000_0000` | `CONTENT_BASE` | `orbistoun-abi` | The markers *behind* a field |
| `0x0000_5E29_0000_0000` | `UNSERVED_GLOBAL_BASE` | `orbistoun-worker` | Markers for globals nothing implements |
| `0x0000_5E2A_0000_0000` | `POISON_BASE` | `orbistoun-worker` | Where a poisoned handoff field points |
| `0x0000_5E2B_0000_0000` | `DESCRIBED_BASE` | `orbistoun-kernel` | Where a marker in an undescribed structure points |
| `0x0000_5E2C_0000_0000` | `DEFAULT_BASE` | `orbistoun-libc` | The fixed-base heap, when `ORBISTOUN_HEAP_BASE` does not say (D513) |
| `0x0000_5E2D_0000_0000` | `GUEST_BLOCK_BASE` | `orbistoun-mem` | Every block handed to the guest as a handle, so handle *n* is the same address every run (D584) |
| `0x0000_6000_0000_0000` | `GUEST_STACK_BASE` | `orbistoun-worker` | The guest stack |
| `0x0000_6100_0000_0000` | `THREAD_STACK_BASE` | `orbistoun-kernel` | Guest thread stacks |
| `0x0000_6800_0000_0000` | `REENTRANT_STACK_BASE` | `orbistoun-kernel` | Stacks for reentrant guest calls |
| `0x0000_6900_0000_0000` | `MAIN_TLS_BASE` | `orbistoun-worker` | The main thread's thread-local block |
| `0x0000_6A00_0000_0000` | `THREAD_TLS_BASE` | `orbistoun-worker` | Spawned threads' thread-local blocks |
| `0x0000_6B00_0000_0000` | `POLICY_REGION_BASE` | `orbistoun-service` | Regions handed to a guest by policy |
| `0x0000_7000_0000_0000` | `SUGGESTED_BASE` | `orbistoun-thunk` | The thunk table |
| `0x0000_7200_0000_0000` | `SUGGESTED_DATA_BASE` | `orbistoun-thunk` | Storage for imports that name data, not functions (D323) |
| `0x0000_7400_0000_0000` | `MAPPING_BASE` | `orbistoun-kernel` | Guest-requested mappings, when the guest expresses no preference |
| `0xffff_ffff_8c29_0000` | `KERNEL_DATA_BASE` | `orbistoun-fs` | Not a region orbistoun reserves - a **console** kernel address, in the guest's own vocabulary |

## Two conventions worth following

**`0x0000_5E2*_0000_0000` is the family for regions of orbistoun's own invention** - things a
guest never asked for that exist so a wrong pointer is recognisable when it surfaces. Seven
occupants, spaced four gibibytes apart, next free is `0x0000_5E2E`. Anything the guest asked
for by name belongs elsewhere; anything orbistoun made up belongs here.

**Everything else is spaced a tebibyte apart**, which is far more room than any of them uses
and is the reason a new base has always fitted so far. Keep it: the spacing is what makes the
overlap check meaningful, and a region that grows is cheaper than one that has to move.

## The reservation failures at `POLICY_REGION_BASE` are expected, and were already explained

Every run reports failures at `0x6b0000000000` - one early on, fourteen once the guest gets
further:

```text
orbistoun: 14 reservation(s) failed, first at 0x6b0000000000,
           last: base=0x6b0000000000 len=0x400000 - conflict - the address was already reserved
```

**This is not a fault and not a mystery.** A policy region plants its base into a guest
argument; the guest's allocator then reserves ranges *at* that base; orbistoun already holds
it, so the hint fails and `sceKernelReserveVirtualRange`'s fallback supplies an address. D443
and D488 established it, and D488 confirmed it by moving the constant twice and watching every
failure follow it. The reasoning is written above the constant in `orbistoun-service`.

Recorded here because this section previously called it "a standing failure nobody has
explained" - written without grepping for the constant, in the same document that exists
because a base was chosen without grepping for the constant (D518).
