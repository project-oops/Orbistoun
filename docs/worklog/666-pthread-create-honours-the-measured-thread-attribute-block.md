# 666. `pthread_create` honours the measured thread-attribute block

**2026-09-17** — inbox `-5a3c`: `scePthreadCreate` ignored its attribute argument, so 125 thread
creations across four commercial titles all ran on the 8 MiB default stack with default affinity,
out of blocks in which the guests had asked for something else. obSCEne measured the block's layout
(`031-stackattr`, REQ-...c2e9), so the fields it carries are now read and honoured where honouring
them is what the console does.

## What `pthread_create` now reads

`pthread_create` (`lib.rs`) read `args[0/2/3/4]` and never touched `args[1]`, the attribute. It now
decodes it through a new pure `spawn_parameters` (tested) behind a thin `thread_attributes` reader:

- **stack size** — a size the guest set is passed to `thread::spawn` and reserved. obSCEne created a
  thread from a block asking for `0x181000` and it ran on exactly `0x181000` (observed-stacksize
  `0x181000`). A block nothing set carries the fresh default (`DEFAULT_ATTR_STACK_SIZE`, 64 KiB) and
  POSIX's `0` means "use the default"; neither is honoured, because a fresh attribute is not measured
  to bound a real thread and 64 KiB is 128× below orbistoun's 8 MiB `DEFAULT_STACK_SIZE` — shrinking
  a working thread to it would invent an overflow the console does not have. `spawn` caps the size to
  what fits a stack slot (`THREAD_STACK_SPACING`, 64 MiB, less its guard pages).
- **affinity** — the mask flows into the thread record, so a thread created from a block reads its
  affinity back through `scePthreadAttrGet`. `scePthreadAttrSetaffinity` already stored it; the value
  now reaches the record instead of stopping at the block ("stored, not applied", unchanged — nothing
  pins a guest thread to a host core).

Two fields are deliberately **not** acted on, each with its reason in the code:

- **detach state** — the same treatment `pthread_detach` already gives: every guest thread is a host
  thread the runtime reclaims when its body returns, so the detach promise is kept by construction.
- **priority** — obSCEne measured `scePthreadAttrSetschedparam` refusing on a retail attribute
  (`0x8002002d`), so the console does not carry priority in the block; it arrives via
  `scePthreadSetprio`. Left unread.

## Why orbistoun's own offsets, not the console's

obSCEne's layout is `stacksize@32`, `detachstate@16`, `affinity@49`. orbistoun does not use it:
`pthread_attr_init` substitutes its own 16-word attribute object and intercepts every setter, so the
values live at orbistoun's own field constants (`ATTR_STACK_SIZE=0`, `ATTR_AFFINITY=40`). The c2e9
measurement is the *semantic* oracle — which fields a thread honours, and that priority is not among
them — not a byte layout orbistoun reads. `spawn_parameters` is unit-tested with the fresh-attribute
negative, so a passthrough of every value cannot pass it.

## Docs corrected

Both `docs/PROJECT_STATUS.md` and `docs/roadmap/013-phase-5-threading-and-synchronisation.md` said
`scePthreadCreate` had never been called by a guest, and PROJECT_STATUS drew a conclusion from it
("the three current walls are phase 4 completion problems, not threading ones") that was being
planned against. Both now carry the per-title counts (PPSA02664 33, PPSA03416 33, PPSA25872 44,
PPSA04263 15 = 125), and PROJECT_STATUS no longer settles the walls' nature by an absence of threads
that does not exist. `grep 'never been called by'` over both prints nothing.

## Gate state

`cargo clippy -p orbistoun-kernel --all-targets` clean; `orbistoun-kernel` 115 + suites pass
(including the new `spawn_parameters` test); fmt clean; prose exit 0; identity scan clean;
`orbistoun-service` builds (the only `thread::spawn` caller is `pthread_create`, in-crate). **Pre-
existing, not mine:** `orbistoun-cli status --check` reports the generated number blocks in README.md
and PROJECT_STATUS.md stale versus `compat/` — confirmed by stashing this change and re-running (it
fails identically, and it flags README.md, which this unit never touched). That is another session's
`compat/` regeneration to land, not a `-5a3c` edit; not regenerated here to avoid folding an
unrelated data change in. No commit.
