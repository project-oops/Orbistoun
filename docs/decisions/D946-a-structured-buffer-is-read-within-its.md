# D946 - A structured buffer is read within its records

**Status:** decided
**Date:** 2026-10-21
**known_by:** assumed

A buffer descriptor in the structured out-of-bounds mode (`OOB_SELECT` 1, bits 29:28 of its fourth
word, `gfx10-rsrc.json`) checks a read's record index against `NUM_RECORDS` and leaves its offset
within the record unchecked. Where the program's offset reach is not known, its reads are taken to
lie within their record, as a vertex fetch's do: the binding runs through the last record's stride,
`NUM_RECORDS` times the stride in all. A known reach is bound as before. Recorded assumed, as the host's
binding of a stride-less structured buffer is (D917): an offset past its record reads the next
record's bytes on hardware, and nothing here has seen a program do that.

**Why:** Grand Theft Auto V's vertex fetch dispatch at byte 0x185fc reads 64-byte records by index
(`buffer_load_ushort` at offsets 0, 2 and 4) through a structured descriptor of 1142 records, and was
refused as unbounded; 131 submissions waited behind it.
