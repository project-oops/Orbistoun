# 2026-09-01 - real-title loop: libSceSysmodule (the load call nearly every title makes)


Ran the real titles to mine walls. PPSA28061 faulted read-of-0 right after sceSysmoduleLoadModule
returned the 0x7fff0001 placeholder - the module-load call was resolving to the stub-everything
placeholder, absent from orbistoun entirely. Added libSceSysmodule (nested module in
orbistoun-systemservice, beside libSceUserService): LoadModule/UnloadModule/IsLoaded all succeed,
because every library a title imports is already resolved by the loader, so the module it asks to
load is one it can already call. Answering 0 states that; the placeholder read as a module handle.

PPSA28061 got FURTHER: 47->56 imports, 933->947 calls, and now progresses through libSceJson2 and
libSceNpCppWebApi initialisation before the same read-of-0. That next wall is library-init scale
(the C++ Json/Np objects stay null when their initialisers are stubbed) rather than a one-call fix.
systemservice tests pass; the modules() array bumped 10->11.

