# PPSA04263 - spins on `sceKernelDirectMemoryQuery`

99.9% of all corpus calls - 852 million in twenty seconds - and the only title reaching no
synchronisation primitives at all. It never reaches `sceKernelAllocateDirectMemory`: it
walks the map, rejects it, and starts again. Not a missing function, a rejected answer.

**The map shape is now eliminated too** (D218). It had never been anything but one free
region from zero, which also meant the earlier third-field sweep proved less than it looked
- a guest hunting for a region matching a criterion cannot distinguish "wrong value" from
"wrong shape" when there is one region. Given four regions it queried `0`, `0x20000000`,
`0xA0000000`, `0xE0000000`, `0x200000000` and restarted: **every region, in order,
correctly, and rejected anyway.**

So four answers have been swept - return code, third field, buffer clearing, map shape -
and the guest is indifferent to all of them.

**Still open, and now the sharpest question:** is the second field `end` or `start + size`?
A *contiguous* map cannot tell, because each region begins where the last ended, so feeding
back the previous end produces identical offsets either way. It needs a map with a **gap**
in it - which is the one shape `MapShape` deliberately does not currently produce, because
a gap reads as memory that does not exist and every existing shape is tested for covering
the whole range. Worth building as an explicitly-labelled diagnostic shape.

