# 743. An indirect peek breaks the ASLR barrier, and reads the fault's heap objects

**2026-09-21** — worklog 739 named the next step and the wall in front of it in the same breath: read
the register-group descriptor's fields, but its address is re-randomised every run, so no static
`ORBISTOUN_PEEK` spec can name it. This tick builds the tool that removes that wall — an **indirect
peek** — and uses it to read the heap objects around PPSA02664's fault for the first time. The tool is
the durable result; the descriptor itself is narrowed but not yet pinned, and the honest state is
recorded as such.

## The barrier, and the tool that removes it

A heap object's address is ASLR'd — across three runs this tick the copy destination `rax` was
`0x74000230f070`, `0x740001fcf070`, `0x7400020cf070` — so a value read from one run's stack dump is
meaningless in the next. But the **stack** sits at a fixed base (the `0x600000…` addresses are identical
run to run), so the *slot* that holds an object's pointer does not move even as the object does. Naming
the slot reaches the object where naming the object cannot.

`ORBISTOUN_PEEK` gained a bracket form, `[<slot>][+len]`: dereference the pointer at `<slot>` at fault
time and dump `len` bytes of whatever it points to. It sits beside the existing `caller` and
`<addr>[+len]` forms in `orbistoun-worker`'s fault dumper, reads only (it stays `Effect::Observes`), and
prints a `via [<slot>] -> <target>` note so the resolved address is on the record. `parse_indirect` is
unit-tested both directions — the bracket form yields the slot, a plain `<addr>+len` does not parse as
indirect and falls through to `parse_addr_len`, and malformed brackets dereference nothing. This is the
"find this class without oracle guidance" instruction applied to the tooling: a copy from an ASLR'd null
object is now readable from a single run, by naming the stack slot the debugger can see.

## What it read

With `[0x6000007fc0b8]+0x80,[0x6000007fc0d8]+0x80,[0x6000007fc178]+0x80` the dumper followed three
stack slots into this run's heap and printed the objects — the indirect resolution worked exactly as
intended (`via [0x6000007fc0b8] -> 0x7400020c7c28`, and so on). Two facts fell out immediately:

- **The null container is a `0xf8`-byte object.** The fault register dump now carries `r8 = 0x50` (the
  copy count `n`) and `r9 = 0xf8`. Since the copier reads its container at `+0xa8` for `0x50` bytes, and
  `0xa8 + 0x50 = 0xf8`, `r9` is the container's total size: a `0xf8`-byte object whose last `0x50` bytes
  (from `+0xa8`) are the register data, and whose base is null on this path.
- **None of the three objects is the flat group descriptor.** 738 sketched the descriptor as parallel
  arrays — data pointers at `+0x18, +0x20, …`, present flags at `+0x5b, +0x5c, …`. The object at
  `0x7400020c7c28` has pointers at `+0x00..+0x38` and `u32` counts after, with a null at `+0x10`; the one
  at `0x7400020c7868` carries a C++ vtable at `+0x20` (`0x400000f849d0`, a fixed guest-code address);
  the third is mostly zero. So the descriptor whose group-nine pointer is null is reached through a slot
  I have not identified yet — the three I sampled are its neighbours on the stack, not it.

## Honest state

The tool is done and proven; the descriptor is not yet in hand. The `+0x10` null in the first object is
a tempting match but does not fit — the faulting copy is the **tenth**, and that object has no tenth
pointer slot, so a null at its third would have faulted the third. The right next step is now cheap and
does not fight ASLR at all: the copier `0x42d90` and its call site `0x42ebd` are at the **fixed** guest
base, so `ORBISTOUN_PEEK=0x400000042d90+0x140` (a direct, stable read) dumps its code, and the
instruction that computes `rsi = [container-source]` names the exact register and offset the null comes
from. With that offset, one indirect peek at the matching slot reads the descriptor — and then a
watchpoint on the null field (739) catches whether the producer ever writes it.

## Gate state

Changed `crates/orbistoun-worker/src/report.rs` (the `[<slot>]` indirect form and its parser, with a
test). `cargo fmt --check`, `cargo clippy --all-targets` and the crate's tests are clean; the workspace
gate is run below. `./bin/orbistoun check` remains red only on the same three generated-doc drifts from
a prior session's uncommitted `compat/PPSA02664-app0.toml` edit (740–742), not this change. Identity
scan clean. No commit.
