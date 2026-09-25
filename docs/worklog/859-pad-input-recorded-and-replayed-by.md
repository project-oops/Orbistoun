# 859. Pad input recorded and replayed by flips, and faults name their page's protection

**2026-09-25**. D721.

**Why.** Going in-game, Neverball crashed: a guest `memset` into freshly mapped direct memory
faulted 64 KB in (`write to 0x740007a60000 at image+0x29bc00`). The report could not tell a page
left guarded by D717/D719/D720 from a gap in direct-memory mapping. Reproducing it needed the menus
navigated, and a script could not reliably do that.

**Fault report.** After the fault line comes the faulting page's host state (committed, reserved or
free; mapped, private or image), its protection, and its region. Then comes every change
orbistoun's page guards made to it, from the last 256 changes, or "never changed this page".

- `page_guard::{guard, protect_writes, release}` note each change: range, protection asked for,
  whether the host agreed, and a sequence number.
- `changes_touching` reads the history with `try_lock` and no allocation, as a fault handler
  needs.
- Tests: `a_fault_report_names_a_page_guards_protection_and_history` and the extended guard test.
  The report test first failed on a real effect: the host reuses freed addresses, and another test
  had guarded the same range. It now checks "never changed" on its own stack.

**Input (D721):**

- **Two clocks.** A step names `at_ms` or `at_flip`, exactly one, and a script uses one
  throughout. `NoTime` and `MixedClocks` are refused like `OutOfOrder`. `at_flip` counts
  `orbistoun_video::flips_accepted`, installed into `orbistoun-input` as a function.
- **Recording.** A run no script drives records what the guest reads: a step per change, stamped
  with the flip, appended and flushed to `<logs>/input/<title>-<unix ms>.toml` as it happens. The
  run prints the path.
- **A run's own script.** `Request::Run::input_script` (serde-defaulted) and
  `orbistoun-cli run --input <file>` take precedence over `config.toml`.
  `./bin/orbistoun run <title> -- <args>` now passes its extra arguments through, as its usage
  said.
- **Per-title overrides were not used.** They are compatibility records that nothing applies to a
  run.
- **Tests:** flip steps and the one-clock rule; a recorded step replays as its state; a recording
  writes one step per change at its flip, and replays from a later flip against the same clock.

**Checked live:** a flip script took Neverball through two menu states, to the Player Name screen,
where pressing Cross types letters. The route in-game comes from a recording.

Also in this unit: a copy superseding another releases the old snapshot and keeps the new one in one
device-thread trip (`LazyCopies::replace`), where it took two.
