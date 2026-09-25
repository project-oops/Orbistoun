# 870. The APR resolve answers its measured contract, and PPSA25872 moves

**2026-09-25**. obSCEne measured `sceKernelAprResolveFilepathsToIdsAndFileSizes` within two hours
of REQ-20260925T1834Z-a7e2, pre-filling each out-array with a signature:

- `arg2` holds `u32` ids;
- `arg3` holds `u64` sizes;
- `arg4` holds `u32` statuses;
- `arg5` is an input that may be null and is never written.

An unresolvable path gets id `0xffffffff`, size 0 and status 0, and the call returns -1.

**Changed.** Every entry's three slots are written at those widths.

- A path in the title's own index (D591) gets the index's id and size, status 0, and the call
  returns 0. This is D592 closed: the index answer is now written by default.
- Any other path gets the measured unresolved answer. What a successful resolve outside an index
  returns on hardware is not measured, and no id is synthesised for it.

`ORBISTOUN_APR_ANSWER` and `answer_resolve` are gone: that experiment existed only because the slots
were unknown. The knowledge entry is `measured`.

Test: `an_unresolved_path_fills_every_slot_at_its_measured_width` rebuilds the signature fill. It
was watched failing with D679's four-byte size write.

**Measured.**

| title | before | after |
|---|---|---|
| PPSA25872 | trap `int 0x41` at `image+0x17554a3` | +43 distinct imports, +17,551 calls, write to `0x1ff7c` at `image+0x3b383b` |
| PPSA03416 | | `globalgamemanagers` resolves from its index (entry 26, 224,748 bytes); one fewer import |
| PPSA02664, PPSA04263 | | same |

PPSA25872's new wall is where its best-ever record stood on 2026-09-13, so it has recovered the
reach it had lost. On PPSA03416 the missing import is `__error`, the errno read on the old failure
path, which the resolve no longer takes. Its wall is unchanged.

None of the sibling projects (oops-sdk, oops-mesa, oops-apps) call APR, so obSCEne's measurement is
the only hardware source for it.

**PPSA25872's wall, read.** `image+0x3b37d0` uploads constants per slot.

- `lookup(shader, table, slot)` (`image+0x3ba960`) returns a `u16` register index from the table
  whose pointer is at `shader+0x08`, bounded by the count at `shader+0x2e`. Past the count it
  returns `0x7fff`.
- An index below `0x20` goes to `[r14+0x10] + idx*4`, the user-data registers. Anything else goes
  to `[r14+0x18] + idx*4 - 0x80`.

At the fault the binding table (`0x74000b2f9d00`) holds count 2 and slots that are all `0xffff`.
The upload is for slot 2, so `0x7fff` comes back and the extended path writes through a null
`[r14+0x18]`: `0x7fff*4 - 0x80 = 0x1ff7c`. Either the reflection data behind that table should
map the slots, or the extended buffer should exist. oops-sdk passes shader headers through opaque,
so the sibling projects do not settle this layout.
