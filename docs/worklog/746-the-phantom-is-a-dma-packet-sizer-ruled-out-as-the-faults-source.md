# 746. The phantom is a DMA-packet sizer, ruled out as the fault's source

**2026-09-21** — with the disassembler from 745 in hand, the phantom GetSize's call site finally reads
as code instead of a hypothesis. It is not where the wall's `0xa8` comes from. The association between
the phantom's written `0xa8` and the fault's `src=0xa8` — carried since worklog 725 and written into the
handler's own doc comment — is a coincidence of value, and the disassembly plus 740's experiment
retire it.

## What the phantom's call site actually does

The phantom `0x7d86501b8094ef57` is called (twice, both from `0x4000000435ba`) inside a **DMA/wait/EOP
packet producer**, disassembled from a fixed-base code dump:

```
0x435b5  call phantom                 ; writes a byte size to *out = [rbp-0x470]
0x435ba  mov  rbx, [rbp-0x470]        ; rbx = size (0xa8)
0x435c4  add rbx,7 ; and rbx,~7       ; align8 -> 0xa8
0x435cf  call sceAgcDmaDataPatchSetDstAddressOrOffset (rdi=r12, rsi=0xa8)
0x435da  call sceAgcWaitRegMemPatchAddress            (rdi=r14, rsi=0xa8)
0x435e5  call sceAgcQueueEndOfPipeActionPatchAddress  (rdi=r15, rsi=0xa8)
```

The trace confirms the callees by name. So the phantom sizes a set of **DMA-data / wait-reg-mem /
end-of-pipe** packets, and its `0xa8` is handed to that `sceAgc*Patch*` family as an
address-or-offset argument. This is a different structure entirely from the register-group descriptor
whose data pointer faults.

## Why it is not the fault's `0xa8`

Two independent reasons, and they agree:

- **The value does not propagate.** 740 changed the phantom's written value to `0xb8`; the fault stayed
  `src=0xa8`. The producer here reads that written value straight into `rbx` and passes it on, so if the
  fault's `0xa8` came through this path it would have tracked the change. It did not.
- **The structure is wrong.** This path feeds `0xa8` into DMA/wait/EOP *patch* calls (measured no-ops),
  not into a register-group descriptor's `[desc+0x18+8k]` data-pointer slot. The faulting `src` is that
  slot's value (745), reached through `0x37ea0` from `0x38xxx`, a path this producer is not on.

So the phantom is a red herring for this wall. It is correctly implemented as a size-writer
(worklog 725's structure holds), it is simply **not connected to the fault**. The handler's doc comment
in `agc.rs` still says its `0xa8` "is the size the guest's `memcpy` reads at the wall this path dies
on"; that sentence is now known false and is flagged here for correction by whoever next touches that
file (a concurrent session's edits are live in it, so it is not amended from this tick).

## Where that leaves the wall

The register-group descriptor's bad data pointer (`0xa8`) is written during the descriptor's
construction, and that descriptor is a **pre-built element of a collection** at `[shared+0x38]`
(739) — fetched by the accessor `0x3fdd0`, not built inline on the faulting path. So the write that
sets the slot to `0xa8` happens earlier than any frame this run reaches, into an ASLR'd heap object
whose address is only known at fault time. That is the crux the available tools do not reach: a
watchpoint needs the address before the write, and the write is before the address is knowable.

## The highest-payoff next step

Four worklogs now (738, 744, 745, 746) have turned on reading guest code at a fault, three of them
hand-decoding before capstone. The recurring cost is the case for teaching orbistoun's own fault report
to **disassemble the code window it already dumps** — a permissive x86 decoder (`iced-x86` MIT, or
`yaxpeax-x86` 0BSD) passes `deny.toml`'s allow-list, and `dump_at_fault`/the `ORBISTOUN_PEEK` renderer
already have the bytes and the base address. That is a reusable capability that serves the standing
"findable without an oracle" instruction and makes every future wall — including the remaining
construction-site question here — cheaper to read. It is the next tick's work.

## Gate state

No code changed — fixed-base code dumps, disassembly, and analysis only. `./bin/orbistoun check` is
unchanged from 745 (green but for the same three generated-doc drifts from a prior session's uncommitted
`compat/PPSA02664-app0.toml` edit, not this tick). Identity scan clean. No commit.
