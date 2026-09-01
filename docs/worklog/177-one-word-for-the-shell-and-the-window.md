# One word for the shell, and the window had already taken it


Before building the shell's front-end, an audit for competing terms. It found one, and not
where the mixing was expected: `orbistoun-gui` described itself as **"the desktop shell"**,
and prosperous pins its GUI version to match "orbistoun's shell" - meaning the window.

So the word already meant two things before the crate existed. Three lines of
self-description, which is nothing; the same collision a week later would have been every
file that mentions either concept, and `shell.rs` inside the window crate would have been
ambiguous on sight.

Principle 13 already supplied the other word. The CLI, the window and worker mode are
*shims*, and a window is a window. `SystemEvent` became `ShellEvent`, `Request::GoHome`
became `Request::ToShell`, "the system button" became "the shell button". `console` stayed -
a console setting is a fact about the machine, which is a different referent rather than a
synonym.

### Where the launch decision went, and why not into `main`

Four outcomes and three ways to contradict itself: `--shell`, `--list`, `--title <name>`, a
stored default, and combinations that mean nothing. Written in `main` it would also be
untestable - nobody writes an assertion against a function that opens a window.

So it is a pure function over an iterator of strings, and the window calls it. The two cases
that justify the separation on their own: repeating a flag is emphasis rather than
contradiction, and `--title --shell` is a *missing name* rather than a title called
`--shell`. Without the second, it reports "no such title" - true, and useless.

Verified end to end rather than only in tests, which is the part that would otherwise have
been assumed:

```
orbistoun: --shell and --list ask for different views; give one of them   (exit 2)
orbistoun: --title needs the name of a title to launch                    (exit 2)
```

### The question worth recording

Asked while this was being built: should the shell render through the *emulated* GPU rather
than through the host UI?

No, and the reason is precise. `orbistoun-gpu` translates guest command streams, and the
shell is not a guest - there is no stream. Routing it through would mean writing the shell as
guest code and running it under the emulator to draw its own menus. The fidelity argument
runs the other way, too: on real hardware the shell is native code on the only GPU there is,
so host-rendering is the faithful choice rather than the shortcut.

The instinct is right about one thing, though, and it is the hard part. An overlay needs the
title's frame (guest -> emulated GPU -> Vulkan) and the shell's UI (egui -> wgpu -> Vulkan)
composited into one image. Today they cannot meet at all: there is no output surface, because
the guest is in a child process. **The overlay is blocked on cross-process presentation, not
on how the shell is drawn** - and when that lands, both sides already being Vulkan makes it
easier rather than harder.

