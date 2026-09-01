# D404 - The wall behind 649: firmware-specific address arithmetic


**guest-observed** - 2026-08-30

Past the version check (D403), `elfldr` takes the branch for the firmware it was told and then
stops again. What it does there is worth stating precisely, because a first draft of this entry
got it wrong in the direction the whole project exists to guard against - claiming more than the
instructions show.

### The wall behind it: firmware-specific address arithmetic

The version is not the last wall. The branch a guest takes on a modern version does this:

```
mov  (%rbx),%rax           ; rbx is the handoff, so this is field zero - the resolver
lea  -0xcc0000(%rax),%rcx  ; a fixed offset below it
lea  0x28c9e80(%rax),%rcx  ; a fixed offset above it, ~42 MiB
```

**What is measured:** the runtime computes a spread of addresses by *fixed, firmware-specific
offsets from handoff field zero* - the resolver's own address - and stores them in its globals.
The offsets differ per firmware band, which is why the version gate comes first. All five
payloads carry the same dispatch constants, so they share one runtime.

**What is not established, and I will not claim it:** whether those computed addresses are inside
libkernel, in another module, or in the kernel. An earlier draft of this entry called field four
a kernel base and the arithmetic kernel-address computation. That was wrong twice - the base is
field zero, not four (field four is only *null-checked*, and its use is untraced), and nothing
measured says the targets are kernel rather than library addresses. The `-14`/`je` null checks on
fields three and four are real; what those fields are *for* is not yet read.

### Why this still means "runs to completion" is not close

Whatever those addresses are, the runtime expects a real system library (or more) laid out at
firmware-specific offsets from the resolver, and then reads and writes through the results. orbistoun
hands out thunks at addresses of its own choosing, so the offset arithmetic lands nowhere real.
Making it land somewhere real means reproducing a specific firmware's memory layout - addresses
this project cannot derive from its own inputs, and must not invent, because a guest reading a
made-up layout and acting on it is the exact failure the project refuses.

### What is worth doing instead, and it is this project's own move

Hand field zero, and the computed spread, into an **unmapped marked region**, so every access
through a firmware-offset address faults at an address that *names the offset the payload wanted* -
exactly as the handoff-field markers named which field a guest read (D369). That cannot run the
payload, but it turns "expects a firmware layout" into an itemised list of the exact offsets each
one reaches for, per firmware. That list is a measurement worth having and is the honest next step;
the interpretation of the offsets waits on it and on the hardware probe.

