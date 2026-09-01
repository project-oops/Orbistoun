# D166 - The main bisection lever was wired to nothing


**decided** · 2026-08-20

Principle 5 states the test plainly: *if answering "what does this function return?"
requires a rebuild, it is in the wrong place.* `StubPolicy` exists for exactly that, with a
default and per-symbol overrides, and `orbistoun-cli policy` prints it as editable TOML.

**Nothing read one back, and nothing consulted it on the call path.**

Two separate gaps, found by trying to use it:

1. `FileConfig` carried the entry, thread, memory and library settings but not the policy,
   so a `[policy]` section in the config file went nowhere.
2. More seriously, the stub-return table installed into the dispatcher was built *only*
   from the knowledge file's declared return kind. The policy's default and overrides were
   never consulted at any point between the file and the guest.

So every override anybody wrote was silently ignored. That is D082 again one layer up - the
registry knew things and nothing asked it at the moment a call happened - and it went
unnoticed for the same reason: the mechanism looks present from every angle except the one
that matters.

### How it was caught, which is the part worth keeping

Not by reading the code. By running the control experiment: set `default_return = "ok"` and
check that behaviour changes *at all*. It did not - identical import count, identical call
count, identical fault - which is impossible if every stub had started answering zero
instead of an error code.

The same shape as the entry-convention measurement (D159): **before believing a setting
does what it says, set it to something that must visibly break and check that it does.** A
setting whose effect is never verified is indistinguishable from one that is not wired up,
and this one was not wired up for its entire existence.

### Precedence, and why this order

1. **An explicit per-symbol override** wins over everything. Somebody has typed a
   deliberate experiment and that is the question being asked.
2. **The knowledge file's declared return kind** beats the policy *default*. A pointer-,
   handle- or count-returning function answers zero regardless, because an error code in a
   pointer register is a wild pointer the guest dereferences immediately (D125) - and a
   blanket "answer ok" must not quietly reintroduce that.
3. **The policy default** for everything else.

Verified by measurement rather than by inspection: `default_return = "ok"` now moves
PPSA28061 from 47 imports and 933 calls to 48 and 935. A small difference, but a real one
where there was provably none.

It also says something about the current wall: blanket success does **not** get past
`image+0x43c4`, so that fault is not a stub return value and no amount of tuning one will
move it.

