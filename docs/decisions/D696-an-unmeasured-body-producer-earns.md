# D696 - An unmeasured-body producer earns wiring when it hands the guest a real cursor

**assumed** - 2026-09-15

`sceAgcDcbSetCxRegistersIndirect` is wired as the seventeenth AGC builder, and unlike the other
sixteen it does not encode its packet body. It writes the measured header, reserves the measured
20-byte extent, zeroes the four body dwords, and returns the real cursor. This records why that is
a builder worth wiring rather than a half-measured guess.

## The rule the other sixteen follow, and why this is outside it

Every other builder here was wired only once its bytes were measured across more than one input,
because a builder guessed into this file gets amended by the `sceAgc*Patch*` family before anyone
reads it back, and a wrong packet under a correct patch is invisible. That rule is about packet
*contents*.

This builder is a **producer**: its entire job in the failing path is to reserve space in the
command buffer and return the address of it, so the guest can fill that space through the patch
family. `REQ-...4386` measured exactly that - the producer returns the packet-start pointer and
advances the cursor 20 bytes; the patch then amends the body in place without advancing. The
contents the producer writes are overwritten by the guest; the reservation is not.

So the thing that must be correct - the header opcode and the extent - is measured, and the thing
that is not measured - the argument-to-body mapping - is the thing the guest replaces anyway.
Wiring it is not guessing a packet; it is performing a measured reservation.

## Why the body is zeroed rather than filled from the capture

`REQ-...4386` is one before/after, not an argument sweep. It shows one body, produced from
obSCEne's own arguments. Writing those four dwords into the guest's stream would encode obSCEne's
arguments, not the guest's - a confident wrong answer of exactly the kind principle 3 forbids. Zero
is the honest "not filled", the same choice `pad::AT_REST` and `statfs` make for the fields they
have not measured, and the header remains a valid, self-describing packet of the right length
regardless.

## Why this is worth doing before the body is measured

Because it fixes a real bug independently of the body. Unwired, the producer answered the loud
placeholder `0xf7ff0001`, and the guest carried that placeholder as the packet's address into a
`memcpy` - a documented fault path (worklog 553, 594). A real cursor removes the placeholder from
the guest's data flow, which was proven: the patch's first argument changed from `0xf7ff0001` to a
real guest mapping after the fix.

That the wall did not move - a second, unrelated null-pointer read stands behind it - does not make
the fix wrong. It resolves one documented placeholder-as-pointer bug and reveals the next obstacle
cleanly, which is worth more than leaving the placeholder in place.

## What would change this

The argument sweep `REQ-...4386`'s single capture could not give. Once the producer's
argument-to-body mapping is measured, this builder becomes a full encoder like the other sixteen,
the zeroed body is replaced with the real one, and this entry's "different in kind" caveat lifts.
Until then, wiring it is a measured reservation with an honestly empty body, not a guessed packet.
