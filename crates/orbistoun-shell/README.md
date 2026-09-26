# orbistoun-shell

The system software: what runs when a title is not the whole machine.

It models the session lifecycle, the guest-visible event queue, and the hardware settings a
person chooses. It fakes nothing; what it cannot deliver truthfully, it withholds and counts.
[orbistoun-systemservice](../orbistoun-systemservice/) exposes its settings to the guest, and
[orbistoun-gui](../orbistoun-gui/) is one front-end over it.

## A model, not a screen

Most of what the shell does is invisible: an interrupted title has to be told, an input the
shell consumed must not reach the title, and the settings a title reads are the ones a person
picked in a menu. None of that is drawing. So the model lives here, below the shims that
expose it to the guest and below the window that drives it, and a front-end could be replaced
by a command line without any semantics moving (CLAUDE.md, *Contracts at guest semantics*).

## Multitasker

The shell is a multitasker, not a launcher. A multitasker keeps the title alive while
something else has the screen, which makes every subsystem answer who a frame is for, who a
button is for, and whether a thread is meant to be executing.

`Lifecycle` is one value with four states (`Foreground`, `Overlaid`, `Background`, and no
title), and `Focus`, `Video` and `Execution` are derived from it, never stored. Two fields
that can disagree about focus eventually will, and the result is a title acting on a button
press the shell already consumed.

## Meaning and number

**Rule:** the shell never delivers a guest an event code or a setting value nobody has
measured. Inventing a plausible code is the forbidden case of CLAUDE.md's *Honest failure*:
the guest reads a number that means something specific to it, acts on it, and the failure
surfaces elsewhere.

- `ShellEvent` is the project's own vocabulary and carries no codes; `Delivery` maps a
  meaning onto a measured code.
- `Settings` holds what a person chose and is entirely the project's own; `Parameters` holds
  the measured numbers a guest reads.

An event with no code is not delivered, and `Withheld` reports it in the run report - for
example "4 withheld for want of a measured code (backgrounded x2, focus-lost x2)" - rather
than the shell appearing to work. Codes arrive the way every other fact here does: measured
and attributed.

## Naming

No vendor product name appears here, and a front-end built on this crate is named after this
project rather than after anything it resembles (CLAUDE.md, *Naming*).
