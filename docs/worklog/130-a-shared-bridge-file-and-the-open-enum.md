# A shared bridge file, and the open-enum rule caught a live bug here


The two threads now have `<shared>\obscene-orbistoun-bridge.md` instead of being relayed by
hand. Neither repo owns it; durable contracts stay in their own repos and the bridge is the
conversation. Both of this side's review points came back settled.

**`generation` no longer collapses `both` into `neither`.** It reads `known|both` for two
driver stacks present and `absent|unknown` for none - distinct records now. `both`
deliberately names no console, because presence is not implementation, and the client carries
that reasoning into its *display* rather than leaving it in a doc: `generation both [two
driver stacks present; this names no console]`. The display is where somebody would otherwise
make that mistake.

**Report enum values are open; the protocol grammar is closed.** obSCEne may append a value
to a `sysinfo` state, a `res` status or provenance, or a `call` outcome without bumping the
version. Verbs, refusal reasons and capability tokens are fixed lists.

### Surprises

**That rule immediately found a real incompatibility in this crate, and it was live.**
`parse_outcome` **hard-failed** on an outcome word it did not know - it returned an error and
refused the whole line. The first outcome obSCEne added without a version bump would have
broken every session this client read.

The fix is where two rules meet and neither yields. The line **parses**, because refusing it
would break on a stream we were told to expect. And the outcome **never answers**:
`Outcome::Unrecognised` carries no value and reports `answered()` as false. Degrading is not
the same as assuming the best - an outcome nobody understands has not been understood, so it
cannot be a result. The same rule as `died` not being `returned 0`, one step further out.

**An unrecognised outcome is `observed-by = probe`, not `driver`.** It arrived *from* the
probe, so the probe observed something this reader cannot name. That is a different fact from
silence, and filing it with the deaths would have been the tidier and wronger choice. It also
meant narrowing an existing test that asserted every non-answering outcome was driver-observed
- true of the three known ones, and not a rule.

**`provenance` had the same shape of bug, benignly.** An unrecognised value and an *absent*
field both parsed to "no grade". They differ now: unrecognised grades as `assumed` - the
weakest reading, never the strongest - but stays distinguishable from a record that predates
grading and claims nothing. Only one of those means *this consumer is out of date*, and that
signal is worth keeping.


