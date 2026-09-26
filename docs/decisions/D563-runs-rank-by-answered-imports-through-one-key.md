# D563 - Runs rank by answered imports, through one key

**Status:** decided
**Date:** 2026-09-04

A run records `unanswered`, the distinct imports it called with nothing behind them, and ranks
by `answered` (imports minus unanswered) directly below imports. One `ranking_key` orders the
record, the frontier and the table; a record that predates the measurement holds `None` and ranks
as though nothing was answered.

**Why:** a percentage of calls follows whatever the guest loops on, so it hides work that removes
whole functions from the work list. A count of functions is stable against a hot loop and is the
work list itself. A guest that reaches further calls new imports, some unimplemented, so ranking
answered above imports would report progress as regression. Separate copies of the ordering
drift, and a table that disagrees with the record is the more convincing and the wrong one.

**Rejected:**
- More precision in `standing`: makes a loop-dominated number more exact, not more useful.
- `Some(0)` for old records: a stale record claims a perfect score and refuses honest runs.
- One tuple per consumer: they drifted as soon as a rung was added.
