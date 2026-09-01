# D168 - The harvested name list is missing the syscall family


**decided** · 2026-08-21

The `sceKernel<posix-name>` idea (D155, D167) was going to be built as a solver pattern.
Measuring first: 108,117 candidates composed from the 2,637 harvested names against the
sixteen still-unnamed hashes. **Zero matches.**

Then the control, because a negative from an unvalidated instrument is worth nothing -
which this project has now said three times. `sceKernelMprotect` is a *confirmed* name of
exactly that shape, so the sweep must be able to produce it. It cannot:

```text
mprotect harvested?  False
sweep generates it?  False
```

`mprotect` is not in the harvested list. Nor are `mmap`, `munmap`, `madvise`, `mlock`,
`lseek`, `pread`, `pwrite`, `openat`, `ftruncate`, `fstat`, `fstatat`, `access`, `ioctl`,
`execve`, `getpid` or `kill`. **The syscall family is largely absent**, while stdio is
complete and `open`, `read`, `write`, `close` are present - so this is a gap in the source
that was read, not a filter rejecting a category.

The harvester does walk subdirectories, so `lib/libc/sys/Symbol.map` would have been found
had it been in the tree. The likeliest explanation is a partial checkout, and the harvester
already prints `note: … is not present, skipping` for missing paths - which nobody was
watching for.

### What this changes

**The negative result is void**, and so is the case for building the solver pattern on it.
The pattern may well hit; it was tested against a vocabulary that cannot express the very
name that motivated it. Building the solver change now would have been building on a
measurement that was already known to be unreliable, had anyone checked.

So item one is **blocked on re-running the harvest against a complete tree**, which needs a
FreeBSD clone this machine does not have. `orbistoun-cli harvest <path>` already does the
work; only the input is missing.

### The lesson, stated because it keeps recurring

Three times in two days a result has turned on validating the instrument rather than
reading the code: the entry convention (D159), the stub policy that was wired to nothing
(D166), and now a name sweep whose vocabulary could not contain the answer.

The pattern is always the same shape - **something that looks present from every angle
except the one that matters** - and the check is always the same: make it produce a result
you already know, and see whether it does. A harvest that reports "2,637 symbols" looks
like success from every angle except asking it for a symbol you can name yourself.

