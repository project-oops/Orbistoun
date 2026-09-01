# 2026-08-30 - The probe answers a question about itself


Running the conformance probe under orbistoun: 498 pass, 3 fail. **Two of the three failures
and a partial are one bug**, and the probe named it unprompted (D392):

```text
900-surface/control   a symbol that does not exist reported present;
                      every count in this section is meaningless
005-generation/detect both generations' drivers resolve (real back-compat, or
                      a stub-everything loader answering for free)
```

Every import gets a stub so an unimplemented call is *reported* rather than a jump into a
zeroed slot. That is the interception model and it is right for measuring - and it means the
platform answers yes to every symbol anything has ever asked about. A probe inferring a
machine's kind or its console generation from what is present gets both answers.

`ORBISTOUN_RESOLVE=named` refuses the imports this build cannot even name - not unimplemented,
*unknown to every input here* - and they relocate as unresolved, which the tally already
counted. Entering had to stop being gated on a complete tally: a refusal is not a failure to
link.

Under it a library comes back honestly absent for the first time. The control still fails, so
its symbol is one we *can* name - which is now a question about one symbol rather than about
the whole method, and one for the probe rather than for here.

Not the default: every record in `compat/` was taken with everything resolving. Accuracy is the
eventual default, measurement is the current one.

**And adding it broke the thing that keeps records honest.** `Experiments` decides whether a
run was ordinary from a hand-written list of variables, so a new setting is invisible until
somebody remembers - and the first `named` run wrote itself into the status slot reserved for
unhelped runs. Third setting-shaped defect today: a report that could not fire, a dump that
could not see a thread's stack, and a diagnostic that did not declare itself.

