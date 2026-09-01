# D155 - Two names confirmed by hash, and the vocabulary extended to derive them


**decided** · 2026-08-20

The ordered tail (D154) ended with `libkernel::0x8434cc175396c635` and then a null write,
two calls after a successful allocation. That shape - allocate physical memory, then ask
where it can be reached - suggested a mapping call.

Proposing candidate names and letting the hash confirm or reject, which is the ordinary
clean-room method and consults nothing, matched two:

| hash | name |
|---|---|
| `0x8434cc175396c635` | `sceKernelMapNamedDirectMemory` |
| `0x366131779b0023bd` | `sceKernelMprotect` |

The first argument agreeing independently - a guest stack address, which is where a caller
wants to be told an answer - is what makes it more than a hash collision on a lucky guess.

`Main` and `Named` were added to the object vocabulary so
`crates/orbistoun-names/data/vendor.toml` derives both `sceKernelAllocateMainDirectMemory`
and `sceKernelMapNamedDirectMemory` from the repository's own grammar. **That is the part
that matters for provenance:** a name confirmed in a session and not added to the
vocabulary is a name nobody can re-derive, which makes it an assertion again.

`sceKernelMprotect` is *not* derivable from the current grammar and is recorded as
outstanding. It points at a pattern worth adding: the vendor wrapping a POSIX name as
`sceKernel<name>`, which would derive it and a family of others from the FreeBSD list
already harvested (D126). Not built here - it needs a solver change rather than a data
edit, and it should be done deliberately rather than as a footnote to this.

