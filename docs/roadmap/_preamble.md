# Roadmap

Committed next steps, in dependency order. Anything not here is in
[BACKLOG.md](BACKLOG.md); rejected directions are in [SCOPE.md](SCOPE.md). The
reasoning behind each shape is in [DECISIONS.md](DECISIONS.md).

The ordering rule: **each phase must produce an observable result on its own.** The
alternative - building the container parser, the address space, and the GPU layer
before anything can be run - is how a project reaches month six with nothing to show
and no way to tell which of three layers is wrong.

Phases 1-6 are a strict chain, with 1b and 2b hanging off 1. Phases 0b through 0e are
independent of everything and of each other.
