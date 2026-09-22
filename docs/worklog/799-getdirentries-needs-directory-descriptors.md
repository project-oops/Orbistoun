# 799. `sceKernelGetdirentries` is the next honest export but needs a directory-descriptor kind, not an accept stub; the clean accept-pattern hardening is done, and directory support is scoped as the next FS build

**2026-09-22** — worklogs 797-798 landed the two clean accept-pattern exports of the hardening pivot
(`scePthreadGetaffinity`, `sceCoredumpRegisterCoredumpHandler`). This tick assessed the third PPSA04263
stub, `sceKernelGetdirentries`, and it is a different size of job worth naming before it is picked up.

## It is a feature, not a stub

`sceKernelGetdirentries(fd, buf, nbytes, basep)` reads a directory's entries into a buffer as `dirent`
records. orbistoun has the enumeration machinery already - `metadata.rs` implements `opendir`/`readdir`
over `std::fs::read_dir` and builds the `sys/dirent.h` record - but the **descriptor** side does not: the
`Target` enum in `descriptor.rs` is `File` / `Socket` / `Queue` / `Device`, with no directory kind, and
`open()` hands a directory path a `std::fs::File` that cannot be walked. So a faithful `getdirentries`
needs a new `Target::Directory` (guest path plus a read position), `open()` taught to produce it when the
host path is a directory, and the syscall itself writing packed `dirent` records and tracking `basep`
across calls. That is a real FS addition, done right, not the one-line accept the previous two were - and
answering it with a bare `0` (end-of-directory) would be a lie by omission if the title needs the
entries, the plausible-empty output principle 3 refuses.

## The clean hardening is done; what is left is sized

For PPSA04263 the accept-pattern exports are answered (stubs 6 → 4 this session). The remaining stubs are
`sceKernelGetdirentries` (this feature), `sceUserServiceGetGamePresets` and `sceImeUpdate` (which need
measured or modelled values, not a convention). Across the other titles the unimplemented calls are
unnamed NIDs, faithful answers, obSCEne-bound age checks, or inline AGC non-exports - none a clean
one-tick export. So the pivot's easy fruit is picked, and the next honest step is the directory-descriptor
build rather than another accept.

## Plan

Next: add `Target::Directory` to `orbistoun-fs`'s descriptor table (path + position), teach `open()` to
produce it for a directory, and implement `sceKernelGetdirentries` in the kernel over it, reusing
`metadata.rs`'s `dirent` encoder. A real feature that answers a real POSIX syscall from published
semantics (`getdirentries(2)`, `sys/dirent.h`), removing the placeholder and giving every title that
lists a directory a true answer - and, like the others, measured against PPSA04263 for whether it moves
the wall (expected not to - the wall is the un-run static constructor, 794-796 - but the fidelity stands
regardless).

## Gate state

No code changed - an assessment that sizes `sceKernelGetdirentries` as a directory-descriptor feature
rather than an accept stub, records the clean accept-pattern hardening as done, and scopes the FS build.
`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
