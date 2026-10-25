# D966 - A descriptor a dispatch selects from a table at run time binds every record

**Status:** decided
**Date:** 2026-10-25
**known_by:** guest-observed

A dispatch's buffer access can read through a descriptor the program selects from a table while it
runs: an `s_buffer_load_dwordx4` through the table's own descriptor, at an offset register the
program computes, loads the four words, and a buffer access reads through them. The tracer follows
those four registers from the load, through scalar moves and across blocks, and gives every access
reading through a record of one table - one table descriptor, one place in its records - one slot,
wherever the accesses lie. Any write to one of the four registers ends it.

Where only one access reads through the table's records and the host's run up to it finds the
descriptor (D753), the access binds that descriptor's range as before. Otherwise the table is bound
whole. The host reads the table's descriptor from the dispatch's user data, and each of its records
at the record's stride, the load's immediate into it. Each record's descriptor names the range the
access's reads reach through it, by its own bounds rules, and those ranges' pages go into the
dispatch's global buffer of pages (D961). A dispatch with no global access of its own has one for
them. The record's own slot binds nothing. The module reads through the descriptor the program
loaded: it applies that descriptor's bounds, forms the guest address from its base, and finds the
word in the pages. A word the pages do not hold reads zero, and where the descriptor's bounds admit
the access it is an escape, so the dispatch's result is refused (D950). A record naming no buffer,
a base or a size of zero, binds nothing, as the hardware reads zero through it.

A format load converts by the fourth word its records share in format and selects (D738). Records
that disagree leave the slot without one, and a format load through it is refused.

A table is bound whole only where it can be: its descriptor resolves, its records are at least
sixteen bytes apart with room for a descriptor at the load's immediate, it holds at most 256
records, every record is mapped, and no record's range is unbounded or larger than one buffer
binds. Otherwise the access is found as before (D900, D914, D951), and its refusal says why the
table was not bound. Only loads read through a selected record; a store or atomic through one is
refused by name, since nothing is written back from the pages it reads.

A dispatch's `s_buffer_load` through a traced descriptor at an offset register now binds the
descriptor's whole buffer, every byte its records admit, and every such load through that
descriptor shares the slot. A scalar buffer load reads zero past its descriptor's records whatever
the offset, so the whole buffer holds every byte it can read, and the descriptor's words are the
dispatch's, the same at every access. The table's own load is one of these.

The tracer's scalar loads forget the registers they write, and no more. A load's width is its
name's; it had forgotten at least four registers, so a two-word load lost the pair after it.

**Why:** Grand Theft Auto V's compute programs select per-lane buffers from descriptor tables
through waterfalls. The program at `0x74005b857200` fetches with `buffer_load_format_x` at 0xd7c
through `s[12:15]`, loaded at 0xd70 by `s_buffer_load_dwordx4 s[12:15], s[20:23], vcc_lo`, the
offset `v_readfirstlane` of a lane's record number shifted by four. No host run of the program can
know a lane's value, so the access was refused (D753), and the wave runs could not follow its
branches on the execution mask (D758). The program at `0x74005b858800` was refused for 100 buffers,
88 of them computed, more than the 64 a stage binds (D733): its vector accesses read through
records of the table in `s[28:31]`, copied between register quads and across blocks, and its
scalar loads read `s[48:51]`, `s[24:27]` and others at lane-derived offsets, each a slot of its own.
With the tables' records sharing a slot each and the scalar loads sharing their descriptor's,
29 buffers remain, 7 of them computed. The program at `0x74005b854a00` fetches at 0x5e0 through `s[8:11]`, loaded
from `s[4:5]` + 0x60, the user data's table; `s_buffer_load_dwordx2 s[2:3]` at 0x25c had made the
tracer forget `s[4:5]`, so the fetch was taken for a computed one.

Binding a table whole is bounded at 256 records. The tables Grand Theft Auto V's programs at
`0x74005b857200` and `0x74005b858800` select from hold 131072, and are refused by that bound; the
refusal counts the records naming a buffer and the pages they reach.

**Amended by D967:** the bound refuses only binding the table whole, not the access. A table of
more than 256 records, or one whose records disagree in format where the program converts by
format, leaves the dispatch to be prepared device-addressed (D964), where the module reads the
selected record's descriptor as it runs and converts by the format it names. A table that can be
bound whole still is: its records are read once and its accesses read bound pages.

**Rejected:**
- One binding per record, as a descriptor array: a table's 256 records would take more than the 64
  bindings a stage has (D733), and every module would declare the array.
- Binding the span from the lowest record to the highest, as a sample's buffer is placed (D951):
  records are separate buffers that can lie gigabytes apart, beyond what one buffer binds.
- Device addressing (D964) for these accesses: a format load converts by its descriptor's format,
  which device addressing does not carry, so it is refused there. The records' pages carry no
  format either, but every record's descriptor is read on the host, so the one format they share is
  known when the module is translated.
- Requiring every record to be a valid descriptor: a table's unused records can hold anything, and
  the hardware reads zero through one naming no buffer.
- Writing the pages back: a store through a selected record is refused until a program needs it.
