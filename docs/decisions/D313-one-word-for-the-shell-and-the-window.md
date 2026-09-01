# D313 - One word for the shell, and the window had already taken it


**decided** · 2026-08-27 · found by auditing terminology before building on it

Settling on "the shell" for the system software turned up a collision that predated the
crate: `orbistoun-gui` described itself as **"the desktop shell"**, and prosperous pins its
GUI version to match "orbistoun's shell", meaning the window.

So the word already meant two things - the developer front-end, and the thing a console
presents to a person. Left alone that is not a naming quibble: `shell.rs` inside the window
crate would have been ambiguous on sight, and every doc comment mentioning "the shell" would
have needed the reader to work out which one.

Principle 13 already supplies the other word. The CLI, the window and worker mode are
**shims**, and the window is a window. So "shell" is now reserved for the system software,
and three lines of self-description changed to say so.

The renames that followed: `SystemEvent` became `ShellEvent`, `Request::GoHome` became
`Request::ToShell`, and "the system button" became "the shell button". `console` stays where
it is - a console setting is a fact about the machine rather than about the shell, and that
is a different referent, not a synonym.

**Worth doing before the feature rather than after.** The collision was three lines while the
shell was one crate. It would have been every file that mentions either concept a week later.

