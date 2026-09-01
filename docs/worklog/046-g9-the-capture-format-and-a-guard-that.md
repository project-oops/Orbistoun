# G9: the capture format, and a guard that was right


The packet vocabulary now has a place to be checked. `crates/orbistoun-gpu/tests/captures/`
takes a pair per capture - what a library call asked for, and the bytes it appended - and
`tests/vocabulary.rs` checks one against a decode of the other. There are no captures yet;
the suite says so loudly and `orbistoun.sh check` reports it beside the device-test skip.

The pairing is the whole design. A recorded command buffer on its own would have to be
read through the table under test, so agreement would prove nothing. The call states the
answer and the bytes are the question, which is what makes this an oracle rather than a
mirror - the same shape as the shader decoder's differential test one layer down.

The comparator returns errors instead of panicking, so both directions are tested with an
empty corpus: one synthetic capture that agrees, one that does not. A comparator only ever
exercised by data that makes it pass cannot be told apart from one that returns success
unconditionally, and this file exists entirely to fail when the table is wrong. The
synthetic fixtures are marked emphatically as harness-validation and not evidence - they
are generated from the table they would be checking.

**Surprises.**

- **The provenance guard failed, and it was right.** The other thread staged the
  repository, which tracked the shader fixtures for the first time, and the guard bans
  `.bin` outright. The obvious fix was a path exception in the three places the guard
  lives - `orbistoun.sh`, `ci.yml`, `.githooks/pre-push`.

  That would have weakened a principle-1 mechanism to accommodate a naming collision, and
  left a directory where a real dump could later be committed unnoticed. The actual
  problem was that **generated fixtures and dumped shaders shared an extension while
  carrying opposite obligations**: one must never be tracked, the other must be. Fixtures
  are now `.gcn`, the guard is untouched and exactly as strict, and `corpus::is_shader`
  accepts both because the difference is provenance rather than content.

  Worth stating plainly: the guard caught a real conflation, not a false positive. The
  first instinct - widen the check that is complaining - would have removed the only thing
  that noticed.

- **The rename reached further than expected.** The generator, the differential test, a
  translate test, and the census CLI's file filter all named the extension. The last of
  those was in a shim crate the other thread also works in; the change is one predicate
  and a message, and it was the census that noticed - it reported zero shaders where it
  had reported ten.

**Still red outside this side.** The gate fails on formatting in `orbistoun-libc`, which
belongs to the loader work. Every crate here is clean.

**Next.** G13, subgroup fidelity - the last item on the roadmap with a real oracle and no
dependency on a capture.


