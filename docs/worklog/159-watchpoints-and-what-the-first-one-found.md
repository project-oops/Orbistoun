# Watchpoints, and what the first one found


`ORBISTOUN_WATCH` copies a region and diffs it, which says *which words nobody wrote*.
D223 chose that over a watchpoint and gave the reason. The question at the
`image+0xafc959` wall stopped being that one: twenty-three dye runs had eliminated every
function `PPSA02664` calls as the source of the missing region base, so what was left was
*who reads the word nobody wrote*, and a snapshot cannot answer that at all. So the
expensive half got built (D276).

Hardware, not simulation: x86 debug registers, four of them, set through a helper thread
because a thread cannot suspend itself. The vectored exception handler that reports faults
already existed, so this is one new branch in it rather than new machinery. A data
breakpoint is a **trap**, so the instruction pointer belongs to the instruction *after* the
access - and naming the one that did it needs its length, which is disassembly. Every line
says `after the access at` for that reason (D277).

**The first one armed against a real title reported nothing at all** - no hits, and not even
the fault that run had produced every other time. Armed on an address the guest never
touches, everything worked. So arming was sound and trapping was not: the handler reads the
watched word to say what the instruction saw, the debug register is still live while it
runs, and under a read-or-write watchpoint that read is itself a watched access. It traps,
reads again, and the process dies having said nothing (D278).

Worth recording because of how it failed. No hits and no fault **reads as a result** - "the
guest never touched it and never got that far" - and both halves are false. The firing test
now uses a read-or-write watchpoint rather than a write-only one, because write-only never
re-enters and would have passed with the hazard still there.

### What it found in four runs

- `image+0x19e9cb0` - the object slot every hypothesis since D171 has been about - is
  **never touched**. Not read, not written, by guest or emulator. That is not a slot nobody
  filled in; it is a slot nobody wants.
- `image+0x19765c8` is a **vtable**. The site report gives it away: the instruction pointer
  equals the value read, which is what `call [mem]` looks like when the trap fires after the
  instruction completes - the access is the slot load and the "next instruction" is the call
  target.
- The faulting function reads `object+0x00` three times, at `image+0xafc90f`, `+0xafc928`
  and `+0xafc946`, immediately before faulting at `+0xafc959`.
- Watching a **stack** address is close to useless: `0x600000800d38` came back with ten
  distinct sites and forty-eight hits, every one of them from the emulator's own code
  reusing the slot. Watch structures, not frames.

### The wall symbol has a name, and it arrived by the other route

`0x6abac2f3dc6f8cee` is **`sceKernelReserveVirtualRange`**, hash-confirmed. It is in
`symbols/generated.json` as `found: static, by: module-strings, from: titles/obscene/eboot.bin`
- so it came out of the strings reader rather than the grammar, which is why sweeping three
billion generated candidates never produced it.

That reframes the wall. The last import call before the fault is
`sceKernelReserveVirtualRange(0x600000800d38)` from `image+0x1595d8b`; nothing declares or
implements it, so it lands on a generic stub. At the fault `rdx = 0x100000` is a plausible
length, `rbx = 0x20` an alignment, and `rax = 0` a base that was never granted -
`0xfffe0` is exactly `0 + 0x100000 - 0x20`.

**The earlier elimination is not wrong, it tested the wrong channel.** Dyeing that call's
*return value* moved nothing, and would not: a reserve hands the granted base back through
an **out-parameter**, and the return is a status. Twenty-three functions were eliminated on
their return values and their offset-zero out-parameters, and this is the shape that gets
through both - the same shape as D210 and D171 before it.

Next is a decision rather than a measurement: what the out-parameter contract is, and
whether reserving really means reserving here.

