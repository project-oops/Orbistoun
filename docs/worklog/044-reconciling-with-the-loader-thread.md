# Reconciling with the loader thread


Three things came back from the loader side and all three landed here.

**A guest virtual address is the host address, and a GPU virtual address is still
unknown.** Half of the open question, and the half that was cheap to answer. Everything
in the pipeline reads a shader at an address a hardware register named, through a trait
that reads the guest's address space - two address spaces treated as one. That assumption
is now `guest_address_of`, the identity function, with the reasoning attached and the
failure message naming it as the thing to suspect first. One edit when the answer
arrives, rather than a search. Not a trait: there is no second implementation and
inventing a seam for a hypothetical would be speculation.

**Decision numbers collided.** Both threads independently reached D118, D119 and D120 in
the same file. Theirs were already published, so the three here moved to D127-D129, with
a comment at the fork explaining the gap so it does not read as a mistake later. Five
older duplicates - D054, D055, D056, D084, D085 - predate this session and span both
threads' history; left alone rather than renumbered unilaterally.

**A formatting overreach worth naming.** They reported a formatting diff in
`orbistoun-translate` and deliberately left it, because it is this side's file. Correct,
and the reciprocal was not being done: every `cargo fmt --all` here would have reformatted
their crates. Nothing was actually modified outside this scope - checked - but the
practice was wrong and is now per-crate.

**The pattern worth recording, because it has now happened three times across two
threads.** Their library attribution was fabricated and looked fine. VINTRP's encoding row
was wrong and sat next to a comment saying it was unverified. The scalar condition-code
writes were missing and nothing here could observe it. All three are the same thing: **a
table that is plausible, internally consistent, and never checked against anything
outside itself.** Self-consistency is not evidence, and every one of these was found by
introducing an external check rather than by reading more carefully.

That is the argument for the two-route disagreement counter added last unit, and it is
now the argument for extending the same shape to the packet vocabulary as soon as there
are captures to do it with.

