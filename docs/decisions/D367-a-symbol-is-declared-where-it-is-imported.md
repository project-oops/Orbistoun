# D367 - A symbol is declared where it is imported

**Status:** decided
**Date:** 2026-08-29

A symbol is declared once, in the library a guest was measured importing it from, and a
delegation entry binds that declaration to code in whichever crate implements it. A declaration
inferred from sibling symbols says so in its knowledge entry.

**Why:** where a symbol is declared is a claim about the target, and where its code lives is a
claim about this repository. Two declarations are two libraries claiming one function, and a
trace would label calls by whichever was found first. Declaring an export nobody observed
invents a fact.

**Rejected:**
- Declaring it beside its implementation: invents an export.
- Declaring it in both places: ambiguous trace labels.
