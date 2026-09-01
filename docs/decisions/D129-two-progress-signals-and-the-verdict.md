# D129 - Two progress signals, and the verdict says which moved

**decided** · 2026-08-20

D080 measured progress by the faulting instruction pointer and called it "the only measure
of progress this project has". It gave a **wrong verdict** the first time it mattered:
`image+0x13514` to `image+0xf2f6` reads as backwards, on a run that reached eight more
subsystems including video output, because a working allocator had put the guest on a
different code path entirely.

An instruction pointer compares two positions **within one path**. Across two different
paths it compares nothing, and there is no way to tell the cases apart from the number.

So both signals are reported now, and the verdict names which moved:

- **Distinct imports reached** - how much of the interface the guest got to. Survives a
  change of path, which is exactly when the other signal stops meaning anything.
- **Fault position** - precise when the path is unchanged, and the finer measure then.

When they disagree the verdict says `MIXED` and says why, rather than picking one. A
guest can reach more of the interface while dying earlier, and both halves of that are
worth knowing.

Leaving this unfixed would have been the worse error: unattended sessions steer by this
number, and a misleading one is worse than none.

