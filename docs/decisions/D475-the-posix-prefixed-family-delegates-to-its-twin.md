# D475 - The `posix_`-prefixed exports delegate to their unprefixed twins, on an assumed footing

**assumed** - 2026-09-02 (user-directed bulk port, batch 8)

`libScePosix` exports a large family of names that are an existing POSIX function with `posix_`
in front: `posix_open`, `posix_close`, `posix_pthread_create`, `posix_mmap` and 138 more. They
are **142 of the 249** documented names still missing, so what is done with them decides most of
the remaining work.

## What is assumed, and it was already assumed

Sixty-nine of the 142 have an unprefixed twin this project already implements. Those are now
delegated to it. The claim being made is that `posix_X` behaves as `X`, and that claim is
**`assumed`**, not published - which is the footing the project had already taken for this
family before this batch. The knowledge file's existing entries say it in as many words:

> Semantics follow the POSIX analogue of the same name. Nothing on the target has confirmed
> them, and the error codes are placeholders rather than established values.

So this decision does not introduce the assumption; it acts on one already recorded, and says so
where the delegation lives.

**The unconfirmed part is the failure convention, not the behaviour.** What `posix_open` *does*
on success is not in doubt. Whether it reports failure as `-1` with `errno`, or as a negative
vendor code the way `sceKernelOpen` does, is not established here - and the two are
distinguishable only by a guest that inspects the value rather than its sign. That is the thing
to check first if a title behaves oddly around error paths, and it is why this is `assumed`
rather than `published`.

## Not the D385 trap

Batch 4 found that two vendor twins take a trailing **name** argument the POSIX form has no
place for, so delegating read a register the caller never set. That trap does not apply here:
`posix_X` and `X` are the same POSIX signature, and each arity was taken from the twin rather
than guessed. The arity check was run anyway and found nothing, which is the answer a check is
allowed to give.

## A layer of indirection that had to be followed

The delegation table maps a POSIX name to **the function that implements it**, and many of the
unprefixed twins are themselves aliases - `close` is a row pointing at `sceKernelClose`, not an
implementation. Pointing `posix_close` at `close` therefore named nothing, and the crate's own
guard said so immediately. Each row now resolves through the table to the implementation that
actually serves it.

That guard earned its place twice over: it caught the mistake, and then it could not say **which**
of 160 rows was broken. It names them now - the same "a message naming a cause must come from the
branch that determined it" the principles already require, applied to a test.

## The 73 not delegated

The other 73 prefixed names have no unprefixed twin implemented either, so there is nothing to
delegate to. They are ordinary future work rather than a separate problem, and they are counted
in the same gap as everything else.
