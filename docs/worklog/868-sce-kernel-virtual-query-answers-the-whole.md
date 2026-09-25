# 868. sceKernelVirtualQuery answers the whole 72-byte structure the console writes

**2026-09-25**. obSCEne measured `SceKernelVirtualQueryInfo` on 2026-09-14
(REQ-20260914T1110Z-9b12), and orbistoun never took it up. Its entry still said `assumed`, and it
wrote start and end and left 56 bytes as the caller had prepared them.

**Measured** (`020-memory/virtual-query-*`, the probe pre-filling 72 bytes with `0xAA`, all 72
changed):

| offset | field | direct mapping | text | main stack |
|---|---|---|---|---|
| `+0x00` / `+0x08` | start / end | | | |
| `+0x10` | physical offset | `0x2a20000` | 0 | 0 |
| `+0x18` | protection (u32) | 3 | 4 (execute-only) | 3 |
| `+0x1c` | memory type (u32) | 0 | 0 | 0 |
| `+0x20` | flag byte | `0x12` | `0x11` | `0x15` |
| `+0x21` | name | `anon` | `executable` | `main stack` |

The flag bits are flexible `0x01`, direct `0x02`, stack `0x04` and committed `0x10`. An unmapped
address answers `0x8002000d` (EACCES), not ENOENT.

**Changed.**

- `query_region` + `QueryRegion::encode` write all 72 bytes.
- A direct mapping (found through the physical-alias table) and the main stack are answered in
  full.
- Flexible mappings get their protection and the flexible and committed bits.
- Noted regions (image, modules) and thread stacks get bounds and zeros. Their per-segment answers
  are not measured, and orbistoun notes the image whole.
- An unmapped address answers EACCES.

The knowledge entry is `measured`, with the gaps in `partial`.

**The first cut went BACK on three Unity titles, and the cause was the old code.** PPSA25872's
allocator reserves 1 MiB, queries it, and tests the committed bit (`+0x20 & 0x10`,
`image+0x1ae2849`):

- If the range is committed, it compares the protection at `+0x18` with the `0xf2` it wants and
  calls `sceKernelMprotect` if they differ.
- If not, it allocates direct memory and maps it.

The old answer left that byte as stack garbage with `0x10` set, so the guest took the `mprotect`
path by accident. The first cut answered orbistoun's reserve-and-back as committed read-write,
which skipped both.

Now:

- A reservation nothing is mapped into answers uncommitted, protection 0
  (`a_fresh_reservation_is_not_committed`, watched failing). This is inferred from what reserving
  means, not measured.
- Protection is what the guest *asked* for, not orbistoun's grant. The guest compares the field with
  `0xf2`, so the console hands GPU bits back (guest-observed).
- `sceKernelMapFlexibleMemory` mappings answer as flexible, although they go through the direct
  path.

PPSA25872 now takes the allocate-and-map path. It makes one fewer distinct import (the accidental
`mprotect`) and one more map. Its wall is unchanged.

Tests:

- `a_query_encodes_what_the_console_wrote` rebuilds two hardware records byte for byte. It was
  watched failing with the flag byte moved.
- `a_query_of_nothing_is_refused_as_the_console_refuses_it`.
