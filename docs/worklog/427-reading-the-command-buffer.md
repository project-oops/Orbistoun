# 427. Reading the command buffer, and the two tools that had to be fixed to do it

**2026-09-07** - directed, continuing 426

## What was done

Two diagnostics were repaired on the way to one measurement, and both were broken in the same
way: they reported more than they had checked.

**Guest mappings are published and named** (D579). The argument dump reads only from spans
something published as readable, and a mapping the guest makes at runtime was never among them -
so every pointer into an ordinary heap structure came back as though the address were wrong.
Published now at `mapping_placed`, the one function that means *a mapping exists*, rather than
at the three sites that place them.

**`ORBISTOUN_WATCH` no longer kills the run** (D580). It dereferenced its address before the
guest started, which for anything the guest maps at runtime is a fault inside the emulator - and
its own doc comment said it did not, directly above the raw read. A region absent at entry is
recorded as absent, and the report says what appeared there instead of a diff it cannot make.

**Both messages now say what they checked.** *"No region this run mapped"* became *"in no span
this run published as readable"*: the tool establishes that nobody told it about the span, never
that nothing is there, and this session lost its first hour to exactly that difference.

## The buffer

```text
0x7400008a3520  0x0000001400010000     u32: 0x10000, 0x14
0x7400008a3528  0x0010000000000001     u32: 1, 0x100000
0x7400008a3530  0x0000740009200000     a pointer, 1 MiB-aligned
0x7400008a3538  0x0000000000000000
```

Read as four words and a pointer, against what the run already establishes - one path resolved,
one file, `sceKernelAprSubmitCommandBufferAndGetResult(buffer, 3, out, out, 0xffffffff, 1)` -
the shape that fits is a **capacity, a used length, a command count, and a buffer**. `0x14` is
twenty bytes of command for one command; the pointer is aligned to its own `0x100000`.

**That is a reading, not a measurement**, and the difference matters: nothing here has been
confirmed by making the guest behave differently. It is written down as the hypothesis the next
run tests, in the words the loop uses for one.

Past `+0x30` the window leaves the object: a repeating `0x30`-byte record in mutually-pointing
pairs, each preceded by an image pointer, is the allocator's neighbours rather than more command
buffer.

## Surprises

- **The destination buffer is not mapped.** In a run whose `+0x10` read `0x740009200000`, a
  watch on `0x740009200000` in that same run found no published span - down to eight bytes. The
  guest holds a pointer to memory it was never given, and the run reports one to two failed
  reservations. Caveated in D580 rather than believed: *unmapped* and *unpublished* are still
  not distinguishable, and one matched pair is one.
- **Two runs of one build do not agree.** Four runs read `+0x10` as `0x740009200000` three times
  and `0x740009100000` once, moving with the reservation-conflict count. D181 and D238 require
  every measurement here to survive a repeat, and this one does not. It is also what stops the
  watch being aimed automatically, since the address has to be read out of a previous run.
- **The watch's doc, code and SAFETY comment disagreed with each other** in fourteen lines - the
  doc promising silence, the code dereferencing, the SAFETY comment admitting it faults. D385 is
  the same shape and the same repository.

## Next

- The twenty bytes of command, which need the destination pointer to be reachable in the run
  that printed it - so the determinism above is on the path, not beside it.
- Following a pointer out of a dumped argument, which is what would have made this one run
  instead of six.
- Why a reservation conflicts at all: two failures a run, unexplained.
