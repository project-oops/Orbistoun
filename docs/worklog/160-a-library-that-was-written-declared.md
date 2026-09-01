# A library that was written, declared, tested, and never registered


The conformance probe reported `sceUserServiceInitialize` and `sceUserServiceGetInitialUser`
both answering `0x7fff0001`, so it could not open a display. That is our own placeholder, so
the reading was "these are unimplemented" and the next step looked like choosing a user id -
which is exactly the decision the probe's own notes warn about, having watched another
project pick a value its own display code then rejected.

**Both have been implemented since D274.** The crate serves two libraries and `register`
handed over one of them, so every call resolved to a stub (D281).

The evidence took three commands and no guessing: the implementation is there and writes four
bytes through the caller's `int *`; `guest_module!` declares `libSceUserService` in a nested
module; `register` mentions `MODULE` once. Then `orbistoun-cli imports titles/obscene/eboot.bin`
confirmed the probe asks for both from that exact library.

### What made it invisible

There was already a test asserting every implementation is **declared**, and its comment
calls out both libraries by name - *"checking only the first would report the user service as
undeclared while it worked"*. Somebody had already thought about this crate having two
modules, in the place where two modules mattered.

Nothing checked declared against **registered**. The test now does, through `resolve` rather
than by counting modules, because a count is satisfied by registering the same one twice. It
was watched failing with the fix removed before being kept:

```
sceUserServiceInitialize is declared and implemented, but its library never reaches the registry
```

### The general point, which is worth more than the fix

`0x7fff0001` correctly says *nothing implemented this*. It cannot say **why**, and the two
whys want opposite work: one is a person writing code, the other is one line. A
function-shaped finding was not just under-specified here, it was **wrong about the cause** -
which is the strongest argument yet for the library-shaped findings the probe's handover
proposes.

Same shape as D275 one layer up, where `sceKernelReadTsc` was never declared at all and every
sleep therefore looked instantaneous.

