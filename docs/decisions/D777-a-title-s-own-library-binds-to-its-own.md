# D777 - A title's own library binds to its own export

**Status:** decided
**Date:** 2026-10-08
**known_by:** assumed (the import names its library, and the console's linker binds it to that library)

An import a module the title ships resolves is kept by orbistoun only when the import's library is
one orbistoun declares, such as libc. An import from a library only the title ships binds to that
module's export, whatever its name, even where orbistoun implements a function of that name in
another library. The exception runtime still yields to a title module in every library (D769).

**Why:** PPSA25872's executable imports `setenv` from the title's own `Il2cppUserAssemblies`. The
binder asked only whether orbistoun implements the name, so libc's `setenv` took the slot. The
module's own export never ran. An import names its library, and keeping a function for a library
orbistoun does not declare answers a call the console sends elsewhere.
