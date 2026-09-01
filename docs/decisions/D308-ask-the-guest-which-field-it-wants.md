# D308 - Ask the guest which field it wants, rather than guessing the structure


**decided** · 2026-08-27

[D306](#d306---the-payloads-take-their-world-from-rdi-and-the-stack-says-nothing) left all
five payloads dying at `0x1` having called nothing, because each takes a pointer in `rdi`
and calls through it, and nothing here knows that structure's layout.

The obvious move is to guess offsets, one boot per candidate. The better one is to make the
guest say. Both are settings on `EntryArgument`, which is what D153 built it for.

### Markers: which field, exactly, in one boot

Fill every slot with a **different unmapped address**. The guest reads one, uses it, faults -
and the faulting address says which slot it came from. Not one boot per candidate offset:
one boot for the whole structure.

```
elfldr 0.25      fetch from 0x5e2700000000    slot 0, +0
klogsrv 0.9      fetch from 0x5e2700000000    slot 0, +0
shsrv 0.20       fetch from 0x5e2700000000    slot 0, +0
ftpsrv 0.21.1    fetch from 0x5e2700000000    slot 0, +0
pldmgr 0.5.1     fetch from 0x5e2700000000    slot 0, +0
```

**The first member is a function pointer and it is called immediately.** Five programs, five
authors, one answer, to the byte. The stride is wide so a displacement the guest adds lands
inside the slot it came from - the fault then names both the field and the offset within it.

### Then: answer everything, and see how far it gets

A second setting points every slot at code that returns zero. The wall moved three times:

| instrument | klogsrv | ftpsrv | elfldr |
|---|---|---|---|
| nothing | `0x241b` | `0x241b` | `0x241b` |
| every field answers | `0x24a2` | `0x7d42` | - |
| slot 0 callable, rest writable | `0x2708` | `0x7fa8` | `0x4a48` |

The middle row taught something the first could not: after calling slot zero the guest
**wrote through** a pointer out of the block, into the read-execute stub page. So the
structure holds data pointers as well as functions, and handing every slot one executable
address cannot tell the two apart. Slot zero now gets the returning page and every other
slot its own writable one.

### Where it stops, and why that is a good place

`illegal instruction at image+0x2708`. The bytes there:

```
0f 0b 0f 0b cc cc cc cc
```

`ud2`, twice, with `int3` padding - a **compiler-emitted deliberate trap**. The guest is not
derailed into data; it ran its own code, checked something, and rejected what it was given.
That distinction is exactly the one D303 insists on, and this lands on the right side of it:
the payload is far enough in to be validating content rather than presence.

### What this says about the method

**The diagnostic was wrong before the guest was.** The first answering stub was three bytes
in a page of zeros, the guest entered it at `+0xa`, ran off the end into `00 00` - `add
[rax], al` - and faulted on a write. The report said the guest wrote to a bad address, which
was true and useless: the wrong thing was the instrument. A page of `ret` fixed it.

That is principle 3 one level up, again, and the third time this session: a tool is as
capable of plausible output as a stub is.

### And the next step is cheaper by reading than by running

The guest is now rejecting content, not presence, so more boots buy less: markers find
*which* field, and no number of them says what a field must **contain**. The payload SDK is
open source and documents its own handoff ABI, which principle 1 permits reading as prose -
recorded `published`, credited, and written out in our own words rather than pasted, since
they are GPL-3.0 and this is not.

Then the ladder does its job: `published`, promoted to `measured` when the marker run agrees.


