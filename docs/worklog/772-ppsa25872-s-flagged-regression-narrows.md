# 772. PPSA25872's flagged regression narrows to commit a8b2d74, and it is not the int 0x41 reclassification but a real path change into the error branch

**2026-09-21** — worklog 673 flagged a PPSA25872 (Terminator: Resistance) regression and left it
unchased ("spans weeks of changes"). Picking it up: the window is small, it narrows to one commit, and
the obvious suspect inside that commit is ruled out - which changes what the fix has to be.

## The regression, restated

- **2026-09-13:** PPSA25872 reached `192 imports / 339,539 calls`, faulting at `image+0x3b383b` - the
  AGC vertex attribute-marshalling loop worklog 529 advanced it into.
- **2026-09-17 (worklog 673) and still today:** `321,973` (today `321,970`) calls, faulting *earlier*
  at `image+0x17554a3` - an `int 0x41` Il2Cpp assert reached through an error-string format
  ("Invalid info address."/"Hostname Look…"). Stable across four days, so a real change, not run
  jitter.

## Narrowed to one commit

Only **8 commits** fall in `2026-09-13 .. 2026-09-17`, and only two touch a crate on this path.
`d666069` is GPU-only (GL cube, AGC packet/shader). **`a8b2d74`** (2026-09-17 08:53,
"agc builders and the scriptingGetMem/int-0x41 wall consolidation") is the one that also rewrites
`orbistoun-fs` heavily - `descriptor.rs` (+74), `lib.rs` (+211), `mount.rs` (+68), `metadata.rs`.

## The obvious suspect inside it is ruled out

`a8b2d74`'s headline is int 0x41 handling, so the tempting read is "it made int 0x41 fatal and that
stopped the run earlier." **The diff refutes that.** Before a8b2d74 orbistoun "had no interrupt handling
at all, so it caught the trap as an unhandled host exception and **stopped**" (worklog 603/605); after,
int 0x41 is *reclassified* from `KernelEntryUnimplemented` to a measured-fatal `Faulted` (obSCEne
`-b3c2`). Either way the run **halts** on int 0x41 - the change is to the verdict text, not to whether it
stops. So the fault-site move is **not** a reporting artefact: before a8b2d74 the guest did not execute
int 0x41 at `image+0x17554a3` at all (it ran on to the AGC fault at `0x3b383b`); after, it does. That is
a genuine **execution-path change** - Terminator now takes the error branch it previously stepped over.

## What that points the fix at

The remaining change in a8b2d74 that can move a guest's data-dependent path is its **filesystem
rewrite** (descriptor/mount/metadata/lib). The leading hypothesis: one of those FS changes - correct for
PPSA04263's `ENOENT`/rpf.cache handling, which is what the commit was for - alters a file or descriptor
result Terminator reads, so its Il2CPP layer now formats an "invalid info address" error and asserts.
Confirming it is one isolated run (build at `a8b2d74^`, run PPSA25872: `0x3b383b`/339k confirms, else
look wider); but the confirmation only names the trigger. **The fix is the error source, not a revert** -
a8b2d74's FS change is right for the title it was made for, and restoring the old behaviour would only
re-hide Terminator's error behind the same unhonest step it used to. So the real next move is to trace
what "invalid info address" is read from on this run and answer *that*, which is the same upstream-value
work worklog 673 named - now with the regression explained rather than open.

## Gate state

No code changed - a characterisation that spends worklog 673's open regression flag: narrows it to
a8b2d74, rules out the int 0x41 reclassification as the cause (the run halted on int 0x41 before and
after), and reframes it as a real path change whose fix is the error source, not a revert.
`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
