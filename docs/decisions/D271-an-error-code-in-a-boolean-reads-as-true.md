# D271 - An error code in a boolean reads as true


**decided** · 2026-08-25 · three failures, one shape

`GuestError` placeholders deliberately avoid the high bit so they can never be mistaken for
an established firmware value. That choice makes them **small positive integers**, and a
small positive integer is what `true` looks like.

So an unimplemented boolean answered yes:

- `sceKernelIsCex` and `sceKernelIsDevKit` both answered non-zero, and the platform claimed
  to be a retail unit *and* a development kit at once.
- `posix_sigismember` answered yes for every signal, so a set that `sigemptyset` had just
  cleared reported every signal still in it - and the failure was attributed to the function
  that did the clearing.

This is D125 in a boolean: a value the caller reads as data rather than tests against a
table. It is the same defect as a failed `open` answering something that looks like a
descriptor (D273), and the same one the stub policy already solves for pointer-returning
functions by answering zero. **The knowledge base records a return kind, so the policy could
answer zero for a boolean too** - that is the general fix, and implementing each boolean by
hand is not it.

