# D125 - An error code in a pointer register is a wild pointer

**decided** · 2026-08-20 · the next piece of work, not yet built

Reading the full call list of a title that faults with `read of 0x5` shows what it managed
before dying: C++ initialisation, string work, and then **`scePthreadSelf`** - which
returns a thread handle, is unimplemented, and therefore returned an error code. The guest
read a field off it.

This is systemic rather than one function. `StubPolicy` answers every unimplemented call
with an error code, which is right for a function returning status and **actively
dangerous** for one returning a pointer or a handle. The guest does not get a failure it
can check; it gets a small integer it dereferences.

**Null is the more honest answer for a pointer-returning function.** It is what a real
`malloc` or `dlsym` returns when it cannot do the job, guests already check for it, and a
null dereference faults at a recognisable address instead of somewhere random.

That needs the stub layer to know what a function *returns*, which is a `returns` field in
the knowledge file (D122) - status, pointer, handle, or size. The file already carries
arity and argument meanings; this is the same kind of fact and the same home.

Worth noting the shape of the discovery: three titles faulted at three unrelated
addresses, and the common cause was only visible from the *whole* call list rather than
from the top of it. The worklist ranks by volume, which is right for finding walls and
wrong for finding this - a function called twice ended a run that a function called 1,201
times did not.


