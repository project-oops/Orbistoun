# D504 - Declared names come from real import tables

**Status:** decided
**Date:** 2026-09-03

A library function is declared only under a name read out of a real module's import table or
measured resolving on hardware. Where no arity is established, the declaration records six, the
trampoline's full capture, and an arity already recorded in the knowledge base takes precedence.

**Why:** a wrong name is a NID that matches no import and a shim that can never be reached,
while a wrong arity only shapes a call trace; arity never reaches the call path. Recording all six
argument registers loses nothing, where guessing low silently discards arguments a trace exists
to show.

**Rejected:**
- Names from a candidate list or completed from a naming pattern: a NID no guest will ever ask for, inflating the declared surface.
- Declaring an unnamed hash: a hash is not a name and says nothing about the function.
- Guessing a smaller arity per function: discards arguments with no evidence to justify it.
