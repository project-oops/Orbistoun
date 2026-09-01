# D340 - The pad library exports both spellings, and both are imported


**decided** · 2026-08-27 · a wrong conclusion, corrected by one command

`orbistoun-input` declared `scePadInit`, `scePadOpen`, `scePadClose`, `scePadReadState`,
`scePadRead` and `scePadSetVibration` and implemented none of them. Before implementing,
those six names were checked against the symbol database - and **none of them is in it**,
while ninety-one other names from the same library are.

The conclusion drawn from that was that the six were previous-generation names, recalled
rather than derived, and therefore hash to NIDs no import would ever carry. The declarations
were rewritten to the confirmed siblings - `scePadOpenExt`, `scePadReadStateExt`,
`scePadSetVibrationForce` - and a whole argument written about unreachable shims.

**It was wrong.** `orbistoun-cli imports titles/obscene/eboot.bin` lists ninety-seven pad
imports and includes *both* halves of every pair: `scePadOpen` and `scePadOpenExt`,
`scePadReadState` and `scePadReadStateExt`, `scePadSetVibration` and
`scePadSetVibrationForce`. The library exports both and a real module asks for both.

So both are declared, and one implementation serves each pair - deciding which of a pair a
title "really" uses is a guess with no upside.

**What the mistake was made of.** Absence from a generated database was read as evidence
about the platform. That database holds what the naming loop has derived and what a harvest
has read; a name missing from it means *nobody here has named it yet*, which is a fact about
this project rather than about the vendor. The direct evidence - what a module in the
library actually imports - was one command away, and it is the command `CLAUDE.md` opens
with.

