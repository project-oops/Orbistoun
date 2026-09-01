# 2026-08-21 - The abort-at-53, diagnosed (D186, D187)


Solved as a diagnosis; the fix is one function short of done.

**`printf` was the key, and it was sitting in the name search all along.** The generator
reached it from the published-standard word list. Both titles had been calling it eight
times to explain exactly what was wrong, and the emulator was discarding the message and
then reporting that the guest stopped for reasons unknown.

Implemented, the guest immediately named four functions the search could not reach - with
the source file and line, and quoting this project's own `0x7fff0001` placeholder back at
us. Hash-confirmed rather than believed, then implemented.

**A conclusion I had recorded was wrong, and the tooling proved it.** I had run the ok-sweep
oracle, seen no change, and concluded "return values are not the cause". `printf` showed the
guest *still* receiving `0x7fff0001` under that policy: undeclared imports were skipped
entirely when the stub-return table was built, so the setting never reached the functions
under test. The experiment had not run.

That is D082 and D166 a third time - a setting consulted nowhere - but the damage was a new
shape. Not a wrong answer, a **confident negative**, which is worse: a wrong answer invites
checking and a clean negative closes the question.

With the policy actually applied: 53 calls and an abort became 215 and an ordinary fault.
Adding hash-keyed overrides bisected it to a single import in one run - `0x48a758b2e731cfd7`
answering success takes both titles to 23 imports, 220 calls, **95% of them on real
implementations**.

**Implementing the four honestly moved the call count down**, 53 to 45, because eight of
those calls were the guest complaining. The clearest demonstration yet that a call count is
not progress.

Next wall, also self-reported: `tlsf_create: Memory must be aligned to 8 bytes.` The D171
shape again - a stub reporting success without writing its out-parameter.

Also added, unused so far: `GuestStack::fill`, a stack poison recorded as a run condition.
Two runs with different fills answer "does this depend on memory nobody wrote?" directly,
which is the question `tlsf_create` has just raised.


