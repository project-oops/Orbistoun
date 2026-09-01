# D125 - Two routes to a shader address, and the disagreement is the point


**Status:** assumed

`Pipeline::submit` takes a list of shaders the guest registered by name as well as the
submitted packets. Registration wins where the two overlap, and the report counts every
overlap both ways: `agreed`, and a list of `disagreed` naming both addresses.

**Why registration is believed.** Not because it is more fundamental - the packets are
what the hardware executes, and the guest can hand-roll or patch a command buffer without
the library ever seeing it. Because registration is *stated* and the register path is
*inferred*, and the table that inference rests on is described by its own data file as
"the least certain thing here".

**Why the inferred route keeps running anyway.** Agreement is the only evidence available
that the register vocabulary is right, and disagreement is the only evidence that it is
wrong. Neither is obtainable without both routes running on the same submission. Stopping
at the first answer would be faster and would throw away the one signal this subsystem
cannot generate for itself.

This came out of the loader thread identifying the guest's graphics entry points. The
names show a command-buffer *builder* library - calls that append packets, and a separate
call that submits the buffer - which is what makes two routes exist at all.

**Inferred from names, and built to degrade honestly.** Whether the registration call
carries an address has not been confirmed. If it does not, the registry stays empty, the
inferred route answers as before, and nothing about this is load-bearing.

