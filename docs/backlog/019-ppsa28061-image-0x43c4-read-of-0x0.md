# PPSA28061 - `image+0x43c4`, read of `0x0`

Nine attempts. Two whole classes are now **eliminated rather than untried**: it is not a
missing import, and it is not a wrong return value - every stub the title calls answering
success leaves the fault byte-identical. What remains is a side effect nobody performed: an
out-parameter never written, or a module asked to load that did not.

**The stack poison has now been tried, and eliminated a third class.** With
`ORBISTOUN_STACK_FILL=5a` the run is byte-identical to the baseline: same fault, same
address, same `rdi=0x3` and `rax=0x9ba49`, same ten textures loaded. So it is **not reading
uninitialised stack** either.

**And direct memory has now been tried too, eliminating a fourth** (D325). With
`ORBISTOUN_DIRECT_FILL=d1` the run is byte-identical again - same fault, same address, same
`rdi=0x3` and `rax=0x0`, same 47 imports and 933 calls - and the fill demonstrably fired:
*17 mapping(s), 71368704 bytes*, all of it before the fault. It is not reading unwritten
direct memory either.

**And with all three poisons at once** - stack, heap and direct memory, each shown to have
fired - the fault is still byte-identical. So the memory class is not narrowed, it is
**closed**: there is nowhere left in guest memory for an unwritten out-parameter to hide.

What is left is the other half of the sentence, and the run report has been naming it every
time:

```
! libSceSysmodule::sceSysmoduleLoadModule was called 3 times and nothing implements it
```

**A module asked to load that did not.** Answering the call `Ok` was tried and changed
nothing - which is the point rather than a setback. The title does not need the call to
succeed; it needs the module to be *present* afterwards, so whatever it looks up next
resolves to something. A return value cannot supply that. **This is now the single
remaining hypothesis for this wall**, and it is a side effect, not an answer. That both walls converged on one class from opposite directions is the most
useful thing either of them has said.

