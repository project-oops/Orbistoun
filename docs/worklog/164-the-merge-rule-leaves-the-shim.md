# The merge rule leaves the shim


Recording a finding is a merge with rules - replace a field only when given, append lists
without duplicating, and **refuse** an entry that claims behaviour without saying how it is
known. All of it lived in `orbistoun-cli::cmd_learn`, which principle 13 says is the wrong
place: *"if a shim starts holding logic, the other two are already drifting."*

It was about to matter rather than merely being untidy. The loop earns entries by measurement
now, and a second caller would have meant a second copy of the merge rule - including the
refusal, which is the entire point of it.

`Record` and `KnowledgeFile::merge` are in `orbistoun-hle::knowledge` now, beside the types
they operate on. The shim keeps where the file is, reading it, writing it, and what to print.

**And it deleted a duplicate one day old.** `turn::promote` had grown its own `Learned` struct
and a one-variant `Oracle` mirroring the real vocabulary - written that way to avoid a
dependency on `orbistoun-hle`, which was the wrong trade. A near-copy of the **provenance
vocabulary** is the last thing this project should carry two of, given its whole job is to
make "where did this come from" checkable. `promote` returns a `Record`.

`merge` returns the faults rather than a `Result`: they are things to say to a person, and
only the caller knows whether it is a command rejecting input or a loop declining to record.

Three tests, and the two that matter are the second and third: a record claiming behaviour
with no oracle behind it **must not be admissible**, and a bare name claims nothing so it must
not be refused either.

