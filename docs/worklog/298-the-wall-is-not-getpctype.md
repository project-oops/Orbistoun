# 2026-09-02 - (/loop) The `_Getpctype` wall is a mislabel: measured, and it reframes four decisions

Chased the puzzle worklog 297 left open - implementation bound and called, guest still faulting - and
the answer is that **the report has been naming the wrong function** (D469). No code changed; every
probe was removed and `dispatch.rs` and `service/lib.rs` are byte-identical to HEAD (`git diff --stat`).

**Killed the leading hypothesis first.** The two-routes theory (import slot vs `dlsym`) is wrong:
PPSA02664 makes **zero** `dlsym` lookups. `dlsym` already logs every distinct name asked, so no
instrumentation was needed - the log is simply empty. The ~141k `sceKernelDlsym` calls that made the
theory attractive come from the "434 imports across **65 runs**" aggregate over the whole corpus. I
had read a corpus-wide number as a per-title one.

**Then measured what actually answers the placeholder.** Two probes - one on the stub path
(`handler.is_none()`), one on the single return point (`answer == 0x7fff_0001`):

- every index that answers a placeholder in a run: **154, 148, 86, 149, 100, 160, 161, 159, 158, 156,
  233, 2** - and **54 is not among them**;
- index 54 is `libc::_Getpctype`, it is bound, and its implementation runs **415 times**, each
  answering a real table pointer;
- the last placeholder before the fault is **index 148, three times: `Il2CppUserAssemblies::setenv`**;
- the symbol has two label slots - 54 `libc::_Getpctype` and 853 `resolved::_Getpctype` - and neither
  answers a placeholder.

Yet the report still prints `libc::_Getpctype was called 1 times and nothing implements it`, and pairs
the fault with it. The dispatcher's own `is_implemented(54)` is true, so **the trace disagrees with
itself**, not with reality.

**Why this matters more than the ctype table did.** D443, D449, D450 and D459 all name `_Getpctype` as
one of this title's two walls, and D450 builds a thread-race model on that pairing. The fault *sites*
were measured and stand; the *name* attached to `image+0xb14be3` does not. D450's prediction - that
implementing `_Getpctype` would push every run onto the allocator - is untested, not confirmed: the
wall did not move at all, six runs in six.

This is principle 3 one level up, again: a report that says "nothing implements it" about a function
that ran 415 times is reporting more than its measurement supports - the same shape as the five tools
that section already lists. I deliberately did **not** name the mechanism. Whether the fault is paired
with the wrong recorded call, or the label table is indexed wrongly, is the next question, and
guessing it here would repeat the very mistake being recorded.

**A method note worth keeping.** The third probe - logging *every* dispatched call - was abandoned
rather than trusted: 10,886 `eprintln!`s against a 20-second limit is a sink that changes the program
it observes (principle 9), and its "last call" was a time-limit artefact, not the pre-fault call. The
two cheap probes that only fire on the rare path were the ones that answered.

**Next**, in order: (1) find why the trace pairs `0xb14be3` with index 54 when no placeholder came from
54 - start at how the fault picks the call it prints and how `labels()` is indexed against the ring
buffer (D459 added the return column; D366 covers label/binding sharing one list). (2) `setenv` is
unimplemented and declared nowhere - `getenv` exists, `setenv` was never added - and is called three
times immediately before the fault. Implementing it is worth doing, but it is an **intervention, not a
diagnosis** (D224/226/227), and the value dereferenced at `0xb14be3` has only been shown *not* to come
from `_Getpctype`, not traced to its producer.

Nothing committed (inside the no-commit window). The ctype work from worklog 297 stands unchanged and
is still correct; it simply was not the wall.
