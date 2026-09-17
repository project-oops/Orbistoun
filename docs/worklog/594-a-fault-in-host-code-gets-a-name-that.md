# 594. A fault in host code gets a name that survives a reboot

**2026-09-15** - the defect worklog 593 found while answering something else

## What was wrong

`locate` names the five regions orbistoun places - image, stubs, stack, guest mappings, the
title's own modules. Anything else fell through to a bare address, and both leading titles end in
host code, so both recorded one:

```
outcome = "0x7fff13abdc8d"
```

**That number moves.** Windows bases system modules per boot, so the same fault reached the same
way was `0x7fff13abdc8d` one day and `0x7ff9c071dc8d` the next. Two consequences, both bad, and
neither previously written down:

- The recorded outcome **cannot be reproduced after a reboot**. What reads as a measurement of the
  guest is partly a measurement of where the host loader happened to put something.
- `compare` reports the ending as changed whenever the machine rebooted between two runs, with
  nothing having changed - a false signal in the **only** measure of progress this project has.

## What it does now

`host_module_of` asks `VirtualQuery` for the allocation base holding the address - which for a
mapped image is its module handle - and `GetModuleFileNameW` for its name. `locate` is tried
first and unchanged; this is only the fallback, and the raw address still survives in
`instruction_pointer`.

```
fault VCRUNTIME140.dll+0x1dc8d   (was 0x7ff9c071dc8d)
```

Both titles now record that, and it reproduces: a module's base is stable relative to itself.

**It is also a better fault.** The address said only that some host code failed. The name says
*which*, and `VCRUNTIME140.dll` is where `memcpy` lives - which is exactly what worklog 553 said
was happening, from a completely different direction: the guest carries orbistoun's `0xf7ff0001`
placeholder into a `memcpy` thirty-two times because `sceAgcDcbSetCxRegistersIndirect` is
unimplemented and answers it. A diagnosis reached by reading call counts, now confirmed by the
faulting module naming itself.

## The file name, never the path

`GetModuleFileNameW` answers a full path, and this string is written into `compat/`. An absolute
path from this machine must never reach a tracked file - the identity guard blocks exactly that.
Only the last component is kept, and the test asserts the answer contains no separator; the
mutation that returns the whole path fails it with a machine path in the message, which is the
right way to find out.

## The subtle one, and why the negative test earns its place

`GetModuleFileNameW(NULL)` is documented to answer **the current executable**. So without the
`base == 0` check, an address in no mapping at all comes back named as this binary:

```
left: Some(("orbistoun_worker-....exe", 139637976727552))
right: None
```

A fabricated location, with a 139-terabyte offset, inside a field that reads as a measurement -
principle 3's failure exactly, and it would have looked plausible in a report. Both guards were
made to fail before being believed.

`obscene-payload` exercises the same path from the other side: its outcome is `0x5e2d`, too small
to be in any mapping, and it stays a bare address because there is genuinely no module to name.

## Where it runs

After the fault, beside the call trace, on the same terms that block already documents - it
allocates, and by then the process is only assembling its report. Not in the handler: `locate` is
pure and allocation-free because it runs there, and this does not belong beside it.

## The frontier moved, and the diff is the point

The gate went red, which is what it is for. Regenerated, and the diff is two lines:

```
-PPSA03416-app0  flipped  220 imports (188 answered)  470416 calls  0x7fff13abdc8d
-PPSA02664-app0  flipped  220 imports (186 answered)  418420 calls  0x7fff13abdc8d
+PPSA03416-app0  flipped  222 imports (192 answered)  470422 calls  VCRUNTIME140.dll+0x1dc8d
+PPSA02664-app0  flipped  222 imports (189 answered)  418424 calls  VCRUNTIME140.dll+0x1dc8d
```

The `+2 imports` is **not** from this change, which touches only how a fault is described. These
runs are the first to measure the tree since another session landed fourteen units overnight, and
the run reported `FURTHER - executed code it could not reach before`. Recorded as movement whose
cause is elsewhere rather than claimed for this unit.

## Surprise: PPSA02664 is not deterministic

Four runs gave 222, 221, and 222 distinct imports, with calls moving by ±85. The record refused
the 221 run as `BACK`, correctly - which is *why* the first post-fix run left the old bare-address
outcome in place, and it took a run that tied 222 to replace it.

Worth having beside worklog 555, which established that **PPSA25872** is deterministic across five
runs and reasoned from it. That property does not generalise across the corpus, and a conclusion
drawn from a single run of this title can be a conclusion about ±1 import.
