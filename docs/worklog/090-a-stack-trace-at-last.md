# A stack trace, at last


`sceSystemServiceParamGetInt` was the leading hypothesis for the wall. Implemented it -
`orbistoun-systemservice`, a new crate - and it was **wrong**. The call is reached, the
implementation is right, the wall is unchanged (D171).

Worth keeping anyway: unimplemented, it wrote nothing to its out-pointer and the guest read
whatever its stack held. That is a worse failure than a wrong return, and a distinct one -
every other unimplemented call here answers wrongly but *consistently*, in a recognisable
range. An unwritten out-pointer answers differently every run and leaves nothing to
recognise, because nothing was written.

Then the tool that actually helps. The fault handler already had `rbp`; walking the chain
gives the guest's own call path (D172):

```text
read of 0x0 while executing at image+0x43c4
  from image+0x4409 ... image+0xdbfb
  from image+0xaf        <- the entry point is 0x70
  from 0x7ff65a3732d2    <- host: enter_guest
```

The whole path from the guest's first instruction to the fault. It already says something
the address alone did not: this is **startup**, not game logic, and four frames cluster
within 0x1e0 bytes - one small group of functions calling into itself.

Care taken, because it runs in a fault handler on a thread that has already faulted: every
read is bounds-checked against the stack region *before* it happens, frames must be aligned
and the chain must climb, and it stops at twelve. An empty result is the ordinary case for
optimised code that omits the frame pointer, not a failure.

Third diagnostic added in two days - ordered call tail, register dump and import
attribution, now the frame chain - and each one was built because a specific bug could not
be seen without it.

