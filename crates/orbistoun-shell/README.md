# orbistoun-shell

The system software: what runs when a title is not the whole machine.

**Models:** the session lifecycle, the guest-visible event queue, and the hardware settings a
person chooses.

**Deliberately fakes:** nothing. It withholds instead - see below.

## Why this is a crate and not a screen

The platform's shell looks like a user interface, and building it as one gets the architecture
wrong on day one. Most of what it does is invisible: a title that is interrupted has to be
*told*, an input the shell consumed must not reach the title, and the settings a title reads
are the ones somebody picked in a menu. None of that is drawing.

So the model lives here, **below** the shims that expose it to the guest and **below** the
window that drives it. The front-end is a view onto this crate and could be replaced by a
command line without any semantics moving (principle 12).

## Launcher or multitasker

Multitasker, decided deliberately. A launcher has two states and "return to the shell" means
the process died; a multitasker keeps the title alive while something else has the screen,
which forces every subsystem to answer a question it never had to - *who is this frame for,
who is this button for, is this thread meant to be executing.*

`Lifecycle` is one value with four states, and `Focus`, `Video` and `Execution` are **derived
from it rather than stored**. Two fields that can disagree about who has input focus
eventually will, and the resulting bug is a title acting on a button press the shell already
consumed.

## What it will not do

**It cannot deliver a guest an event or a setting nobody has measured, and it counts what it
withholds.**

A title learns it was interrupted by draining an event queue. This repository has no lawful
source for what those events are *numbered*, and inventing a plausible code is principle 3's
forbidden case exactly - the guest reads a number that means something specific to it, acts
on it, and the failure surfaces somewhere else entirely.

So meaning and number are kept apart. `SystemEvent` is our own vocabulary and carries no
codes; `Delivery` maps a meaning onto a code and ships **empty**. Same split in `settings`:
`Settings` holds what a person chose and is entirely ours, `Parameters` holds numbers and
ships empty too.

An event with no code is not delivered, and `Withheld` says so in a run report - *"4 withheld
for want of a measured code (backgrounded x2, focus-lost x2)"* - rather than the shell
quietly appearing to work. That is a worse emulator today and the only version that can
become a correct one; codes arrive the way every other fact here does, measured and
attributed.

## Naming

Principle 2: no vendor product name appears here, and the front-end built on this should be
named after this project rather than after anything it resembles. Reproducing the hardware's
presentation is the one way to take a clean-room design and hand back the position it was
built to hold.
