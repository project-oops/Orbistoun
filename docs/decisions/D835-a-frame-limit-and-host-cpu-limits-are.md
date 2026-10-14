# D835 - A frame limit and host CPU limits are performance settings, global and per title

**Status:** assumed
**Date:** 2026-10-14

`config.toml`'s `[performance]` section holds three settings a run applies before its first guest
thread starts, and a title's own settings (its compat record or the user's override file, D048)
replace each for its runs:

| `config.toml` | title setting | does |
|---|---|---|
| `frame-limit` | `frame_limit` | the most flips a second; a flip waits on its own thread until it is due. None at zero |
| `host-cores` | `host_cores` | how many of the host's cores the whole process may use, the first that many it is allowed; all at zero |
| `host-priority` | `host_priority` | the process's priority among the host's programs: `idle`, `below-normal`, `normal`, `above-normal` |

The limiter paces flips at one interval after the last was due rather than after it arrived, so
one late flip does not push every later one back, and one more than an interval late starts the
count again. A host limit the host refuses is reported and the run goes on. The window's
preferences edit the three in a performance pane; the per-title editor's template names the keys.

**Why:** a title that flips freely runs as fast as the host allows - MDEM00001 at 280 fps - which
is no display's rate, burns the host for nothing a player sees, and makes one title's speed depend
on the machine. A run left playing beside other work should be able to stay out of its way. Both
are a player's choices about a title, not facts about it, so they are settings, keyed by what they
do and never by which title asks (D048); orbistoun itself names no title.

**Rejected:**
- Pacing to the logical clock's 60 Hz (D786): the logical clock is what the guest is told, not how
  fast the host goes, and a title on the host clock would not be paced at all.
- Applying the guest's own thread priorities to host threads: still recorded only (`apply_priority`),
  since raising a host thread on a guest's word can starve the emulator's own threads.
- A render resolution setting with these: the host renders at the guest's target size because the
  frame is written back into guest memory at that size; another size needs a resample at
  write-back, which is its own decision.
