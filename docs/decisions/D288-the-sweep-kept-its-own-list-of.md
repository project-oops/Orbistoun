# D288 - The sweep kept its own list of diagnostics, and it was already wrong


**decided** · 2026-08-26 · found while asking what else the loop could run by itself

`GuestTrial::spawn` clears every diagnostic before each run, and says why in the strongest
terms available: *"**Every diagnostic variable cleared first, always.** One experiment
inheriting another's - or the environment this sweep was launched from - is not a controlled
run, and a baseline taken with a stale variable set is not a baseline at all."*

It cleared them from a **hand-written array of seven strings**. `ORBISTOUN_WATCHPOINT` was
added to `orbistoun-env` earlier the same day and never reached it, so a sweep launched from
a shell with a watchpoint set would have inherited it into all twenty-four runs and called
the result a controlled experiment.

`orbistoun-env` exists to stop exactly this. Its own module documentation gives the reason -
*"a typo does nothing… documentation drifts… nothing stops another one appearing"* - and the
registry is described as **the one list**. A second copy of it in another crate is the
failure that crate was built to prevent, reproduced inside a function whose comment is about
not letting runs contaminate each other.

Same family as D123 and D281: a second list that looks authoritative, drifts silently, and is
only wrong in the one place nobody re-reads.

**And the same sweep held four more copies.** `Axis::env` spelled every variable it sets as a
string literal, and `orbistoun-llm` kept its own `ORBISTOUN_LLM_API_KEY`. All now read
`orbistoun_env::<VAR>.name`, which is what `orbistoun-paths` already did and said why:
*"named by `orbistoun-env` rather than here, so the one list of what this project reads stays
one list."* Three literals survive on purpose - `option_env!` needs one by construction, and
the `orbistoun-paths` and `axis` tests assert the spelling deliberately, which is the second
list that **catches** drift rather than causing it.

So the array goes and the diagnostics are read from the registry, filtered on
`Kind::Diagnostic`. Settings are deliberately **not** cleared - `ORBISTOUN_DATA_DIR` is how
the sweep points a trial at its own temporary trace directory, and clearing it would send
every run at the machine's real one. That distinction is what `Kind` is for, and it is now
load-bearing rather than descriptive.

