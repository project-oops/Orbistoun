# D276 - The expensive half of a watchpoint, built because the cheap half ran out


**decided** · 2026-08-25 · the `image+0xafc959` wall, after every call was eliminated

D223 chose a snapshot over a watchpoint and gave the reason: a watchpoint costs a debug
register, a per-platform API and an exception per access, while a snapshot answers *"did
anything ever fill this slot in?"* for one memcpy and no platform code. That was right for
the question being asked then, and it is still the thing to reach for first.

The question changed. Twenty-three dye runs - every function `PPSA02664` calls, implemented
and unimplemented, return values and out-parameters - left the fault byte-identical at
`write to 0xfffe0 while executing at image+0xafc959`. So the missing region base is not
produced by any call, and the snapshot has already said which words nobody wrote. What is
left to ask is **who read the word nobody wrote**, and a snapshot cannot answer that at all.

That is the case D223 named as the one where a watchpoint earns its keep: *when* and *who*
matter. So it is built now, and the two are kept as separate diagnostics rather than one
replacing the other, because they answer different questions and the cheap one is still the
one to run first:

| | `ORBISTOUN_WATCH` | `ORBISTOUN_WATCHPOINT` |
|---|---|---|
| mechanism | copy the region, diff it after | x86 debug registers, trap per access |
| answers | which bytes ended up different | which instruction touched it, and when |
| costs | one memcpy | a debug register, an exception per access |
| limit | any size | four, of 1/2/4/8 bytes, aligned |

**The two compose into a mechanical pipeline**, which is the point of building it rather
than a one-off script: the snapshot names the words nobody wrote, and up to four of those
addresses become read-or-write watchpoints on the next run, each reporting the instruction
that consumed the empty slot. Neither step needs a person to read the guest's code, which
is what makes it a candidate for step 17 of the loop rather than a manual detour.

