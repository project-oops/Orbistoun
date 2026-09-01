# D281 - Five crates export a registration nothing calls


**decided** · 2026-08-26 · a wrong diagnosis, and the right one underneath it

The conformance probe reported `sceUserServiceInitialize` and `sceUserServiceGetInitialUser`
both answering `0x7fff0001`, so it could not open a display. `orbistoun-systemservice`
serves two libraries - `libSceSystemService` and a nested `libSceUserService` - and its
`register` handed over only the first. That looked like the whole answer and **it was not**.

`register` is never called. Not by the worker, not by the service, not by any shim. Nor is
the equivalent in `orbistoun-audio`, `orbistoun-fs`, `orbistoun-gpu`, `orbistoun-input` or
`orbistoun-video`. The registry is built from `orbistoun_service::symbols::modules()`, a
hand-written array, and that array **already listed both** system-service modules. The user
service resolved correctly the whole time; the probe's observation predates the
implementation landing (D274).

So the finding is not a missing registration. It is that **five crates export a public
`register(&mut Registry)` that looks like how a library gets wired up and is not**, beside a
hand-written array in a different crate that actually is. A crate adding a module and
dutifully updating its own `register` changes nothing, and the change looks right in review.

That is not hypothetical. `orbistoun-systemservice::register` was **already wrong in exactly
that way** - one module of two - and it had cost nothing only because nothing calls it. The
trap was armed and the failure was waiting for whoever first made the dead function live.

**The correction is worth as much as the fix.** The wrong diagnosis was reached by reading
the code and stopping at the first thing that explained the symptom, and it survived a
plausible confirmation: the probe opened a display after the change. It would have opened one
without it. A dead function edited and a working path are indistinguishable from the outside,
which is the same failure principle 3 names one level down - reporting more than the
measurement supports.

**They are not merely dead, they are residue.** D123 records the design they belong to: the
service *used* to hand-call `register` per crate, `libc` was added to one list and not the
other, and the result was a function that `orbistoun-cli symbols` listed, that a trace named
correctly, and that resolved to nothing - *"every layer agreed except the one that mattered"*.
That second copy was replaced by `modules()`. The per-crate functions it called were left
behind, still public, still reading like the way a library gets wired up.

So: the dead functions go - all six of them - and the guard moves to where the registry is
really built. A test
in `orbistoun-service` resolves **every implemented name, integer and floating-point alike**,
through a registry assembled exactly as a run assembles it. That catches a module missing
from the array, and it catches an implementation whose declaration never reaches it - which
the per-crate test could not, because it was asking a registry no run ever sees.


