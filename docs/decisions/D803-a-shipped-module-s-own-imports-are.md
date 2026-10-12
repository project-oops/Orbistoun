# D803 - a shipped module's own imports are served from the package

**Status:** decided
**Date:** 2026-10-12

The title's own modules are found by following imports from every module the title ships, not
just the executable's. That covers the ones it links (D482) and the plugins it loads by path
(D515). Each module's imports are looked up as the executable's are: a platform library is the
title's only in the package's module directory, `sce_module`. The search repeats until nothing new
is needed.

The platform's loader serves a module the libraries that module imports, from the package's module
directory or from its own, when it loads it. So a plugin brings the platform libraries it links
with it.

**Why:** PPSA25872's and PPSA03416's PSN plugins import `libSceNpCppWebApi`, which both titles ship
in `sce_module`, while their executables import nothing from it. The shipped module was never
placed, so the plugins' calls into it landed on orbistoun's stubs. PPSA25872 then logged
`CppWebApi::Common::initialize() failed` and gave up on its online start-up.

**Rejected:**
- Placing everything in `sce_module` up front: a module nothing imports is not loaded by the
  platform, and placing it would run its initialisers for nothing.
- Implementing the library in orbistoun: the title ships its own copy, and the platform loads
  that copy.
