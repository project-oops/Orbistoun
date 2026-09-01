# 2026-08-30 - A landing zone, and the first syscalls a payload has ever made here


`elfldr` does not ask for a syscall gadget. It resolves an ordinary function by name, adds ten
bytes, and calls that for every system call it will make - reaching the instruction inside a
wrapper rather than its prologue. Every address this project hands out is a thunk, and a thunk
had no inside: ten bytes in was `mov r11, trampoline; jmp r11`, so the payload entered the
dispatcher **correctly** having skipped only the index load, and the dispatcher switched on a
stale register. A well-formed call to an arbitrary function, which is worse than a crash.

A stub is now sixty-four bytes with a sled between the entry jump and the dispatch path, sliding
into the syscall gadget. Any small offset lands - deliberately not just the ten this payload
uses, because that number belongs to whichever C library a guest was built against.

**It works.** The payload resolves `getpid`, builds its entry from that thunk plus ten, and makes
three system calls, two of which are served. Resolve, offset, land, dispatch, return.

### What the sequence of red herrings cost, and why

`0x2001` was chased three times: as a constant the payload dies on, then as a handle being
dereferenced, then as the thing to serve. It is a module handle - hardware confirmed that the
same morning - and on the path a working run takes it **never comes up at all**, because the
handle-1 lookup succeeds and the `0x2001` fallback is only for when it fails.

Every one of those readings was consistent with the evidence available at the time. What
shortened it in the end was not a better guess but a watchpoint: one run that said which value
arrived and who wrote it. The lesson is not about handles - it is that three cheap
interpretations cost more than one measurement, and the measurement was available throughout.

