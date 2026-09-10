# 456. Advice that cannot succeed

**2026-09-08** - directed, continuing 455

Bus idle a fifth pass: six requests open, two mine, none claimed, no sweep since 17:04.

## The report was sending readers somewhere with no answer

Every unnamed import got one sentence: *extend the candidate vocabulary and re-run the name
search.* Right for a vendor library, where the vocabulary is short and the hash is the oracle.
Useless for `PS5Util`, which is a file the game ships - its exports were written by whoever wrote
the game, and no vendor word list will ever hold them.

**Six of the seven unnamed imports in the corpus are that kind**, including the busiest call this
project has ever recorded (D630).

The loader already indexes what the title's placed modules export, by library; those names are now
published to the reporter and the finding branches on them (D631):

```text
! PS5Util::0xf948d02a4f9f5ace was called 19689015 times and has no name
    PS5Util is a module this title ships, so the hash is the game's own symbol
  -> do not search a vendor vocabulary for this - it is the title's own code, and the module
     exporting it is already placed and started, so what the guest wants is its export
```

Empty means *nobody said*, not *not the game's*: a run whose loader reported nothing falls back to
the vocabulary advice, the same rule `name_is_libkernel` follows for the same reason (D629).

Both branches were run against real guests - PPSA25872 for the shipped case, PPSA02664 for the
vendor one - and both are pinned by one test asserting *both*, because a version giving the new
advice to everything would satisfy half of it.

## Surprises

- **The second PS5Util import is handed a pointer to the game's own code.** `arg0` reads
  `the title's own modules+0x11e760 = 55 48 89 e5 41 57 41 56 41 54 53 …` - a function prologue.
  So the guest passes its own module's functions into calls orbistoun answers with a placeholder.
  Found by reading a finding that had just stopped giving the wrong advice.
- **`LinkedTitle::exports()` already indexes every placed module's exports by library and hash.**
  The index the six-for-six mismatch in D630 would need already exists; what does not exist is the
  resolution step that consults it.

## Next

- The six title-module imports (D630), still the largest wall measured anywhere, still waiting on
  whether loader work is in scope for this session.
- The unapplied relocation behind `obs_sink_open` (D628) - the same class, one layer down.
- The declined-syscall casualties, waiting on `REQ-20260908T1620Z-4c1e`.
- `sceAgcCreateShader`, waiting on `REQ-20260908T1621Z-7a5d`.
