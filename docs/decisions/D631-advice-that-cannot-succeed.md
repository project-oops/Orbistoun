# D631 - Advice that cannot succeed

**Status:** measured
**Date:** 2026-09-08

## One sentence, given to every unnamed hash

Every import still known only by a hash produced the same finding and the same instruction:

```text
! PS5Util::0xf948d02a4f9f5ace was called 19689015 times and has no name
    the hash resolved to no name in the symbol database
  -> extend the candidate vocabulary and re-run the name search …
```

For a vendor library that is exactly right - the vocabulary is short, the hash is the oracle, and
`names` is the command. For `PS5Util` it is an afternoon spent on something with no answer.
`PS5Util.prx` is a file the **game ships** (D630); its exports were written by whoever wrote the
game, and no vendor word list will ever contain them however long the search runs.

**Six of the seven unnamed imports in the corpus are that kind**, including the single busiest
call this project has ever recorded.

## The advice now depends on who wrote the symbol

The loader already indexes what the title's placed modules export, by library. Those library names
are published to the reporter, and the finding branches on them:

```text
! PS5Util::0xf948d02a4f9f5ace was called 19689015 times and has no name
    PS5Util is a module this title ships, so the hash is the game's own symbol
  -> do not search a vendor vocabulary for this - it is the title's own code, and the module
     exporting it is already placed and started, so what the guest wants is its export rather
     than a name

! libSceAgc::0x53bbd82b51d172db was called 1 times and has no name
    the hash resolved to no name in the symbol database
  -> extend the candidate vocabulary and re-run the name search …
```

**Empty means "nobody said", not "not the game's".** A run whose loader reported nothing, and a
guest that ships no modules of its own, both fall back to the vocabulary advice - the same rule
`name_is_libkernel` follows one crate over (D629), and for the same reason: silence is not a
denial.

## Watched failing, both ways

Both branches were run against real guests before this was written - PPSA25872 for the shipped
case, PPSA02664 for the vendor one - and both are pinned by a test that asserts *both*, because a
version handing the new advice to everything would satisfy the first assertion alone.

## What it turned up on the way

The second PS5Util import is passed `arg0 = 0x48000011e760`, which the report names as
`the title's own modules+0x11e760` and shows as `55 48 89 e5 41 57 41 56 41 54 53 …` - a function
prologue. So the guest is handing pointers to its own module's code into calls that orbistoun
answers with a placeholder. More evidence for D630, obtained by reading a finding that had just
stopped giving the wrong advice.
