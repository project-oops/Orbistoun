# D715 - a worker whose control channel closes ends its process

**Status:** assumed
**Date:** 2026-09-24

## The question

A guest runs in a worker process (D032): the shim's own executable relaunched with `--worker`,
talking over stdin and stdout. On 2026-09-24 one of these workers was still running a guest about
an hour after its window had closed, with nobody left to report to. It also held the release
executable open, so the next build failed with "Access is denied". What should a worker do when
its parent goes away?

## The choice

**When the control channel (stdin) ends without a `Shutdown` before it, the worker ends its whole
process**, with exit status `EXIT_ORPHANED` (3) and a line on stderr saying why.

- The only peer is `WorkerHandle`. It holds the pipe open until it sends `Shutdown`, so an end
  without one means the parent exited, crashed or was killed. On Windows a read from a pipe whose
  writer is gone ends as EOF, like on every other platform.
- The worker already reads stdin on a thread of its own (D310), so it notices the end even while
  the main thread is inside guest code. `serve` takes an `on_hangup` callback, which that thread
  calls. The worker process passes one that exits. Tests pass one that records the call.
- **The process, not the run.** Guest code cannot be unwound from outside, and the stop button
  already works by ending the process (`Stopper`, D032). A run's trace is written as it goes, so
  nothing recorded so far is lost.

## Rejected

- **A Windows job object with kill-on-close.** It also covers a parent that is killed, but only
  on Windows, and it needs new `unsafe` code. The EOF signal covers the same cases (a killed
  parent's pipe handles close too) on every platform with no `unsafe`. It is still available if a
  case turns up that EOF misses, such as a grandchild holding the pipe open.
- **Letting the loop finish what it had queued first.** It cannot: the case that matters is a run
  that never returns.

## What it costs

A peer that closes stdin while it still wants answers, for example a script piping requests in and
reading the replies, loses them. `WorkerHandle` never does this, and nothing else drives a worker.
