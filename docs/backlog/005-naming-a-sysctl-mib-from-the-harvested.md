# Naming a `sysctl` MIB from the harvested constants *(evaluated, not built)*


`sysctl` reports an unknown name as its numbers - `[1.14.8.0]` - and with 911 constants
harvested (D352) it looks like those could be resolved to `CTL_KERN.KERN_PROC.KERN_PROC_PROC`
and the work item made legible.

**Reverse lookup is ambiguous and was not built.** Counting candidates in the obvious prefix:

```
value 1  as CTL_*        CTL_KERN, CTL_P1003_1B_ASYNCHRONOUS_IO, CTL_SYSCTL_NAME
value 14 as KERN_*       KERN_PROC, KERN_PROC_OFILEDESC
value 8  as KERN_PROC_*  KERN_PROC_PROC                                  (unique)
```

Only the deepest component resolves. Narrowing the others means excluding candidates that
are really sub-identifiers sharing a longer prefix - which is a rule about **naming
convention**, not about the values, and a name produced that way would be inference
presented as a lookup. A wrong name here is worse than a number, because the number is
checkable against the header and a name invites being believed.

The meaning of this particular MIB is recorded with citations in D352 anyway, so the feature
would buy legibility at the cost of the one property that makes the table worth having.

Worth revisiting only with something that fixes the ambiguity properly - the header's own
hierarchy rather than its spelling.

