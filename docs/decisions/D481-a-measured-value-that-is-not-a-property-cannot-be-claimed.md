# D481 - A measured value that is not a property of the interface gets its own list

**measured** - 2026-09-02 (user-directed plan, TitleOwn loader scoping)

The hardware coverage gate had two lists: `CLAIMED` for measurements orbistoun is asserted
against, and `OUTSTANDING` for ones nothing asserts yet, each with a reason. Every constant
measurement had to be in one of them, which is what makes a new capture arrive as work rather
than as a file nobody reads.

**Two measurements fit neither.** The console answered `0x15` and `0x14` when asked to load two
of a title's own modules. Both captures agree, so they are constants by the gate's own
definition - and asserting either would be wrong.

## Why a module handle is not a property of the call

A handle is allocated by the loader. `0x15` says that console's loader had already placed
twenty modules when it answered; another machine, another firmware, or the same machine having
loaded one more thing first would answer differently. Two runs agreeing establishes that the
number is stable *on that machine in that state*, which is not the same claim at all.

Orbistoun answers `0x40` upward, and its own comment says the value is opaque and need not
match. That is right. Pinning orbistoun to another loader's bookkeeping would be recording
somebody else's accident as a requirement.

## Why it needed a third list rather than a note

Leaving them in `OUTSTANDING` says "not done yet", and every entry there should one day move
into a test. These never will. **A queue with permanent residents stops being read as a
queue** - which is the same failure the queue was built to avoid, since the whole point of the
coverage gate is that an entry in it is a real unit of work with a completion condition.

So `OPAQUE` names them and says why. The gate now requires every constant to be in exactly one
of the three, and the arithmetic is asserted rather than reported - checked by dropping a
category and watching the count come back 25 against 27.

## What is still asserted, because the record is still worth having

The *shape* is a property of the call even though the number is not: a title's own module must
load rather than be refused, and two loads must answer different handles. That is what a guest
keys on, and it is a test.

**The distinction to carry forward**: a measurement records what one machine did. Whether that
is a fact about the interface or a fact about the machine is a separate judgement, and the gate
now has somewhere to put the answer instead of forcing every record into a claim or a promise.
