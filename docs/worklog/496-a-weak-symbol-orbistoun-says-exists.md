# 496. A weak symbol orbistoun says exists

**2026-09-10** - loop, continuing 495

Went after `900-surface/control`, the last outright failure. Diagnosed it precisely and
**could not fix it**: the change belongs in `crates/orbistoun-loader/src/`, which this session's
permissions deny. Took the mouse instead, which is reachable and was also measured.

## The control, and why it fails

`900-surface/control` is obSCEne's own sanity check on its census. It takes the address of two
symbols it declares - one it defines, one it never does - and asserts the first reads present
and the second absent. orbistoun reports the second **present**, so the check fails with *"a
symbol that does not exist reported present; every count in this section is meaningless"*.

The undefined one is **weak**. The ELF gABI is unambiguous: a weak undefined symbol resolves to
zero and linking succeeds. That is what a console does and what obSCEne's control relies on.

orbistoun does neither branch of it:

- by default every unresolved import gets a stub so a call is reported, which makes the weak
  symbol read as **present**;
- under `ORBISTOUN_RESOLVE=named` it refuses the two it cannot name and then **will not enter**
  the image at all - *"3 unresolved ... not entered - the image is not fully linked"* (D010).

Neither is the ELF rule. The fix is to read the binding - the high nibble of `st_info`, which
`imports_from_symbols` currently discards on purpose - and bind a weak undefined symbol to zero
without counting it unresolved.

**This is the D670 class again**, one layer down: orbistoun answering in a way that reads as
*yes* to a guest that checks. A guest doing feature detection by weak symbol - a very ordinary
pattern - takes the "this platform has X" path on every one.

`crates/orbistoun-loader/src/relocate.rs:310` is where `tally.unresolved += 1` happens and where
the weak case belongs. That directory is denied to this session, so the diagnosis is written
down and the change is not made.

## The mouse, which was reachable

`101-input-ext/mouse-read` passes now. The console opens a mouse with nothing plugged in
(`0xc70700`), and a read answers `0x0` having written **zero bytes** - an empty queue is a
report, not a refusal. orbistoun refused the open and answered a placeholder (D673).

Divergences against the matching hardware leg: **five to four**. orbistoun passes 175 against
hardware's 167.

## Surprises

**The D668 guard caught its thirteenth file.** Writing `libSceMouse.toml` without registering it
in `EMBEDDED` failed `every_knowledge_file_on_disk_is_embedded` on the first run, by name. That
test exists because twelve files had already accumulated that no build loaded; this one never
joined them.

**Hardware fails two of the checks I was about to chase.** `101-input-ext/keyboard-presence` and
`mouse-presence` fail on the console too - *"libSceKeyboard loaded but no symbols resolved"* -
and `kbd-read` **skips** there while orbistoun reports `partial`. So the keyboard is not a gap;
checking which side each check falls on before working on it saved the whole errand.

## What is left, and what is blocked

Four checks where hardware passes and orbistoun does not:

- `900-surface/control` - diagnosed above, **blocked on the loader directory**
- `111-modlink/walk` - skip, needs `DT_DEBUG`, and probably the same directory
- `080-video/visual-flip` - skip, no scanout
- `130-layout/system-software-version` - partial, not looked at

No sibling has answered any of the five requests to obSCEne or the two to Prosperous.

- `category::present` is still built and uncalled, four turns running.

## And the flake was real

`./bin/orbistoun check` failed on `cargo test --workspace` twice, on two turns, while every
direct re-run passed. The first time I recorded it as not reproducible and flagged it; the
second time was the signal.

`ISSUED.store` in the pad and mouse shims is a plain store on a high-water mark whose allocator
is behind a lock. Two threads opening at once can have the second allocate the higher number and
the first store its lower one after, leaving a live handle **above the mark and refused by every
call that takes it**. `fetch_max` (D674).

Not only a test problem - the tests running in parallel are just what made it visible. Two guest
threads opening pads at once hit the same window, and there the failure is a title told its
controller does not exist, intermittently, with nothing in the trace to explain it.

**My own `tail -6` on the gate had thrown the failure detail away both times.** A pipe that trims
a gate's output trims the part you need; the diagnosis came from reading the code, not from the
capture.

## Next

- `130-layout/system-software-version` is the only one of the four not blocked and not a
  subsystem orbistoun lacks outright.
- `category::present` is still built and uncalled, four turns running.
