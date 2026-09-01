# D161 - egui, chosen for the shape of what it has to draw


**decided** · 2026-08-20

`orbistoun-gui` is an immediate-mode application on egui, via `eframe` with the wgpu
backend.

The panels this shell exists to show are a ranked import list, an ordered call tail, a
register dump and a progress verdict. All of them are **replaced wholesale** when a run
finishes rather than edited field by field, which is exactly what immediate mode draws
well - a retained widget tree would spend its effort syncing against state that is thrown
away and rebuilt.

wgpu rather than glow because phase 6 has to present a guest frame eventually, and that
path is clearer through wgpu than through a webview or a GL wrapper. Not built now - there
is no frame - but the toolkit choice is the one part of this that is expensive to revisit.

Considered and not taken: Tauri, which is familiar from other work here, but drags a
webview in and makes raw framebuffer presentation awkward later; and iced, which is tidier
for conventional app layouts and more ceremony for dense diagnostic tables.

The window is checked for worker mode **before it is created**. `WorkerHandle::spawn_self`
re-executes the same binary with a flag, so a shim that cannot serve the protocol cannot
run a guest at all - and reaching the window code in a worker process would open a second
window on every launch.

