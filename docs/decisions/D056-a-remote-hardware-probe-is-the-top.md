# D056 - A remote hardware probe is the top-value future capability

**decided** · 2026-08-19 · aspirational, gated on hardware

The idea: run `obSCEne` on real hardware in a **remote-controlled** mode, so a
developer or an agent can issue calls and read back what the real system does.

**Why this outranks most of the roadmap.** The central difficulty of this project is
that there is no specification - every layer past the loader is inference against a
black box, and TESTING.md's entire oracle hierarchy exists to work around that. A
remote probe does not work around it; it removes it. "What does this return for an
unaligned request" stops being a research project and becomes a query.

**It strengthens provenance rather than threatening it.** D014's concern is derivation
*from source*. Black-box measurement through a documented interface is the lawful
alternative and is cleaner than any other route to the same knowledge: observe, record,
implement independently. This is the clean-room pattern, not a compromise of it.

**The output is a corpus, not a conversation.** Whatever is learned must land as a
committed, machine-readable probe corpus - input, observed output, firmware version -
and become test assertions. Knowledge that lives only in a session evaporates at the
next compaction, which is the same argument that made the run report rather than the
logs the contract (D046). This is also what finally populates the error-code corpus
that currently has no acquisition path.

**Design problems that are real, not incidental:**

- **Crashes are the normal case.** Arbitrary calls with arbitrary arguments hard-fault
  constantly. Needs a watchdog and auto-restart, and must distinguish *"returned X"*
  from *"died before answering"* - a timeout read as a null return would poison the
  corpus with fiction, which is worse than having no corpus.
- **State leaks between probes.** An allocation changes what the next probe observes.
  Either a fresh process per query, an explicit reset, or ordering recorded as part of
  the input.
- **It answers "what does it return" far better than "what does it do".** Side effects
  need paired observation; timing-dependent behaviour will not reproduce. Knowing that
  boundary matters before trusting a result.

**Home:** `obSCEne`, as a fourth reporting mode alongside D043's trace-as-report,
self-reporting, and interactive tests. Same app, same sections, driven remotely rather
than running a fixed suite. The boundary stays clean - obSCEne is guest software,
orbistoun is the host, the corpus is what crosses between them.

**Gated on hardware.** Console jailbreaks are firmware-locked, so this realistically
means a dedicated device kept off updates. Recorded now because it should shape
obSCEne's structure from the start rather than being retrofitted: the sections and the
result model already suit it.

