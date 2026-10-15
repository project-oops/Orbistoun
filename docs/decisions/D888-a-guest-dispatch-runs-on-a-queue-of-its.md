# D888 - A guest dispatch runs on a queue of its own

**Status:** measured
**Date:** 2026-10-15

**Amends:** D805 - a guest dispatch settled every draw queued on the device before it ran.

Where the device has one, a guest dispatch runs on a queue apart from the one draws are recorded to:
the first queue of a compute-only family, else a second queue of the draws' family. Every dispatch
runs there, so the buffers, images and pools dispatches share are only ever used by that family. A
guest dispatch on a queue of its own no longer settles the device first. A device with one queue in
its family runs dispatches on the draws' queue and settles first, as before.

A dispatch needs nothing a queued draw produces on the device: its window, images and buffers are
uploaded by the host for it, its results are read back to the host, and what a draw writes on the
device reaches guest memory only through a write-back the command processor has already made. On
the draws' queue it waited for every draw ahead of it - the settle first, and then its fence, which
the queue orders behind them. The console's compute work runs on queues of its own beside the
graphics ring.

**Why:** CRFT00001 waited on the device for a third of its run while the device worked a fifth of
it; its three dispatches a frame each waited out the frame's queued draws. `orbistoun-cli ab`
against d4801ab7, three rounds of 30 s, beside another program keeping the GPU 70-98% busy: CRFT00001
8.7 -> 14.1 fps (every run of the change above every run of the baseline), STKT00001 27.8 -> 36.1,
MCUB00001 67.6 against 67.4; `device: setup: settle` went to nothing in each, and every flip the
baselines repeated was drawn alike. A second queue of the graphics family alone moved the wait into
the dispatch's fence (CRFT00001 `device: run: wait` 1.8% -> 12.2%, its rate unmoved).

On an idle GPU, against 0271b972, three rounds of 30 s: CRFT00001 31.6 -> 34.4 fps, MCUB00001
137.5 -> 141.3, STKT00001 98.9 -> 90.7 with its median frame time unmoved (7.2 -> 7.1 ms; two of its
runs read 84 and 86); `device: setup: settle` went to nothing in each and the dispatch's own wait
grew by a tenth of it, every repeated flip drawn alike.

**Known limits:** the gain is largest beside other GPU work, where dispatches no longer queue behind
draws it slows; on an idle GPU it is the settle alone, a few percent of a dispatch-heavy title.
