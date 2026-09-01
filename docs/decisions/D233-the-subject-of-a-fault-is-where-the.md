# D233 - The subject of a fault is where the guest died, not what it called


**decided** · 2026-08-25 · caught by `NeverPlanted` on the first live dispatcher run

`Finding::subject` for `Gap::Faulted` is `f.region` - `image`. The dispatcher took it as the
call to sweep, so it planted sentinels in the arguments of a region, which has none. Twelve
runs, nothing planted.

It was visible only because `Finding::NeverPlanted` exists. Without it the output is six
slots that changed nothing, which reads as *this call does not reach the address* - a
positive-sounding elimination drawn from an experiment that never ran. That is the second
time this exact failure has been caught by that one distinction; the first was passing
`library::symbol` to a variable that splits on `:`, which produced 276 empty runs and 23
false negatives.

The call leading in is in `Finding::evidence`, prefixed `just before: `. That prefix is now
`orbistoun_report::diagnose::PRECEDED_BY`, declared once beside the code that writes it
rather than copied into the consumer that reads it.

