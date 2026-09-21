# 758. GTAV's rpf.cache was misrouted, not missing - serving it clears the int 0x41 wall

**2026-09-21** — PPSA04263 (Grand Theft Auto V) reached its first forward progress in weeks, by
fixing the thing three prior worklogs walked past: the `int 0x41` abort is a *downstream symptom* of
orbistoun misrouting a file open, and the file it could not find existed the whole time. This also
corrects a wrong intermediate draft of this same worklog (mine, earlier today), which claimed the
cache was a missing install artifact - it is not.

## The correction, and how the wrong version happened

An earlier pass concluded `rpf.cache` was "generated at install time and absent from the corpus."
That was wrong, and it was wrong for a mechanical reason worth naming: the search looked in the repo
`titles/` stub, not in the **resolved data directory** the run actually reads from. `rpf.cache` is
right there, at the title root, guest path **`/app0/rpf.cache`**. The operator only ships working
retail dumps, so a "missing file" was never the honest hypothesis - the file's absence had not been
checked against the place the run reads. This is now doctrine (D709), and `titles/README.md` documents
the two locations.

## What the guest actually does (measured)

`ORBISTOUN_DUMP=sceKernelOpen,sceKernelStat` shows the complete file picture:

- opens `/app0` (the directory) - fine;
- opens **and** stats **`/host//ap/rpf.cache`** (`arg1 = 0x0`, `O_RDONLY`) → `0x80020002` (ENOENT);
- **never opens `/app0/rpf.cache`**, where the file is.

So the guest routes its cache read to the `/host` device (worklog 603 measured the classification:
it holds `/ap/rpf.cache`, finds it is not under `/app0/`, and joins `/host/` + `/ap/rpf.cache`, the
doubled slash being the engine's naive join). Orbistoun served no `/host` mount, so the read failed.

The failed read is what the `int 0x41` abort is about. `ORBISTOUN_PEEK` at the fault (`image+0x196b91a`)
showed the assertion reads a `0xFF`-poisoned object whose vtable (`[rbx+0]` → a table of code pointers
in the file-loading region) is RAGE's own resource loader: the loader left the object invalid because
its cache load failed, and the next thing to inspect it aborted. obSCEne already measured `int 0x41`
as a fatal trap (REQ-b3c2), so the cause was always upstream (D699) - and the upstream cause is this
open, not "the title's il2cpp assertion" worklog 620 blamed (GTAV is RAGE, not Unity; that label was
wrong and the "faithful ENOENT" was never measured).

## The fix, and the confirmation

The console serves `/host//ap/rpf.cache` from the title's own storage, so orbistoun now does too:
`mount::mount_title` layers the title directory at `RAGE_HOST_APP_MOUNT` (`/host//ap`) as well as at
`/app0`, and the shipped `rpf.cache` answers the read with **no copied file**. A test pins that both
`/host//ap/rpf.cache` and `/app0/rpf.cache` resolve to the one shipped file.

Confirmed by running it, twice. A diagnostic first (the real cache staged where the guest looks)
proved causation; then the clean mount reproduced it byte for byte:

```text
before:  fault image+0x196b91a   71 imports   30262 calls   (the int 0x41 abort)
after:   fault image+0x19676d7   74 imports   30454 calls   (+3 imports, +192 calls)
```

The cache open now succeeds and GTAV runs 192 calls further into new code, hitting a **new** wall -
`scePthreadMutexUnlock` answers `0x0` and the guest dereferences it as a pointer (`read of 0x58` at
`image+0x19676d7`). Also orbistoun's, and the next target.

## Why this took weeks, recorded so it does not again

The `int 0x41` chase produced real, kept infrastructure - the fault reporter that reads heap objects,
`ORBISTOUN_PEEK`, the single-stepper, the call-before-the-fault taxonomy, a POSIX filesystem layer.
But all of it was spent explaining a downstream trap whose cause was one unchecked upstream fact: does
the file exist, and where does the run read it from. D709 makes the cheap check come first and bans the
"title's fault / devkit / faithful failure" exit that closed this wall wrongly for weeks.

## Gate state

Changed `crates/orbistoun-fs/src/mount.rs` (`RAGE_HOST_APP_MOUNT` const + the `mount_title` layer + a
test), `titles/README.md` (the two title locations), `docs/THE_LOOP.md` (the "title is not the bug"
rule), and added `docs/decisions/D709`. Diagnostics used: `ORBISTOUN_DUMP`, `ORBISTOUN_TRACE_OPENS`,
`ORBISTOUN_PEEK`. `./bin/orbistoun check` green, indexes regenerated, identity scan clean. No commit.
