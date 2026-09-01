# D428 - libSceSysmodule, and the real-title walls triaged


**measured** - 2026-09-01

With the obSCEne conformance fixes done, ran the real titles - the project's actual loop - to find
what they need. The first wall was shared and clean: `sceSysmoduleLoadModule`, the load call nearly
every title makes, was absent from orbistoun and resolving to the stub-everything placeholder.
PPSA28061 stored that positive placeholder as a module handle and faulted `read of 0` a few calls
later. Added `libSceSysmodule` (LoadModule / UnloadModule / IsLoaded, all success, because every
library a title imports is already resolved by the loader), and PPSA28061 got FURTHER - 47->56
imports - now reaching libSceJson2/libSceNpCppWebApi initialisation.

The walls past it are subsystem- or library-scale, and worth recording so a future session starts
from the map rather than the terminal:

| title | dies at | just-before | what it needs |
|---|---|---|---|
| PPSA28061 | read of 0, image+0x43c4 | libSceJson2 Initializer, libSceNpCppWebApi | C++ library init (Json/Np objects stay null when their initialisers are stubbed) |
| PPSA25872 | read of 0x5, image+0x7b591e | `_Execute_once` (std::call_once), `__cxa_decrement_exception_refcount` | the C++ runtime - call_once must actually call its function (reentrant guest execution), exceptions |
| PPSA02664 | write to 0, image+0xafcc08 | `sceKernelReserveVirtualRange`, `sceKernelVirtualQuery` | virtual-memory management - reserve a range and write the address back; this is the parallel session's mmap/AddressSpace area |

None is a one-call crunch fix. libSceSysmodule was; the rest are the road: a C++ runtime
(call_once/exceptions, reentrant execution), whole libraries (Json, Np), the virtual-memory calls
(the parallel escape workstream's territory), and the GPU translation the Agc/Gnm path leads to.

