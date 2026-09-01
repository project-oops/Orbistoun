# D065 - Execute is never dropped, and never means "no access"

**decided** · 2026-08-19

A bug worth recording in full, because the shape of it will recur.

Guest text segments here carry `p_flags` of **`0x1`** - execute with the **read bit
clear**. The Windows protection mapping tested `read` before `execute`, so
`(read: false, write: false, execute: true)` fell through to `PAGE_NOACCESS`. The image
placed correctly, relocated 174,172 entries correctly, reported five protection runs
correctly, and then faulted on its very first instruction fetch.

**Everything upstream said the load was fine.** The entry-point check even passed - it
asks whether the entry sits in a segment whose *flags* say executable, and they did.
Only the page said otherwise. Two layers agreeing and a third disagreeing is what made
it look like a bad entry point rather than a protection bug.

The corrected mapping never drops execute:

| read | write | execute | page protection |
|------|-------|---------|-----------------|
| any  | yes   | yes     | execute-read-write |
| any  | yes   | no      | read-write |
| any  | no    | yes     | **execute-read** |
| yes  | no    | no      | read-only |
| no   | no    | no      | no access |

Granting read alongside execute is accurate rather than lax: classic x86-64 paging has
no execute-without-read, so the two behave identically in hardware.

**What found it** was the fault reporter, not reasoning - `instruction fetch from
image+0x70` named the operation and the address in one line. Before it existed the same
failure read only as "access violation".

