# D489 - Placing a module does nothing until it is bound, protected and located

**measured** - 2026-09-03 (PPSA02664 now executes its own shipped code)

D482 found the title's modules, D483 decided how an import binds into one, and worklogs 336-338
placed and relocated them. The guest never executed a byte of any of it, and worklog 338
recorded that linking had *"no measured effect"* - correctly, and for a reason it did not name.

**Three things were missing, and each on its own made the other two inert.**

## 1. The binding was designed and never implemented

`PlacedTitleModules::resolve` - the whole of D483's rule - was called from nowhere. The worker
relocated the executable against `ImportResolver { thunks, data }`, which is the stub table.

So `0x6f8b9da539afc9af`, the NID D482 exists to answer, still landed on a stub. The run report
said so in as many words and nobody read it:

```text
! Il2CppUserAssemblies::0x6f8b9da539afc9af was called 222 times and has no name
    arg0 = "il2cpp_init"        arg2 = 0x7fff0001
```

222 calls, with names like `il2cpp_init` - a symbol-lookup function, answering a placeholder
every time, while the code that implements it sat relocated a few gigabytes away.

`TitleResolver` binds it now, in front of the stubs, for the imports D483's rule selects: the
module answers what orbistoun does not implement, and orbistoun's own implementations keep the
slots every measurement was taken against.

## 2. The modules were never made executable

Binding alone moved the fault straight to `0x480002a12890` - an **instruction fetch** inside a
module. Placement leaves every page writable and none executable, because relocation writes
into text; the executable has always been re-protected afterwards and the modules never were.

Same ordering as the executable: place, relocate, *then* protect.

## 3. The reporter could not see them

With the modules protected the guest ran into them, and the report announced:

```text
>> EMULATOR BUG: the fault is in orbistoun's OWN code, not the guest's
```

It was the guest's code. The test for "our code" is *the instruction pointer is outside every
region we registered*, and module regions were registered nowhere - not with the fault reporter
and not with `sceKernelVirtualQuery`, which would have refused an address the guest was running
from. Both fixed; the fault now reads `the title's own modules+0x13dca44`.

## The reporter was also contradicting itself about `0x7fff0001`

Separately, and found first: `0x7fff0001` is `GuestError::Unimplemented` - **orbistoun's own
placeholder**, documented in D128, D154, D186, D187, D190, D281 and D299. A stub returns it, the
guest uses it as a pointer, and the instruction pointer is then outside every placed region for
a reason that has nothing to do with a bug here.

The header said `EMULATOR BUG`. The findings section of the *same report* said:

```text
instruction fetch from 0x7fff0001 is one of our own placeholder codes, used as an address
```

Two parts of one report, opposite diagnoses, and the loud one at the top was wrong.
`orbistoun_core::placeholder_named` now recognises the family and the header defers to it.

**Bounded above as well as below**, which a negative test caught: `>= 0x7FFF_0000` alone claims
every vendor code too, and `0x8002_0016` is a value a console answered - naming it as
orbistoun's own would report a measurement as an invention.

## What it cost, honestly

| | before | after |
|---|---|---|
| distinct imports | 69 | 46 |
| calls | 10884 | 2080 |
| died | using a placeholder as an address | reading `0x8` inside `Il2CppUserAssemblies` |

**By the project's own metric this is `BACK`**, and the metric is not wrong: the guest reaches
less of the platform interface than it did. It reaches it *from inside its own shipped code*,
which it had never entered, and the new wall is a null field read in the title's il2cpp rather
than a jump through one of our refusals.

Both are true and the number is the one that will be compared next time, so it is written down
here rather than explained away. D487's point stands: distinct imports measure breadth, and this
was depth.
