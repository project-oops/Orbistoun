# 2026-08-30 - The work list could not see a system call


Having got `elfldr` as far as making real syscalls, the obvious question was which one to
implement next. `orbistoun-cli worklist` is the list this project works from - 358 entries
ranked by call volume across sixteen runs - and syscall 649, the thing that actually stops the
payload, was not on it. Nor could it have been: the list totals **imports**, and a guest
reaching the kernel by number touches no stub.

The fact was being produced and thrown away, printed to stderr at the end of a run. Third time
that exact shape has turned up: the sysctl names before D397, the unanswered paths before D387,
and now this.

Recorded in the trace beside the imports rather than among them, and ranked by how many runs
asked rather than by how often - because the recorder is a bitmap and a call count would be a
number nobody measured.

### The bit that would have gone wrong quietly

The traces directory is not wiped between versions, and `cmd_worklist` **skips a file it cannot
parse with a note rather than a failure**. A new field without a serde default would therefore
have turned every existing trace into a skipped file and left the work list silently counting
only runs from today - still printing a confident ranked table. Tested, and the test says why.

