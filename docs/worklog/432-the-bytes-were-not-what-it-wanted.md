# 432. The bytes were not what it wanted

**2026-09-08** - directed, continuing 431

## What was done

**Built the delivery experiment and got a clean negative** (D589). `ORBISTOUN_APR_DELIVER` reads
the file the asynchronous path resolved into the buffer its command header names - 224,748 bytes
of `globalgamemanagers` into `0x740009200000` - and the guest does not care.

| | distinct imports |
|---|--:|
| baseline | 193, 193, 193 |
| delivering | 192, 192, 193 |

Identical fault in every run. The first delivering run said `FURTHER` and that was the
192-to-193 drift landing the convenient way round.

## Two traps, both caught by apparatus built earlier this week

- **A single run said FURTHER.** Three runs each said it did not. `Step::CheckRepeats` measures
  exactly this drift, and the habit of running three is what it bought.
- **The caveat did not print.** `APR_DELIVER` was declared `Effect::Intervenes` and *not*
  carried in `Experiments`, so the run reported `same` with nothing saying it was propped.
  Declaring the effect is half the job; the conditions record is the half that gets forgotten.

## What is now closed and what replaces it

"The guest needs the bytes" was a sentence in worklog 431 that would have been repeated until
somebody tried it. It is a measurement now, and the next reading has to explain why the bytes did
not help.

`libSceAmpr` exports `sceAmprCommandBufferWriteKernelEventQueueOnCompletion`, which says
completion is reported through an **event queue** rather than a return value. That is the obvious
next reading and nothing here has tested it.

## Surprises

- **Forcing all three Apr calls to success also changes nothing** - measured before the delivery
  experiment, and it is the same shape of answer. The guest logs its complaint and carries on.
  So the Apr path *fails* and making it succeed does not help, which is a different statement
  from "the Apr path is the wall".
- **The title prints `TODO:` for its own `LocalFileSystemPS5::Enumerate`.** That is Unity's
  dev-build marker in a shipped binary, and it sits between the Apr wait and the fault. The
  fallback reading - Apr fails, the title falls back to an enumeration it never finished
  writing - is still the one that fits and is still unmeasured.

## Next

- The event-queue completion path, which the export list points at.
- Why the guest calls neither `sceAmprAprCommandBufferConstructor` nor
  `sceAmprAprCommandBufferReadFile` while importing both.
- The mapping count doubling when handles clustered, carried over from 430 and still untouched.
