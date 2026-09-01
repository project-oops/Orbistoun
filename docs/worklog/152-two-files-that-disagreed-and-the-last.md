# Two files that disagreed, and the last unexplained sweep result


**`wanted.txt` held 131 hashes something could already name.** The work list removed only
what a given run solved, and a named hash is never searched for again - so it could never
be solved again, so it stayed forever, against a header promising the opposite. `Service::is_named`
now answers from both sources (the registry as well as the database, which is why 131 and
not 116), and the rule moved into a pure `wanted_now` that can be tested without a corpus.
The test was confirmed to fail against the old rule before being kept. 3829 -> 3698, and the
two files now agree exactly (D243).

**`scePthreadMutexattrInit: Moved{slot:1}` was an illegal instruction.** Both sentinels
produced the identical fault address, which is the tell - nothing was computed from either.
An illegal instruction carries no address parameters, so the reporter fills the field with
the instruction pointer, and subtracting a sentinel from it is arithmetic over two unrelated
things. `FaultSite::TOUCHED` publishes which kinds mean what they look like, the worker
emits from that same list, and `Finding::Derailed` says what happened instead (D244).

**And the fix over-reached, which the live sweep caught.** It also disqualified runs that
reached fewer imports - correct in the axis sweep, wrong here, and it reclassified five
sound findings. Poisoning a region and getting less far means the poison broke the run;
planting a sentinel in a pointer and getting less far is what success looks like. The test
that pins this was first written asserting the opposite.

Every import of the live title now classifies definitely: 5 dereferenced, 1 derailed, 17
unmoved, **no `Moved` at all**. The out-parameter explanation stays ruled out, with nothing
ambiguous left in the evidence for it.

