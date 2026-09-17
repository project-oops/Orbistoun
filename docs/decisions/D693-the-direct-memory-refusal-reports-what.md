# D693 - A refused direct-memory allocation reports what refused it

**assumed** - 2026-09-15

`sceKernelAllocateMainDirectMemory` answered `NoMemory` and said nothing else. That single value
covers at least three situations which want three different responses:

- the pool is genuinely full;
- the pool has the bytes but not in one span, because the guest fragmented it;
- the pool has a span, but the requested alignment pushes its start past the end of every region
  that could hold it.

A guest cannot tell these apart either, but a guest is not the audience - **we** are, and for
PPSA04263 the difference was the whole investigation. The refusal now writes the numbers that
decided it: the length, the alignment as asked and as widened, the largest placeable span, the
free total, and the region list.

This is CLAUDE.md principle 3's third rule applied literally - *a message naming a cause must come
from the branch that determined it* - to a branch that was naming no cause at all. It is the same
defect as `Further` firing for two reasons and reporting one, and it cost roughly an hour of
hypotheses (the map shape, the alignment, the assumed argument layout, a shrunken pool) that the
message would have refuted in a line.

## Why a print rather than a counter or a trace field

Considered and rejected:

- **A trace field.** The trace already records `memory_map`, so it looks like the natural home.
  It is not: that field is written by `record_conditions` *before the guest runs*, and records
  the map as constructed so a shape that fell back is not reported as the shape nobody got
  (D357). It is a statement about the run's starting conditions, and a refusal is an event
  during the run. Putting one in the other's field would break exactly the guarantee D357 bought.
  A per-event record would be a new trace section, which is worth doing when a second event wants
  one; today there is one event and one line.
- **A diagnostic environment variable.** `orbistoun-env` is where anything that *intervenes*
  belongs, and several things that only observe live there too. Rejected because a refusal is
  rare, already exceptional, and the person who needs the message is the person who did not know
  they would need it - a switch you must set in advance is no use on the run that surprised you.
- **Returning a richer error.** The return value is ABI. It says what the console says.

## What it costs

One line per refused allocation. A guest that probes for a size by allocating until it fails would
print once per probe, which is why the region list is bounded (`REFUSAL_REGIONS`, twelve) and says
how many it left out rather than truncating silently. A guest doing that in a loop would be noisy,
and if one ever does, the fix is to rate-limit this message, not to remove it.

## What would overturn this

Evidence that a title probes allocation sizes in a hot loop, making the message a cost rather than
a diagnostic. Nothing observed so far allocates more than a handful of times: PPSA04263, the
largest allocator in the corpus, calls the direct-memory allocators five times in twenty seconds.
