# D270 - The C library arrived as one batch because it was one absence


**decided** · 2026-08-25 · sixteen failures naming fifteen functions

`035-libc` failed sixteen checks and almost every one named a function that was simply not
there: `strstr`, `strspn`, `toupper`, `atol`, `strtoul`, `strncasecmp`, `wcslen`. They are
defined by the C standard, so there is one right answer per function and no room for a guess
- which is why they came as a batch rather than as sixteen decisions.

Three of them were **hangs** rather than failures: `strtok`, `strncat` and `strdup` did not
return, and the probe skipped them on later runs to get past. Implementing them cleared all
three.

The skips persisted across runs, which is worth noting on its own: the probe remembers what
hung, and it can only do that because `/data` became writable (D250). Clearing its report
made it retry.

Everything here works on bytes and the C locale, which is defined only for ASCII. A byte
above 127 is not a letter, and treating it as one is how a locale-dependent answer becomes a
wrong one on somebody else's machine.

