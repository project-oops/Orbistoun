# 2026-09-02 - (/loop) Bulk port batch 3: the standard streams, real recursive locks, and the C++ runtime

Two carried-over items cleared and the C++ batch done. Eighteen entries, and the biggest thing
in it is a decision about what *not* to write.

```
documented   711 needed, 434 missing   ->   711 needed, 416 missing
```

## The standard streams, as data (carried over)

`_Stdout` and `_Stderr` are **objects holding a `FILE *`**, not functions, so nothing could fill
them lazily - there is no call to hook. New `orbistoun-libc/src/streams.rs` fills them once,
from the worker, after the data imports are published and before the guest is entered.

What goes in them is not an invented `FILE` layout: `fprintf` already routes a stream by asking
`orbistoun_fs::open::wrapped_descriptor` which descriptor it stands for, and `wrap_descriptor`
mints exactly that handle - the same mechanism `fdopen` uses. So the standard streams are two
wrapped descriptors, 1 and 2, and every existing path that writes to a wrapped stream works on
them unchanged.

This is a real behavioural gain rather than a box ticked: until now `fprintf(stdout, ...)` and
`fprintf(stderr, ...)` **both** fell through to the host's error stream, so a guest's output
could not be told from its diagnostics.

## The runtime's recursive locks, done properly (carried over)

`_Lockfilelock` / `_Unlockfilelock` / `_Locksyslock` / `_Unlocksyslock`, in
`orbistoun-libc/src/locks.rs`: owner thread, depth, and a condition variable. Batch 2 deferred
these rather than write the no-op, and the tests say why - one thread may take a lock twice and
must release it twice, and **a release by a thread that does not hold it is ignored**, because
obeying it would free a lock its real owner still believes it holds. A no-op implementation does
exactly that on every call, and looks perfectly correct until two threads share a `FILE`.

The two tables are kept separate so a `FILE *` that happens to equal a small lock index cannot
collide with it.

## The C++ runtime, and the line drawn through it (D473)

**Written:** the allocation forms `operator new(nothrow_t)`, `operator new(align_val_t)`,
`operator delete(align_val_t)`, and `std::get_new_handler` - which answers **null**, and that is
the correct answer rather than a placeholder: the standard's initial handler *is* a null
pointer, and nothing here imports `set_new_handler`.

**Stopped, deliberately:** `std::terminate`, `__cxa_pure_virtual`, `_Unwind_Resume`, and the
runtime's seven throw helpers. All are `[[noreturn]]`, and there is no unwinder. Returning would
resume the guest at an instruction its own compiler proved unreachable, holding the half-built
object that provoked the throw - a silent wrong continuation. Stopping cannot deliver the
exception to a `catch`, and says so, naming the exception and its message. One `stop_for` for all
of them, so none can quietly grow a `return`. D473 has the full argument.

**Left unimplemented:** `__cxa_throw` and its family, `__gxx_personality_v0`, the `_ZTV*` vtables,
`_ZSt4cout`/`_ZSt4cerr`, the locale and facet objects. The `__cxa_*` calls are not all
`[[noreturn]]`, so the stopping argument does not extend to them; the vtables and stream objects
are data needing real C++ object layouts, which is a different kind of work from writing a
function. An unimplemented import is already loud, which is the right state for these.

## State

clippy `--tests` clean, fmt clean, orbistoun-libc (114) and orbistoun-worker tests pass, identity
scan clean, nothing committed.

**Next**: batch 4 (stdio), batch 5 (the real `coshf`/`expf` math family, not the runtime's `_F*`
internals), batch 6 (conversions incl. `strtoumax`, plus a locale batch for
`wcstombs`/`wcsrtombs`). Still pending: `setenv`, and `_Thrd_id`/`_Thrd_join` which belong in
`orbistoun-kernel` beside the `_Mtx_*`/`_Cnd_*` family. Then the libScePosix 281, where the
`DELEGATED` table in `orbistoun-posix` should be checked first - many may already exist and need
only declaring.
