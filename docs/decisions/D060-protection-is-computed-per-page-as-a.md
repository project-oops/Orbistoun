# D060 - Protection is computed per page, as a union

**decided** · 2026-08-19

An image is populated read-write and re-protected after relocation, because population
and execution want opposite permissions. Two things about how are not obvious.

**Segment boundaries are not page boundaries.** Where a read-only segment ends partway
through a page the next writable segment begins in, that page must satisfy **both**.
The obvious loop - protect each segment in turn - applies whichever came last and
silently strips permissions the other still needs, producing a fault at an address that
belongs to neither segment obviously. So protection is computed per page as the union
of every segment touching it, then merged back into runs. Pure, and therefore testable
with nothing mapped.

Merging matters practically: one call per page across a 96 MB image is twenty-five
thousand syscalls to describe five regions.

**A write-plus-execute segment is honoured and counted, not refused or downgraded.**
Refusing would make a loadable image unloadable; downgrading would fault somewhere
unrelated, which is the failure D010 exists to prevent. It is reported instead. All
four commercial executables examined report **zero** such segments, so W^X holds
naturally rather than by enforcement.

