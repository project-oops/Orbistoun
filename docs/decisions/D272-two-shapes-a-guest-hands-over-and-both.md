# D272 - Two shapes a guest hands over, and both were got wrong


**decided** · 2026-08-25 · one hang and ten million wasted calls

### A pointer to a pointer is not the object

`scePthreadMutexattrInit` takes `&attr` where the guest declared `ScePthreadMutexattr attr =
NULL`. It must **allocate** and write the address back - the same model the mutexes already
used. Treating the double pointer as the object meant `Settype` overwrote the guest's own
pointer variable with a type value.

### An `int` out-parameter is four bytes

Fixing that produced a worse failure. `scePthreadMutexattrGettype` writes through `int *`,
and writing a whole word took the caller's neighbouring stack variable with it - which was
the **loop counter**, so every iteration reset it and the check ran until the call budget
stopped it at twenty million calls.

This crate already carried a type to prevent exactly that: `SemaphoreHandle` is an `i32`
because "a mutex is a `void *` and a semaphore is an `int` written through an out-pointer:
four bytes, not eight" (D210). The lesson had been learned and written down and the code
still did it again, in a different function, because the knowledge lived in a type that this
call did not use.

There is now a `write_int` beside `write_word`, so the next one is a function call rather
than a decision.

**Both bugs were caught by the run stopping at a deterministic twenty million calls** rather
than a wall clock (D238). A time limit would have made the same hang look like a slow
machine.

