# 2026-08-29 - Markers in `.bss`, and the SDK's logging helper


D359 established *that* the payloads depend on uninitialised `.bss`; this asks *which*
global (D360). Each slot now holds its own address under the fill byte, so a guest using one
as a pointer faults on something that names it.

**It has not fired.** Neither payload calls an uninitialised global - both read `.bss` and
derive something else, and a constant fill gives the identical fault. Reported as such
rather than as a finding.

What it did narrow is where they land, and that is worth having:

```
klogsrv  image+0x28fc   klog_printf +300
ftpsrv   image+0x819c   klog_printf +300
```

Same function, same offset, two different programs - so the dependency is in the SDK's
shared logging helper, not in either server. Consistent with everything else pointing at the
runtime. *Why* a filled `.bss` becomes a dereference of `-1` is not established and needs
their code.

### The mechanism is tested even though no guest tripped it

A marker that never fires and a marker that is wrong look identical from a run, so a unit
test decodes each slot back to its own address.

### And that test broke two others

It set the fill byte through the cache `bss_byte` reads once; tests share a process, so
every other test in the binary saw the change and the one asserting `.bss` is zeroed failed.
The byte is a parameter now.

**Third appearance of this hazard** - after `orbistoun-abi`'s shared array and D324's
fixed-address collisions - and the same fix each time: pass the thing rather than reaching
for it.

