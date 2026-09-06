# 2026-09-03 - (/loop) The guest runs its own code

```
tests   1984  ->  1991
```

PPSA02664 now executes `Il2CppUserAssemblies`. It has never done that before, and getting there
took undoing three separate reasons why placing the module had achieved nothing.

## It started with the fault address, and the decision log had it

`0x7fff0001` is `GuestError::Unimplemented` - **orbistoun's own placeholder** - recorded in
D128, D154, D186, D187, D190, D281 and D299. Grepping the decision log first is this tick's own
rule after D488, and it turned a wall into a one-line answer.

So the report's header was wrong:

```text
>> EMULATOR BUG: the fault is in orbistoun's OWN code, not the guest's
```

And the *findings section of the same report* was right:

```text
instruction fetch from 0x7fff0001 is one of our own placeholder codes, used as an address
```

**One report, two opposite diagnoses, and the loud one at the top was the wrong one.** The test
for "our code" is "the instruction pointer is outside every placed region", which cannot tell
our code from our own *return values*. `placeholder_named` now recognises the family; the header
defers to it.

A negative test earned itself immediately: `>= 0x7FFF_0000` alone also claims every vendor code,
and `0x8002_0016` is a value a console answered. Naming that as orbistoun's own would report a
measurement as an invention. Bounded above.

## Then the report answered the actual question

```text
! Il2CppUserAssemblies::0x6f8b9da539afc9af was called 222 times and has no name
    arg0 = "il2cpp_init"        arg2 = 0x7fff0001
```

**That is the NID D482 exists to answer** - a symbol-lookup function, called 222 times with
names like `il2cpp_init`, answering a placeholder every time while the code implementing it sat
relocated a few gigabytes away.

## Three things were missing, each making the others inert (D489)

**The binding was never implemented.** `PlacedTitleModules::resolve` - the whole of D483's rule -
was called from nowhere, and the worker relocated the executable against the stub table.
`TitleResolver` puts the title's modules in front of the stubs, for the imports D483 selects.

That alone moved the fault to an **instruction fetch** inside a module, because -

**The modules were never protected.** Placement leaves pages writable and none executable, since
relocation writes into text. The executable has always been re-protected afterwards; the modules
never were. Same ordering now: place, relocate, then protect.

Which got the guest running, and then -

**The reporter could not see them.** A fault inside a module was outside every registered region,
which is the exact test for "our code faulted", so the guest's own crash was announced as an
emulator bug. Module spans are now registered with both the fault reporter and
`sceKernelVirtualQuery` - the latter would otherwise refuse an address the guest is running from.

```text
guest fault: read of 0x8 while executing at 0x4800013dca44 (the title's own modules+0x13dca44)
  from 0x4800013d5f00 (the title's own modules+0x13d5f00)
```

## The score, which is not flattering

| | before | after |
|---|---|---|
| distinct imports | 69 | 46 |
| calls | 10884 | 2080 |
| died | using a placeholder as an address | reading `0x8` inside `Il2CppUserAssemblies` |

**The verdict is `BACK`, and it is not wrong.** The guest reaches less of the platform interface
than it did - it reaches it from inside its own code, which it had never entered, and the wall
is now a null field read in the title's il2cpp rather than a jump through one of our refusals.

Writing the number down rather than explaining it away, because it is the one the next run
compares against. D487's point again: distinct imports measure breadth and this was depth.

## Also

The ±3 call oscillation survives (44/46 distinct, 2077/2080 calls across three runs), on a
stable fault address. Still unexplained, still not moving anything that matters.

Four `place_and_relocate` extractions came out of the line limit rather than of taste -
`describe_title_modules`, `publish_what_the_guest_reads`, `relocate_the_executable`. The last
one is the better shape anyway: the resolvers borrow the tables, and building them inside the
function ends the borrow before the report starts moving things about.

## State

`cargo test --workspace` green - **117 suites, 1991 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean.

Nothing committed. The day holds worklogs 292-339 and D466-D489.

**Next**: the null read at `the title's own modules+0x13dca44`. The guest is in its own il2cpp
init and dereferencing a field of a pointer nothing set - so the question is which call was
supposed to have returned it, and the calls just before the fault are printed.
