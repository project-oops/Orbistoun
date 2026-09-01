# 2026-08-29 - The null jump is not the sysctl refusal


Both payloads that run die in their own `find_pid` with `instruction fetch from 0x0`, and
that is now the one thing between here and sockets - **neither guest reaches a socket call**,
so building Stage 2 would be building toward something unreachable.

Naming the function was the useful step: `klogsrv` at `find_pid+209`, `ftpsrv` at
`find_pid+154`. Same shared SDK helper, the one that calls `sysctl [1, 14, 8, 0]`.

**Eliminated by experiment:** patched `sysctl` to answer a size query with zero-length
success. The guest went 6 imports to 7 and the fault moved `image+0x5eaa` to `image+0x5f34`
- and still jumped to null, after the same `printf` / `__error` / `strerror` sequence.
Reverted; it was a diagnostic, not a fix, and leaving it in would have been a lie about what
orbistoun knows about processes.

Also eliminated: `strerror` works (the guest's own message carries its text), and `exit` is
implemented and does stop the run. What is left is what the guest does *after* reporting.

### Two hours of the workspace fighting back

Three separate breakages from the concurrent session, all transient, all costing a
diagnosis:

1. `orbistoun-cli` failed to compile - `write_answer` not found. It was defined nine seconds
   later; I had compiled mid-write.
2. `libkernel.toml` had a duplicate key and the emulator panicked at startup. Fixed by them
   within the minute.
3. Then it *kept* panicking after their fix, because the knowledge files are `include_str!`d
   at compile time - the binary had embedded the broken copy. A rebuild was the fix, and
   nothing in the error said so.

The third is worth keeping: **a data file that is embedded at build time fails as though it
were still broken until something forces a rebuild**, and the message names the file rather
than the binary.


### The loop writes down what it works out

The answer from the previous entry - *the guest walks the map by feeding back each region's
end* - existed only as terminal output. A day spent applying "anything that exists only in a
conversation is already lost" to measurements, and answers had the same hole.

An answered question is a **proposal** now: a patch against the entry that asked it, inert
until promoted, `known_by = "measured"` because the guest was put in a situation built to
separate two readings rather than merely watched. Re-running proposes nothing - a settled
question producing a fresh patch every turn is how `patches/` becomes noise.

The whole cycle, verified: turn answers, writes a proposal, the patch applies, the file
parses, the tool reads it back, and the next turn sees it settled.

### Two bugs, both "applies is not valid"

The first patch inserted a key the entry already had - `duplicate key edge_cases in table
function` - and `git apply` accepted it without complaint. A key that exists must be joined,
not added again, and the join has to be scoped to *that* entry: searching the file finds
whichever came first and files one function's answer under another's name. A patch that
applies, parses, and lies.

The second left the line ending in a space, because the array is multi-line so joining onto
`edge_cases = [` leaves a trailing separator. Warned about, nothing failed.

**Three times now a generated patch has applied cleanly and been wrong** - the invented
`found_by`, the duplicate key, the trailing space - and each was caught by a checker that
understands the format, never by reading the diff.

### The thing that nearly hid the last step

`knows` did not show the answer after the patch applied. Knowledge files are `include_str!`-ed,
so the running binary held the old copy - D260, unchanged, and still the first thing to
suspect when a data change appears not to have landed.

