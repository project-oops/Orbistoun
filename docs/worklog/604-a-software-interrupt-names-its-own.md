# 604. A software interrupt names its own vector, and says the gap is ours

**2026-09-15** - orbistoun does the diagnosis in code, so a person does not do it by hand

## What this fixes, and why it is worth a unit

`int 0x41` walled a commercial title (PPSA04263) and the investigation went sideways for an
afternoon - first to a filesystem path, then to a "missing asset" conclusion that was simply wrong
(worklog 603). The report *had* recognised the instruction, and that is the galling part: it said

> `>> the faulting instruction is int (a software interrupt) - a privileged/trap instruction
> orbistoun does not intercept ... no import is to blame`

Two failures in one sentence:

- **A bare "int".** The vector - `0x41` - is the entire actionable fact: it names *which* kernel
  entry the guest took and therefore exactly what has to be characterised. The byte was sitting in
  `opcode[1]`, unused.
- **"does not intercept ... no import is to blame".** True, and useless. It reads as *nothing to
  do here*, which is the opposite of the truth: a retail title that runs on hardware and stops on
  `int 0x41` under orbistoun has found **orbistoun's** gap, precisely located.

So the report described the fault and then talked its reader out of acting on it. A person
(me) filled the gap by hand-decoding the bytes and reasoning to the answer. That is exactly the
work the tool should do - the model is the last resort, not the disassembler.

## What it says now

Deterministically, in the fault handler, allocation-free:

```
>> KERNEL ENTRY, UNIMPLEMENTED: the faulting instruction is int 0x41 (a software interrupt)
   The guest entered the kernel through an instruction orbistoun does not implement.
   This is orbistoun's gap, not the guest's - a retail title reaching it runs on real
   hardware, so the wall is here. Next step: characterise what this entry reads and
   returns (an obSCEne measurement, since it is below the NID/library layer), then add
   the handler. No import is involved and none is to blame.
```

- The **vector is extracted and named** (`int 0x41`), because it is the thing an obSCEne request
  must carry.
- It leads with a **category** - `KERNEL ENTRY, UNIMPLEMENTED` - so the fault class is legible at a
  glance and does not hide behind the generic `access violation, read of 0xffff...` the host
  reports for a trap instruction.
- It states the **default correctly**: a retail title reaching this runs on hardware, so the wall
  is orbistoun's. That is the assumption the whole investigation should have started from.
- It names the **next step**: a device measurement, since the entry is below the NID/library layer
  that the rest of the tracing is built around.

`syscall`, `sysenter`, `hlt` are the same category. `ud2` is split off as `GUEST TRAP` - a trap the
guest raised itself, whose cause is upstream - because conflating "orbistoun's unimplemented entry"
with "the guest aborted on a failed check" is its own way to send an investigation wrong.

## Allocation-free, deliberately

This runs in the vectored fault handler, where allocating may deadlock - the reason `Line::hex` is
hand-rolled in the first place. The first draft used `format!` and `to_owned`; both were removed and
the vector is written through `Line::text`/`Line::hex` directly. A fault reporter that allocates is
a fault reporter that can hang the process it is trying to describe (principle 9).

## Made to fail

`a_software_interrupt_names_its_vector_and_says_the_gap_is_ours` asserts the two things that were
missing - the specific vector `0x41` and the `KERNEL ENTRY` category - plus that `ud2` is
categorised as a guest trap and an ordinary `mov` fires nothing. Reverting to the bare "int" fails
it with the exact string that misled the investigation: `int (a software interrupt)` with no
vector.

## The wider point

This is one instance of a rule worth stating: **orbistoun should diagnose deterministically, in
code, in every scenario it can - the human and the model are the last resort.** The fault reporter
already decodes the instruction shape; it just was not carrying the one field that mattered or
framing the finding as actionable. The same discipline applies to the fault taxonomy generally (a
null-deref from a value orbistoun supplied is a different class from a missing import is a different
class from an unimplemented kernel entry) and to the kernel/interrupt boundary, which the tracing -
built around the NID/library boundary - still under-serves. Those are the next telemetry units.

## Gate state

`cargo fmt --all --check` clean, `orbistoun-worker` clippy `-D warnings` clean, 58+9+1 worker tests
pass. Verified on the live run: PPSA04263 now prints `int 0x41 ... orbistoun's gap` where before it
printed a bare "int" and a shrug.
