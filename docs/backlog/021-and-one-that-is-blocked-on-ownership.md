# And one that is blocked on ownership, not knowledge

`sceAgcCreateShader`'s argument convention is derived and recorded in its knowledge entry -
out-parameter, header beginning `31 32 33 34` with a size field, bytecode payload. It is
declared in `orbistoun-gpu`, which another session owns, so it is theirs to implement. It
is also the point at which a guest hands over real shader bytecode, which is material that
side has so far only had synthetically.

