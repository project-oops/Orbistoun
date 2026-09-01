# D306 - The payloads take their world from `rdi`, and the stack says nothing


**decided** · 2026-08-26

With [D305](#d305---a-plain-name-is-a-nid-nobody-hashed-yet) in place, `klogsrv 0.9` parses,
resolves, maps, **links completely** and enters. It then faults immediately. Six runs settle
what the entry point wants, and one of the six is not what anybody expected.

`EntrySettings` offers two conventions and three arguments, which is six combinations and
six relaunches - no rebuild, which is the whole of principle 5 and why D153 made them
settings.

| convention | argument | where it died |
|---|---|---|
| function | image-address | jumped to `0x1` |
| function | zeroed-block | jumped to `0x0` |
| function | zero | ran to `image+0x2751`, **read** null |
| process | image-address | jumped to `0x1` |
| process | zeroed-block | jumped to `0x0` |
| process | zero | ran to `image+0x2751`, **read** null |

### The convention makes no difference at all

Not "little difference" - none. `rsp` moves between the two rows (`stack+0x800ee8` against
`stack+0x800ef8`), so the setting is reaching the guest; the fault address, the frame chain
and the register file are otherwise identical.

**So these payloads read nothing from the stack at entry.** That is the opposite of what
D159 measured for vendor titles, where the convention decided whether 2 or 372 calls landed
on a conforming stack. It is one more way the two kinds of module are not the same guest
wearing different tables.

### What the argument decides is everything

The three arguments produce three different deaths, and they line up:

- Hand it the image address, and it jumps to `1`.
- Hand it a zeroed block, and it jumps to `0`.
- Hand it nothing, and it stops jumping - it runs another `0x330` bytes of real code and
  reads through a null instead.

That is one behaviour seen three times: **the entry point calls through a pointer it takes
from `rdi`**, and what it called was whatever happened to be at the front of what it was
given. `1` is not a coincidence either - it is the argument count sitting at the top of the
process image, read as a function pointer.

`rdi` reads `0x1` in the fault frame under *all three* arguments including `zero`, so that
register is the payload's own by the time it faults and is not what orbistoun handed over.
Worth stating because it invited exactly the wrong conclusion for an hour.

### It is all five, not one

Measured on `klogsrv` and then, because one guest is an anecdote, on the rest:

```
elfldr 0.25      instruction fetch from 0x1   reached 0 distinct imports
klogsrv 0.9      instruction fetch from 0x1   reached 0 distinct imports
shsrv 0.20       instruction fetch from 0x1   reached 0 distinct imports
ftpsrv 0.21.1    instruction fetch from 0x1   reached 0 distinct imports
pldmgr 0.5.1     instruction fetch from 0x1   reached 0 distinct imports
```

Identical, to the address. Five programs by different authors, of very different sizes,
sharing one entry contract - which is what makes it a property of the toolchain and its
loader rather than of any one payload, and is the strongest evidence here that the thing
being missed is a single structure rather than five separate problems.

### Where this stops, and why it stops rather than continuing

The payload wants a structure whose layout is not derivable from anything in this
repository. Inventing one to get past the fault is the failure principle 3 names: a guest
that proceeds on an invented field is not evidence about anything, and the invention would
be indistinguishable from a measurement a week later.

Two routes remain and both are honest:

1. **Sweep it.** Grade candidate blocks by whether the guest proceeds - oracle three, one
   bit per boot, which is the machinery `orbistoun-turn` already is. Expensive and correct.
2. **Leave it.** `argument = zero` is the furthest any setting reaches and it is the
   payload's own no-arguments path, which is a legitimate thing to be on.

What this is **not** is a loader failure. Parsing, resolution, mapping, relocation and entry
all work on a module orbistoun refused outright this morning. The wall moved from "cannot
read this file" to "does not know what this program wants to be handed", which is a
different and much later question.

