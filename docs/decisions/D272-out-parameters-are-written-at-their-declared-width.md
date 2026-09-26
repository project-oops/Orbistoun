# D272 - Out-parameters are written at their declared width

**Status:** decided
**Date:** 2026-08-25

An implementation writes an out-parameter at the width of the type it points to: four bytes
through an `int *`, a full word through a pointer-sized slot. A pointer-to-pointer argument is
a handle the implementation allocates and writes back, not the object itself.

**Why:** writing a word through an `int *` overwrites the caller's neighbouring variable, and
that corruption surfaces far from the call. Treating `&attr` as the attribute object overwrites
the guest's own pointer variable.

**Rejected:**
- Writing a full word everywhere: corrupts the neighbouring stack slot.
- Treating a double pointer as the object: overwrites the guest's variable with a value.
