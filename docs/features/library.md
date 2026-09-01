# The library

What Orbistoun has found that it can try to run, and how it decides.

The window's left panel is the library; the detail panel is what is known about the selected
entry. On the command line the same material is reached through `orbistoun inspect` and
`orbistoun status`.

## Where titles come from

Orbistoun **does not ship any**, and never will. What it scans is a directory you point it at.

The scan reads each candidate far enough to say what it is - the container, the executable
inside it, and the imports it declares - and stops there. Nothing is executed by scanning, which
is what makes it safe to point at a directory of unknown things.

`file → rescan library` picks up anything added since the window opened. It is a menu item
rather than a filesystem watcher on purpose: a rescan can be slow over a network share, and a
tool that quietly stalls because a directory changed is worse than one that rescans when asked.

## What the detail panel is telling you

The useful column is **imports**: the list of platform functions a title asks for before it runs
a single instruction.

That list is knowable in advance because interception here is *linking* rather than hooking - the
guest imports by hash, the loader resolves the whole table, and so the complete set of demands is
in hand before anything executes. It is the single best predictor of whether a title will get
anywhere, and it costs nothing to look at.

A name shown in full is one Orbistoun implements or has a record of. A bare hash is one nothing
has named yet - see [naming](naming.md).

## Running one

Selecting an entry and running it loads the container, maps its segments, resolves the import
table and starts the guest. Guest instructions are x86-64 and **run natively** - there is no
interpreter and no recompiler - so everything Orbistoun does is the operating system underneath
them.

That is why a title stopping is usually a missing or wrong system call rather than a wrong
instruction, and it is why the report from a run is about calls rather than about code.

## What "it did not work" looks like

**It stopped immediately.** Almost always an import that could not be resolved. The detail panel
lists them before you run.

**It stopped later, somewhere unrelated.** Usually a stub that returned a plausible answer
instead of failing honestly. This is the failure mode Orbistoun is built to avoid and the one
worth reporting, because it is the expensive kind: the damage happens at the call and shows up
thousands of frames later.

**It ran and drew nothing.** The graphics path is the newest part. A run that reaches the command
stream and produces no frame is a normal state today, not a surprise.
