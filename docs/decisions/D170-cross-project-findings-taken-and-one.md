# D170 - Cross-project findings taken, and one declined


**decided** · 2026-08-21

The obSCEne thread sent loader design notes from running its suite under several other
loaders. Checked against orbistoun's actual code rather than accepted, and answered in
writing so the declined item is not re-proposed.

### Declined: an identifier-to-name database

The notes recommend generating the symbol table from a public database of 78,372
identifier-to-name pairs. **This is correct for obSCEne and disqualifying here.**

orbistoun obtains names by proposing candidates and letting the hash confirm them,
consulting nothing (principle 1, docs/PROVENANCE.md). Adopting a table would not be a small
compromise: it would retroactively poison every name already found, because afterwards
nobody could distinguish a swept name from a supplied one. The property is all-or-nothing.

The asymmetry is deliberate and pre-agreed - obSCEne may consult, orbistoun may not. Names
arriving from that side are **hypotheses to verify by hash**, recorded as `supplied` rather
than `generated`.

### Taken: a guest must be able to write to standard output

The highest-value item, and it was not implemented here at all - `orbistoun-fs` declared
its interface and provided nothing.

One loader examined implements `sceKernelWrite` purely as a filesystem call, requiring a
real opened descriptor and refusing descriptor 1, and **that single choice is why it cannot
emit a report**. A conformance probe that cannot write to standard output cannot talk to
you at all.

Implemented, with an adaptation the notes could not have anticipated: **descriptors 1 and 2
land on the host's *error* stream, never its output stream.** The worker speaks its
protocol over stdout as newline-delimited JSON, and guest bytes interleaved into that would
break the reader permanently - the same reasoning the fault reporter already follows.

`sceKernelOpen`, `Close`, `Read` and `Lseek` came with it, read-only like the rest of the
filesystem: `/app0` is the user's own title directory and nothing should write into it.

### Adopted as rules, not yet exercised

- **One implementation behind two spellings.** Not yet a problem - only vendor spellings
  are exposed - but adopted before the threading surface grows. The notes describe a
  read/write lock exposed under two names where one refuses a writer correctly and the
  other admits it, each passing its own tests.
- **Process time is not wall time**, and **sleeps are a lower bound.** Neither implemented;
  recorded so the choice is deliberate rather than inherited.
- **Grade on behaviour, not resolution.** The notes' `frontier` shape - established,
  *blocked*, deepest-green - is better than what is here, and the blocked count is the
  right number to steer by. Ours already uses two signals (D129), but one of them is an
  import count, which is inflatable exactly as the notes say.

### The caveats are the most valuable part

Zero of 112 checks are hardware-confirmed; 38 are the probe's own reasoning. So obSCEne is
a **development signal, not an oracle**: a `fail [spec]` is a defect here, a
`fail [assumed]` is a conversation that may be declined with a reason. Their framing,
honoured.

Also noted back: several findings are conclusions about another loader's *implementation*.
As written they are all "a mistake to avoid", which is safe. Shaped as "here is how they
did it", it would be the same problem as the database.

