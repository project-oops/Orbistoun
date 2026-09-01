# 2026-08-29 - A setting consulted nowhere, for the fourth time


`payload_args` now holds the same handoff block the declared-entry path is handed, so the two
modes agree about what the guest is looking at. It did not move the wall; it is recorded
because it is right rather than because it worked.

The second finding is the one that matters. **Forcing `vsnprintf` to answer a value did
nothing at all** - no report, no change, no sign the knob had been read. `ThunkTable::len`
means the guest's imports, deliberately, so a report cannot flatter itself by counting
everything this emulator can answer (D366) - and every diagnostic was sized by that number,
so by-name resolutions fell outside all of them.

Fourth time a setting has been consulted nowhere: D082, D166, D187, and now this. Each time
the knob existed, was set, reached nothing, and the run reported no change - which reads
exactly like a measurement.

The reports keep `len`; the diagnostics take `total`. It paid immediately: the argument dump
fired for the first time and named the format `klogsrv` renders - `%s:%d:%s: %s` - which is
`klog_printf`'s file, line, function and message, four conversions, all inside the register
half.

The `-1` read is still unexplained after six eliminations. Every pointer into that path is
guarded and the fault is unchanged, at a fixed offset in our own binary. Next step is to look
at where in *our* code it faults - something this project can do to itself and has never
needed to.

