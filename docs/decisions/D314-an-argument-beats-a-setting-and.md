# D314 - An argument beats a setting, and contradictions are refused rather than resolved


**decided** · 2026-08-27 · a decision with four outcomes, put where it can be tested

Which view the window opens into has three inputs - `--shell`, `--list`, `--title <name>` -
and a stored default, and it is a decision that can be self-contradictory. `--shell --list`
means nothing. `--title` with a flag where a name should be means nothing.

Written in `main` it would also be **untestable**: nobody writes an assertion against a
function that opens a window. So it is a pure function over an iterator of strings in
`orbistoun-shell`, and the window calls it - a decision function plus a thin wrapper, which
is the shape principle 8 already asks for.

Precedence is that an argument always beats the setting. The setting is what somebody wants
*usually*; an argument is what they want *this time*, and a launcher entry that has to say
which view it means would be useless if a preference could override it.

Contradictions are refused with a named cause and a non-zero exit rather than resolved by
picking one. Silently choosing means the flag somebody typed did something other than what
it says, with no way to notice.

Two details that are the whole value of testing it separately: repeating one flag is
emphasis rather than contradiction, and `--title --shell` is a *missing name* rather than a
title called `--shell` - without that, it reports "no such title", which is true and useless.

Unrecognised arguments are ignored rather than refused. The window is re-executed with a
worker flag (D033) and sits on frameworks that take their own; refusing what this module
does not recognise would make it the arbiter of every other crate's command line.

The stored default lives beside the library root in `config.toml`, because both describe how
somebody wants to meet their own collection and neither is a property of one launch.

