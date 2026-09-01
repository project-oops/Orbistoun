# The bisection lever had never been connected


Went to use the stub policy on `sceSysmoduleLoadModule` - the classic one-bit oracle,
answer ok and see whether the guest proceeds - and it did nothing. So ran the control:
`default_return = "ok"`, everything answers success. Also nothing. Identical imports,
identical calls, identical fault, which is impossible if every stub had changed its answer.

Two gaps (D166). The config file did not carry a `[policy]` section at all, and - worse -
the stub-return table handed to the dispatcher was built only from the knowledge file's
declared return kind. The policy's default and overrides were consulted nowhere between the
file and the guest. Every override ever written was silently ignored.

That is D082 one layer up, and it hid for the same reason: the mechanism looks present from
every angle except the one that matters. `orbistoun-cli policy` prints an editable file,
`StubPolicy` has overrides, the service holds one - and none of it reached the call path.

Caught by the control experiment rather than by reading the code, which is the lesson worth
keeping: **before believing a setting works, set it to something that must visibly break,
and check that it does.** Same method that found the entry-convention bug this morning.

Precedence now: explicit override, then the knowledge file's declared kind, then the policy
default. The middle one beats the default deliberately - a blanket "answer ok" must not
undo D125 and start returning error codes in pointer registers.

Measured working: `default_return = "ok"` moves PPSA28061 from 47/933 to 48/935. Small, but
real where there was provably none. And it says something about the current wall -
blanket success does not get past `image+0x43c4`, so that fault is not a stub value.

