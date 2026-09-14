# 543. Syscall 1 bound to nothing, because a rename named nothing

**2026-09-14** - one of the two unimplemented syscalls was already implemented

## What was actually wrong

The run report says two syscall numbers reach nothing: `1`, and `601`. Number 1 looked like a
missing function. It was not - `orbistoun-libc` has implemented `exit` for a long time, and the
syscall table is built by crossing harvested constants with implemented names. The crossing failed.

FreeBSD's entry 1 is the **raw `_exit`**, not the `exit(3)` wrapper, so the harvested constant is
`SYS__exit` and the name derived from it by stripping `SYS_` is `_exit`. Two things then had to line
up and neither did:

- `SPELT_DIFFERENTLY` in `orbistoun-service` carried `("SYS_exit", "exit")`. **No harvested constant
  is called `SYS_exit`**, so that entry matched nothing, ever. It read like a binding in review and
  was inert.
- `orbistoun-libc` registered `exit` and `_Exit`, but not `_exit`, so the derived name found nothing
  either.

The result: a guest calling `exit` at the end of a clean run got `ENOSYS` and carried on to the time
limit. `enter.rs` states the architecture plainly - *"there is nothing to return to. A program leaves
by calling exit"* - and the one call that does that was unreachable by its number.

## The fix, and the guard that matters more

`_exit` is now declared and registered in `orbistoun-libc`, sharing `exit`'s body because that body
already runs nothing on the way out, which is exactly `_exit`'s contract. The dead rename is gone and
the comment in its place says why no entry is needed.

The valuable part is the second test, not the first. Pinning `("_exit", 1)` beside `read`/`write`/
`open`/`close` catches *this* case. The new `every_rename_names_a_constant_that_exists` catches the
**class**: any `SPELT_DIFFERENTLY` key that matches no harvested constant. That failure is invisible
by construction - nothing breaks, a number quietly answers `ENOSYS` forever - so it needs a gate
rather than attention. Reviving `("SYS_exit", "exit")` makes it fail and name the entry.

## 601 is not implementable, and that is the finding

`601` is `SYS_pdwait` in the harvested FreeBSD numbering. That is **not** grounds to implement it.
`vendor-syscalls.toml` exists precisely for target numbers FreeBSD does not have, and sets the bar:
a guest must have been watched asking for the number, **and what it does with the answer described**.
One guest (`dist`, nine calls total) asked once with first argument `7`. The first half is met and
the second is not, so nothing was added - to the table or to the vendor file.

## The unrelated break this turned up

`every_implementation_resolves_through_the_registry_a_run_builds` failed on
**`sceAgcDcbSetCxRegisterDirect`** - one of the eight AGC builders wired in worklog 538. Two of those
eight were implemented but never **declared** in `guest_module!`, so they were dead: registered with
a body, invisible to resolution.

Worklog 538 reported eight builders "wired and dispatching". Six were. `tests/dcb_wiring.rs` did not
catch it because it drives `agc::implementations()` directly - one step short of what a guest goes
through - and because that work was verified with `-p orbistoun-gpu`, while the gate that knew lives
in `orbistoun-service`. **Scoping a check to the crate you edited hides exactly the failures that
cross a crate boundary**, which is the kind this was. Both are declared now.

## Surprises

**"Unimplemented" meant "unreachable" twice in one turn**, at two different layers, and neither was a
missing function: syscall 1 had an implementation and no name that reached it, and two AGC builders
had implementations and no declaration that reached them. Both were reported as absence. A report
that cannot tell "nothing implements this" from "nothing can reach what does" will send someone to
write code that already exists.

**Perl's `qq{}` counts braces.** Two attempts to generate Rust test code through it died on the
code's own braces before anything was written. Files and `awk` did it first try. Third self-inflicted
quoting wound of the session, all the same shape: generating code through a layer that parses it.

## Verified on the guest that calls it

Re-running `obscene-payload` with `_exit` bound, the prediction held. The payload's recorded outcome
was `0x5e2d` - a very small address, which is precisely what `enter.rs` says a program produces when
it falls off its entry point instead of leaving by `exit`. It now reports:

```
orbistoun: the guest called exit (0x0)
  fault    the guest called exit
```

and the report's list of syscalls nothing implements went from two to one - only `601` remains. The
payload stops deliberately, with status zero, where it used to fall off the end and fault.

## The verdict was BACK, and that is the interesting part

```
progress
  imports  223 distinct (-1), 432211 calls (-6)
  verdict  BACK     reaching less of the interface than it did
```

One fewer import, six fewer calls - **exactly the calls the payload used to make after its failed
`exit` returned `ENOSYS` and let it run on**. The guest now stops when it means to, and the progress
metric scores stopping correctly as reaching less.

That is not the metric malfunctioning; `FURTHER` measures interface reach and it measured it. It is
the metric answering a different question from the one a reader assumes. A guest that exits cleanly
at the end of its program has *succeeded*, and there is no rung on the ladder
(`Rejected → Parsed → Linked → Entered → Flipped`) that says so - `Flipped` is as good as it gets, and
a clean exit scores below a fault that happened later.

**And the run recorded nothing.** The earlier `PPSA02664` run this session wrote one
`recorded [status]` line; this one wrote none, and nothing in the output says why. Observed, not read
out of the code: a `BACK` verdict appears not to overwrite the record. So `compat/obscene-payload.toml`
still shows `outcome = "0x5e2d"`, dated four days ago - the pre-fix state, preserved because it scored
higher. The improvement is real, verified, and invisible in the file that is supposed to hold it.

Worth a decision on its own: either the ladder gains a rung for a guest that exited deliberately, or
the record gains a way to say "this run was better in a way the score does not capture". Not decided
here - it is a user-visible behaviour change and wants agreement, not an assumption.
