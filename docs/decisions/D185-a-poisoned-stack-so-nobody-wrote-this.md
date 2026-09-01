# D185 - A poisoned stack, so "nobody wrote this" can be measured


**decided** · 2026-08-21

The host hands back zeroed pages, so guest memory nobody has written reads as zero -
consistently, every run. That is comfortable and misleading. A stub that should have filled
a caller's buffer and did not leaves the guest reading a plausible, stable zero, and
**there is no signature to recognise in a trace because nothing was written to recognise**
(D171).

`GuestStack::fill` writes a byte over the usable stack before the guest is entered, chosen
by `ORBISTOUN_STACK_FILL`. Running twice with different fills answers the question directly:
if the two runs disagree, the guest read memory nobody wrote; if they agree, it did not, and
a whole class of explanation is eliminated rather than argued about.

### Why an environment variable rather than a setting

It is a *question*, not a configuration. A question is asked once. Anything that outlived
the asking would drift into being a permanent workaround for a bug nobody found - which is
how a diagnostic becomes a shim.

Refused rather than skipped on failure, for the same reason: a diagnostic that silently did
not run answers the question wrongly and confidently, which is the worst of the three
outcomes.

### Recorded as a run condition

A poisoned run is answering a different question from an ordinary one, so `Conditions`
carries the fill and a comparison across it is labelled (D181). Otherwise two runs measuring
different things report `same` and invite exactly the wrong conclusion.

### First use returned a clean negative

Investigating why a Unity title's `tlsf_create` complains its memory is misaligned, eight
megabytes of `0xCC` produced an identical run - same imports, same calls, same fault, same
message. The guest is not reading unwritten stack on that path. The D171 explanation is
eliminated, which is worth more than another plausible hypothesis.

**Recorded late.** Three source comments cited this number before it existed, because the
session moved on to a louder discovery before writing it down. The duplicate-number check
(D201) would not have caught it: it looks for two decisions sharing a number, not a citation
pointing at none.

