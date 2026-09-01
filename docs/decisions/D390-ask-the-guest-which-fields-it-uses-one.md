# D390 - Ask the guest which fields it uses, one poisoned field per run


**assumed** - 2026-08-30

The structure a payload's runtime is handed is not published, and every session that has
touched it has done the same thing by hand: put a value in a field, run, read the fault, try
another. Twelve edits and twelve faults to answer one question.

It is a command now. `orbistoun-cli handoff <path>` poisons **one field per run** with an
address nothing maps, and reads back whether the guest faulted on it:

```text
  field  0  USED         instruction fetch from the field's own value
  field  1  USED         read of the field's own value, at image+0x4519
  field  2  USED         read of the field's own value, at image+0x44ef
  field  3  not reached  (read of 0x2001)
  field  5  USED         write to the field's own value, at image+0x24a2
fields used: 0, 1, 2, 5
```

**One field at a time, deliberately.** Poisoning several answers "did it touch any of these",
which is a question nobody asked, and the first fault hides the rest. A run that faults on the
value *used* the field; a run that ends anywhere else **never reached it**, which is as much of
an answer and the half that is otherwise hard to get - a field nothing touches produces no
evidence of any kind.

Three payloads, built separately, agree exactly: field 0 is **called** (the resolver, D365),
1 and 2 are **read**, 5 is **written through**, and 3, 4 and 6-11 are never reached. Field five
is the new one and it is the interesting kind - the loader hands the payload somewhere to put
something.

### Why the tool matters more than the answer

This session leaned hard on the payloads being open source and carrying symbol tables: names in
fault reports, a `main` to enter at, READMEs describing an interface. **None of that will be
true of a commercial title**, and a method that needs symbols stops working the moment it is
pointed at the thing this project exists for.

This needs neither. A stripped binary, an undocumented structure, one run per question.

