# 413. libSceUlt, and the quiet four gigabytes

**2026-09-04** - directed

## What was done

Implemented the five libSceUlt setup calls PPSA28061 makes - `sceUltInitialize`, both
`GetWorkAreaSize` functions and both constructors - after chasing why that title stops at 25
imports where its record claimed 47.

The chase found this:

```text
sceUltWaitingQueueResourcePoolGetWorkAreaSize(16, 16) -> 0x7fff0001
malloc(0x7fff0001)                                    -> a pointer
sceUltUlthreadRuntimeGetWorkAreaSize(16, 3)           -> 0x7fff0001
malloc(0x7fff0001)                                    -> a pointer
```

`0x7fff_0001` is the unimplemented placeholder. **As a size it is 2 GiB, and this emulator's
`malloc` served both requests.** Four gigabytes, and nothing said a word.

Sizes are now **4,352 and 2,688 bytes**, both constructors succeed, and the runtime and pool are
in a table with the guest's own names for them - `"sample runtime"` and `"waiting queue"`.

## The rule this sharpens

The placeholder exists to be **loud**: no vendor function returns it, so a guest acting on it goes
visibly wrong. **That reasoning assumes the return is a status.** Where a function answers a
*size*, the placeholder is a number the caller spends - and spending it succeeds.

D125 already covers the placeholder answered where a **pointer** was wanted. This is the same
mistake one type over and it is quieter, because nothing crashes. The general form is worth
having: **a function whose answer is arithmetic - a size, a count, a length, an offset - cannot be
left unimplemented safely.**

## What it did not buy

**No reach.** PPSA28061 still stops at 25 imports, because the abort is Agc's: it goes on to
`sceAgcDriverRegisterDefaultOwner` and `sceAgcCreateShader`, both answer the placeholder, and it
calls `abort`. That was visible before implementing any of this and is unchanged by it - said
here plainly because a worklog that led with "five functions implemented" would imply otherwise.

## Guards

Six, each watched failing: the sizer answering the placeholder again (the original bug); the sizer
answering zero; the sizer ignoring the request; the constructor writing nothing into the object;
the constructor inventing a handle instead of issuing one; and a null destination accepted.

**One break needed two edits to fire, and that is in the test.** Removing the `out == 0` check
alone does not fail the null guard, because `write_word` refuses null too. The two are not the
same guarantee - one says *this call refuses null*, the other says *this process will not write
through null* - so both are kept and the test says why.

## Surprise

**A gate I had not met before caught me**: `every_implementation_is_also_declared_here_or_says_why_not`.
The Ult functions are declared in the `ult` module and implemented beside libkernel's table, which
needs an entry in that gate's exception list. The existing Ult mutexes were already there, so the
pattern was written down - I just had not added to it.
