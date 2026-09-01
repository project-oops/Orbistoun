# D135 - The local data share is workgroup storage, and the lane model refuses it


**Status:** assumed

`ds_read_b32` and `ds_write_b32` translate to loads and stores of a `Workgroup`-class
array in the wavefront model. The per-lane model refuses them.

**Why the lane model cannot have one.** It is storage the lanes of a wavefront share, and
that model gives each lane its own invocation - so each would get its own copy and read
back only what it wrote itself. A shader using it to exchange values between lanes runs
and is wrong, which is the failure this fidelity ladder exists to prevent.

**Why workgroup storage rather than private.** One invocation is one wavefront and a
dispatch is currently one invocation, so the two are indistinguishable today. Declaring it
private would be correct now and silently wrong the first time a dispatch has more than
one wavefront per group - and the guest's local share *is* shared, so the storage class
should say so.

**Writes are masked, reads are not.** Sharper than the same rule for guest memory: another
lane of this same wavefront will read the word, so a suppressed write that lands anyway
corrupts a value a different lane is about to use. A read has no effect anything can
observe, so suppressing it would cost an instruction and change nothing.

**No initialiser**, because workgroup storage cannot have one - and the guest's local
share is uninitialised too, so a shader reading a word it never wrote gets undefined
contents in both.

