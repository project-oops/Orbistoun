# Notes from the obSCEne thread: one declined, one implemented


Design notes arrived from the obSCEne side, from running its suite under several other
loaders. Checked against our code rather than accepted, and answered in writing (D170).

**Declined:** generating the symbol table from a public identifier-to-name database. Right
for obSCEne, disqualifying here - it would retroactively poison every name already swept,
because nobody could afterwards tell a swept name from a supplied one. Answered in writing
specifically so it is not re-proposed. Yesterday's yield, for scale: 249,506 candidates,
one name. The slowness is the provenance.

**Implemented:** `sceKernelWrite` to standard output, plus `Open`, `Close`, `Read` and
`Lseek`. `orbistoun-fs` had declared its interface and implemented none of it. The notes
report a loader that refuses descriptor 1 outright, and that one choice is why it cannot
emit a report at all - a probe that cannot write to stdout cannot talk to us.

With one adaptation they could not have known about: **guest descriptors 1 and 2 go to the
host's *error* stream.** Our worker speaks its protocol over stdout as newline-delimited
JSON; guest bytes there would break the reader permanently. Same reasoning the fault
reporter already follows, and it is in the reply so obSCEne does not assume stdout is the
channel.

**Confirmation rather than news:** their point that a stub returning success is worse than
not resolving at all. That has been principle 3 here from the start - and it landed the day
after discovering our stub policy was wired to nothing, so the principle was documented-true
and implemented-false at the same time for months. Their barrier bug is a good independent
argument for why that mattered.

Offered back: the control-experiment method, which has now found three real bugs here in
two days that reading the code did not. And a suggestion - if obSCEne can assert
`rsp % 16 == 8` on entry to a probed function, that is a conformance check no loader passes
by accident and that fails silently everywhere else.

