# D637 - An answer needs somebody who asked

**Status:** measured
**Date:** 2026-09-09

## A finding I added, at the head of a list it had no business being on

D625 introduced `Gap::Captured` and printed it **ahead of the ranked six**, on the argument that a
finding which exists because somebody typed a variable is not competing with findings the tool
volunteered. That argument is right and the implementation did not honour it.

An ordinary run of the conformance eboot - no diagnostics, nothing asked for - now opened with:

```text
what to do about it
  ! libc::acos was asked about, and here is what it was passed
      12 captured argument value(s), listed below
  ! libc::asin was asked about, and here is what it was passed
```

Nobody asked about either. The real finding - `libScePad::scePadReadState` called 180 times with
nothing implementing it - was pushed down the page by two functions that work.

## Why those two, and why it was invisible

`captured` filtered on *having a dump*, and dumps are taken for every import nothing implements as
well as for anything forced. So the question is which imports have a dump and **no** unimplemented
finding to claim it, and the answer is a category nobody had thought about:

The dump condition tests `handler.is_none()` - the **integer** handler. A function answering in
`xmm0` is bound to the *float* table (D268), so its integer handler is `None`, so it is dumped by
default - and it is implemented, so `unimplemented` produces no finding for it. Exactly the maths
library, exactly `acos` and `asin`.

Two tables that are disjoint by construction, and one condition that only knows about one of them.

## The forced list is now published, not just counted

`arm_dumps` already computed which slots a run named; it printed the count and threw the labels
away. They are recorded on the trace now, and `captured` fires only for them.

`ORBISTOUN_DUMP=sceKernelWrite` still produces exactly one captured finding; an ordinary run
produces none, and `scePadReadState` is back at the top where it belongs.

## Both directions, tested

A version that emitted nothing at all would satisfy "no noise in an ordinary run" perfectly - and
emitting nothing is precisely what this finding did for its entire first day (D625). So the test
asserts both: a dump taken by the default rule produces no `Captured` finding, and an import
somebody named still produces one.

## The thing worth keeping

This is mine, made today, in the same session as eight records about instruments that could only
say one of the two things they appeared to say. It was found by running a guest for an unrelated
reason and reading the top of its report.

The lesson is not "be more careful". It is that **the report is the instrument that checks the
other instruments**, and the only way to use it is to keep reading ordinary runs of guests nobody
is currently investigating.
