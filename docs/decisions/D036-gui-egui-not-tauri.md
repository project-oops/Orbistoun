# D036 - GUI: egui, not Tauri

**decided** · 2026-08-19

The deciding constraint is where emulated video goes. egui shares a Vulkan surface
with the emulator output natively, which is how the menu-as-overlay style most
emulators use actually works. A webview cannot host an emulator framebuffer without
blitting frames into it - the wrong place to spend a frame budget - leaving two
windows and two event loops.

Windows PC is the target. Mobile would be nice eventually and is not worth taxing the
start of the project for; D034 keeps that door open, since a Tauri or mobile shell
would be another shim rather than a rewrite.

egui renders on **our own** Vulkan device via `egui-winit` plus an ash-backed
renderer, not `eframe`'s wgpu - otherwise two graphics stacks contend for the same
GPU and the UI cannot share a surface with the output.

